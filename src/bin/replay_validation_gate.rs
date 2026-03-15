use direct_cobra_copy::replay_validation::{
    evaluate_replay_samples, parse_replay_samples_from_players_manifest, render_replay_gate_report,
    ReplayGateThresholds,
};
use std::path::Path;

const DEFAULT_MANIFEST_PATH: &str =
    "data/replay-corpus/s2-ranked-1v1-rd70-165/manifests/players_manifest.json";
const DEFAULT_REPORT_PATH: &str = "data/reports/replay-metrics.txt";

fn create_parent_dir(path: &Path) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().collect();
    let manifest_path = args
        .get(1)
        .map(String::as_str)
        .unwrap_or(DEFAULT_MANIFEST_PATH);
    let report_path = args
        .get(2)
        .map(String::as_str)
        .unwrap_or(DEFAULT_REPORT_PATH);

    let manifest_content = std::fs::read_to_string(manifest_path).map_err(|e| e.to_string())?;
    let samples = parse_replay_samples_from_players_manifest(&manifest_content)?;
    let thresholds = ReplayGateThresholds::strict_profile();
    let evaluation = evaluate_replay_samples(&samples, thresholds)?;
    let report = render_replay_gate_report(&evaluation);

    let output_path = Path::new(report_path);
    create_parent_dir(output_path)?;
    std::fs::write(output_path, report).map_err(|e| e.to_string())?;

    println!(
        "replay_gate_status={}",
        if evaluation.passed { "PASS" } else { "FAIL" }
    );
    println!(
        "metrics severe_recall={:.6} false_severe_rate={:.6} obligation_compliance={:.6}",
        evaluation.metrics.severe_recall,
        evaluation.metrics.false_severe_rate,
        evaluation.metrics.obligation_compliance,
    );
    println!("determinism_hash={}", evaluation.metrics.determinism_hash);
    println!("report={}", output_path.display());

    if !evaluation.passed {
        return Err(format!(
            "replay promotion gate failed: {}",
            evaluation.failures.join("; ")
        ));
    }

    Ok(())
}
