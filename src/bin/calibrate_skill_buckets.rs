use direct_cobra_copy::calibration::{
    generate_profile_from_players_manifest, CalibrationProfile, CALIBRATION_VERSION_V1,
};
use std::path::Path;

const DEFAULT_MANIFEST_PATH: &str =
    "data/replay-corpus/s2-ranked-1v1-rd70-165/manifests/players_manifest.json";
const DEFAULT_OUTPUT_PATH: &str = "data/calibration/skill_bucket_calibration_v1.cal";

fn create_parent_dir(path: &Path) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn write_profile(output_path: &Path, profile: &CalibrationProfile) -> Result<(), String> {
    create_parent_dir(output_path)?;
    std::fs::write(output_path, profile.to_artifact_string()).map_err(|e| e.to_string())
}

fn main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().collect();
    let manifest_path = args
        .get(1)
        .map(String::as_str)
        .unwrap_or(DEFAULT_MANIFEST_PATH);
    let output_path = args
        .get(2)
        .map(String::as_str)
        .unwrap_or(DEFAULT_OUTPUT_PATH);

    let manifest_content = std::fs::read_to_string(manifest_path).map_err(|e| e.to_string())?;
    let profile =
        generate_profile_from_players_manifest(CALIBRATION_VERSION_V1, &manifest_content)?;

    let output = Path::new(output_path);
    write_profile(output, &profile)?;

    println!("generated calibration profile v{}", profile.version);
    println!("source_fingerprint={}", profile.source_fingerprint);
    println!("rows={}", profile.rows.len());
    println!("output={}", output.display());

    Ok(())
}
