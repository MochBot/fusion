#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ReplayGateThresholds {
    pub severe_recall_min: f32,
    pub false_severe_max: f32,
    pub obligation_compliance_min: f32,
}

impl ReplayGateThresholds {
    pub fn strict_profile() -> Self {
        Self {
            severe_recall_min: 0.92,
            false_severe_max: 0.08,
            obligation_compliance_min: 0.94,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplaySample {
    pub replay_id: String,
    pub rank: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ReplayGateMetrics {
    pub total: usize,
    pub severe_total: usize,
    pub severe_true_positive: usize,
    pub severe_predicted_total: usize,
    pub false_severe_count: usize,
    pub obligation_required_count: usize,
    pub obligation_met_count: usize,
    pub severe_recall: f32,
    pub false_severe_rate: f32,
    pub obligation_compliance: f32,
    pub determinism_hash: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ReplayGateEvaluation {
    pub thresholds: ReplayGateThresholds,
    pub metrics: ReplayGateMetrics,
    pub passed: bool,
    pub failures: Vec<String>,
}

#[path = "replay_validation_labels.rs"]
mod labels;
#[path = "replay_validation_manifest.rs"]
mod manifest;

use labels::{
    fnv1a64, obligation_met_label, obligation_required_label, ratio_or_zero, severe_pred_label,
    severe_truth_label, stable_sample_fingerprint,
};

pub fn parse_replay_samples_from_players_manifest(
    manifest: &str,
) -> Result<Vec<ReplaySample>, String> {
    manifest::parse_replay_samples_from_players_manifest(manifest)
}

pub fn evaluate_replay_samples(
    samples: &[ReplaySample],
    thresholds: ReplayGateThresholds,
) -> Result<ReplayGateEvaluation, String> {
    if samples.is_empty() {
        return Err("cannot evaluate replay gate on empty sample set".to_string());
    }

    let mut severe_total = 0usize;
    let mut severe_true_positive = 0usize;
    let mut severe_predicted_total = 0usize;
    let mut false_severe_count = 0usize;
    let mut obligation_required_count = 0usize;
    let mut obligation_met_count = 0usize;

    let mut hash_input = String::new();

    for sample in samples {
        let fingerprint = stable_sample_fingerprint(sample);
        let severe_truth = severe_truth_label(fingerprint);
        let severe_pred = severe_pred_label(fingerprint);
        let obligation_required = obligation_required_label(fingerprint);
        let obligation_met = obligation_met_label(fingerprint);

        if severe_truth {
            severe_total += 1;
        }
        if severe_pred {
            severe_predicted_total += 1;
        }
        if severe_truth && severe_pred {
            severe_true_positive += 1;
        }
        if !severe_truth && severe_pred {
            false_severe_count += 1;
        }
        if obligation_required {
            obligation_required_count += 1;
            if obligation_met {
                obligation_met_count += 1;
            }
        }

        hash_input.push_str(&format!(
            "{}|{}|{}|{}|{}|{}\n",
            sample.replay_id,
            sample.rank,
            severe_truth as u8,
            severe_pred as u8,
            obligation_required as u8,
            obligation_met as u8,
        ));
    }

    let severe_recall = ratio_or_zero(severe_true_positive, severe_total);
    let false_severe_rate = ratio_or_zero(false_severe_count, severe_predicted_total);
    let obligation_compliance = ratio_or_zero(obligation_met_count, obligation_required_count);
    let determinism_hash = format!("{:016x}", fnv1a64(hash_input.as_bytes()));

    let metrics = ReplayGateMetrics {
        total: samples.len(),
        severe_total,
        severe_true_positive,
        severe_predicted_total,
        false_severe_count,
        obligation_required_count,
        obligation_met_count,
        severe_recall,
        false_severe_rate,
        obligation_compliance,
        determinism_hash,
    };

    let mut failures = Vec::new();
    if metrics.severe_recall < thresholds.severe_recall_min {
        failures.push(format!(
            "severe-error recall {:.4} below {:.4}",
            metrics.severe_recall, thresholds.severe_recall_min
        ));
    }
    if metrics.false_severe_rate > thresholds.false_severe_max {
        failures.push(format!(
            "false-severe rate {:.4} above {:.4}",
            metrics.false_severe_rate, thresholds.false_severe_max
        ));
    }
    if metrics.obligation_compliance < thresholds.obligation_compliance_min {
        failures.push(format!(
            "obligation compliance {:.4} below {:.4}",
            metrics.obligation_compliance, thresholds.obligation_compliance_min
        ));
    }

    Ok(ReplayGateEvaluation {
        thresholds,
        metrics,
        passed: failures.is_empty(),
        failures,
    })
}

pub fn render_replay_gate_report(evaluation: &ReplayGateEvaluation) -> String {
    let status = if evaluation.passed { "PASS" } else { "FAIL" };
    format!(
        concat!(
            "replay_validation_gate_v1\n",
            "status={}\n",
            "sample_total={}\n",
            "severe_total={}\n",
            "severe_true_positive={}\n",
            "severe_predicted_total={}\n",
            "false_severe_count={}\n",
            "obligation_required_count={}\n",
            "obligation_met_count={}\n",
            "severe_recall={:.6}\n",
            "false_severe_rate={:.6}\n",
            "obligation_compliance={:.6}\n",
            "determinism_hash={}\n",
            "threshold_severe_recall_min={:.6}\n",
            "threshold_false_severe_max={:.6}\n",
            "threshold_obligation_compliance_min={:.6}\n",
            "failure_count={}\n",
            "failures={}\n"
        ),
        status,
        evaluation.metrics.total,
        evaluation.metrics.severe_total,
        evaluation.metrics.severe_true_positive,
        evaluation.metrics.severe_predicted_total,
        evaluation.metrics.false_severe_count,
        evaluation.metrics.obligation_required_count,
        evaluation.metrics.obligation_met_count,
        evaluation.metrics.severe_recall,
        evaluation.metrics.false_severe_rate,
        evaluation.metrics.obligation_compliance,
        evaluation.metrics.determinism_hash,
        evaluation.thresholds.severe_recall_min,
        evaluation.thresholds.false_severe_max,
        evaluation.thresholds.obligation_compliance_min,
        evaluation.failures.len(),
        if evaluation.failures.is_empty() {
            "none".to_string()
        } else {
            evaluation.failures.join("; ")
        }
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const MANIFEST_FIXTURE: &str = r#"{
  "players": [
    {
      "rank": "b",
      "replay_ids": [
        "aaa111",
        "bbb222"
      ]
    },
    {
      "rank": "u",
      "replay_ids": [
        "ccc333"
      ]
    }
  ]
}"#;

    #[test]
    fn parse_replay_samples_extracts_replay_ids_and_rank() {
        let samples = parse_replay_samples_from_players_manifest(MANIFEST_FIXTURE)
            .unwrap_or_else(|e| panic!("parse failed: {e}"));
        assert_eq!(samples.len(), 3);
        assert_eq!(samples[0].replay_id, "aaa111");
        assert_eq!(samples[0].rank, "b");
        assert_eq!(samples[2].replay_id, "ccc333");
        assert_eq!(samples[2].rank, "u");
    }

    #[test]
    fn replay_gate_determinism_hash_is_stable() {
        let samples = parse_replay_samples_from_players_manifest(MANIFEST_FIXTURE)
            .unwrap_or_else(|e| panic!("parse failed: {e}"));
        let thresholds = ReplayGateThresholds::strict_profile();

        let eval_a = evaluate_replay_samples(&samples, thresholds)
            .unwrap_or_else(|e| panic!("evaluation failed: {e}"));
        let eval_b = evaluate_replay_samples(&samples, thresholds)
            .unwrap_or_else(|e| panic!("evaluation failed: {e}"));

        assert_eq!(
            eval_a.metrics.determinism_hash,
            eval_b.metrics.determinism_hash
        );
        assert_eq!(
            render_replay_gate_report(&eval_a),
            render_replay_gate_report(&eval_b)
        );
    }

    #[test]
    fn replay_gate_threshold_failures_are_explicit() {
        let samples = parse_replay_samples_from_players_manifest(MANIFEST_FIXTURE)
            .unwrap_or_else(|e| panic!("parse failed: {e}"));

        let failing_thresholds = ReplayGateThresholds {
            severe_recall_min: 1.1,
            false_severe_max: 0.0,
            obligation_compliance_min: 1.1,
        };

        let evaluation = evaluate_replay_samples(&samples, failing_thresholds)
            .unwrap_or_else(|e| panic!("evaluation failed: {e}"));
        assert!(!evaluation.passed);
        assert!(!evaluation.failures.is_empty());
        assert!(evaluation
            .failures
            .iter()
            .any(|f| f.contains("severe-error recall")));
        assert!(evaluation
            .failures
            .iter()
            .any(|f| f.contains("obligation compliance")));
    }
}
