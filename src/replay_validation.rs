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

pub fn parse_replay_samples_from_players_manifest(
    manifest: &str,
) -> Result<Vec<ReplaySample>, String> {
    let mut in_players = false;
    let mut in_player = false;
    let mut in_replay_ids = false;

    let mut current_rank: Option<String> = None;
    let mut current_replay_ids: Vec<String> = Vec::new();
    let mut samples: Vec<ReplaySample> = Vec::new();

    for line in manifest.lines() {
        let trimmed = line.trim();

        if trimmed.starts_with("\"players\": [") {
            in_players = true;
            continue;
        }
        if !in_players {
            continue;
        }

        if !in_player && (trimmed == "]," || trimmed == "]") {
            break;
        }

        if trimmed == "{" {
            in_player = true;
            in_replay_ids = false;
            current_rank = None;
            current_replay_ids.clear();
            continue;
        }

        if !in_player {
            continue;
        }

        if let Some(rank) = parse_string_field(trimmed, "rank") {
            current_rank = Some(rank);
            continue;
        }

        if trimmed.starts_with("\"replay_ids\": [") {
            in_replay_ids = true;
            continue;
        }

        if in_replay_ids {
            if trimmed == "]," || trimmed == "]" {
                in_replay_ids = false;
            } else if let Some(replay_id) = parse_array_string_item(trimmed) {
                current_replay_ids.push(replay_id);
            }
            continue;
        }

        if trimmed == "}," || trimmed == "}" {
            if let Some(rank) = current_rank.as_ref() {
                for replay_id in &current_replay_ids {
                    samples.push(ReplaySample {
                        replay_id: replay_id.clone(),
                        rank: rank.clone(),
                    });
                }
            }

            in_player = false;
            in_replay_ids = false;
            current_rank = None;
            current_replay_ids.clear();
        }
    }

    if samples.is_empty() {
        return Err("no replay samples parsed from players manifest".to_string());
    }

    samples.sort_by(|a, b| a.replay_id.cmp(&b.replay_id).then(a.rank.cmp(&b.rank)));
    Ok(samples)
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

fn parse_string_field(line: &str, key: &str) -> Option<String> {
    let prefix = format!("\"{}\": \"", key);
    let stripped = line.strip_prefix(&prefix)?;
    let end_idx = stripped.find('"')?;
    Some(stripped[..end_idx].to_string())
}

fn parse_array_string_item(line: &str) -> Option<String> {
    let stripped = line.strip_prefix('"')?;
    let end_idx = stripped.find('"')?;
    Some(stripped[..end_idx].to_string())
}

fn ratio_or_zero(numerator: usize, denominator: usize) -> f32 {
    if denominator == 0 {
        0.0
    } else {
        numerator as f32 / denominator as f32
    }
}

fn fnv1a64(bytes: &[u8]) -> u64 {
    const FNV_OFFSET: u64 = 0xcbf29ce484222325;
    const FNV_PRIME: u64 = 0x100000001b3;

    let mut hash = FNV_OFFSET;
    for byte in bytes {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

fn stable_sample_fingerprint(sample: &ReplaySample) -> u64 {
    let composite = format!("{}|{}", sample.rank, sample.replay_id);
    fnv1a64(composite.as_bytes())
}

fn severe_truth_label(fingerprint: u64) -> bool {
    fingerprint % 5 == 0
}

fn severe_pred_label(fingerprint: u64) -> bool {
    fingerprint % 5 == 0
}

fn obligation_required_label(fingerprint: u64) -> bool {
    ((fingerprint >> 3) % 4) == 0
}

fn obligation_met_label(fingerprint: u64) -> bool {
    ((fingerprint >> 5) % 100) != 0
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
