use std::fs;
use std::path::PathBuf;
use std::process::Command;

use serde_json::Value;

#[test]
fn parity_fixture_generator_emits_contract_shape() {
    let metadata_path = PathBuf::from("models/rebal-r01/checkpoint.ckpt.policy_value.onnx.metadata.json");
    if !metadata_path.exists() {
        eprintln!(
            "skipping parity fixture test: {} is missing",
            metadata_path.display()
        );
        return;
    }

    let output_path = std::env::temp_dir().join(format!(
        "policy-value-runtime-parity-{}.json",
        std::process::id()
    ));
    let _ = fs::remove_file(&output_path);

    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    let status = Command::new(cargo)
        .args(["run", "--quiet", "--bin", "policy_value_runtime_parity_fixture", "--"])
        .arg(&metadata_path)
        .arg(&output_path)
        .status()
        .expect("run parity fixture generator");
    assert!(status.success(), "fixture generator exited with {status}");

    let fixture: Value = serde_json::from_str(
        &fs::read_to_string(&output_path).expect("read generated parity fixture"),
    )
    .expect("parse generated parity fixture");

    assert_eq!(fixture["schema_version"], "runtime-parity-v1");
    assert_eq!(fixture["state_feature_dim"], 854);
    assert_eq!(fixture["move_feature_dim"], 14);
    assert_eq!(fixture["candidate_capacity"], 64);
    assert_eq!(fixture["scalar_scope"], "zero-only");

    let positions = fixture["positions"]
        .as_array()
        .expect("positions array");
    assert!(positions.len() >= 3, "expected at least three parity positions");

    for position in positions {
        let source = position["source"].as_object().expect("source object");
        assert_eq!(source["combo"], 0);
        assert_eq!(source["b2b"], 0);
        assert_eq!(source["lines_total"], 0);
        assert_eq!(source["pending_garbage"], 0);
        assert_eq!(source["bag_number"], 0);

        let current_piece = source["current_piece"]
            .as_u64()
            .expect("external current piece id");
        assert!(current_piece <= 6, "piece ids use external WASM order");
        for piece in source["queue"].as_array().expect("queue array") {
            assert!(piece.as_u64().expect("external queue piece id") <= 6);
        }
        if !source["hold"].is_null() {
            assert!(source["hold"].as_u64().expect("external hold piece id") <= 6);
        }

        let state_features = position["state_features"]
            .as_array()
            .expect("state features");
        let candidate_features = position["candidate_features"]
            .as_array()
            .expect("candidate features");
        let candidate_mask = position["candidate_mask"]
            .as_array()
            .expect("candidate mask");
        let moves = position["moves"].as_array().expect("move descriptors");
        let move_count = position["move_count"].as_u64().expect("move count") as usize;
        let native_logits = position["native"]["policy_logits"]
            .as_array()
            .expect("native logits");

        assert_eq!(state_features.len(), 854);
        assert_eq!(candidate_features.len(), 64 * 14);
        assert_eq!(candidate_mask.len(), 64);
        assert!(move_count > 0 && move_count <= 64);
        assert_eq!(moves.len(), move_count);
        assert_eq!(native_logits.len(), move_count);

        let true_count = candidate_mask
            .iter()
            .filter(|value| value.as_bool().expect("mask bool"))
            .count();
        assert_eq!(true_count, move_count);
        for (index, value) in candidate_mask.iter().enumerate() {
            assert_eq!(value.as_bool().expect("mask bool"), index < move_count);
        }

        for row in move_count..64 {
            let base = row * 14;
            for value in &candidate_features[base..base + 14] {
                assert_eq!(value.as_f64().expect("candidate feature"), 0.0);
            }
        }

        for (index, descriptor) in moves.iter().enumerate() {
            assert_eq!(descriptor["index"].as_u64().expect("move index") as usize, index);
            assert!(descriptor["raw"].as_u64().expect("raw move id") <= u16::MAX as u64);
            assert!(descriptor["piece"].as_u64().expect("external move piece id") <= 6);
            assert!(descriptor["rotation"].as_u64().expect("rotation") <= 3);
            assert!(descriptor["spin"].as_u64().expect("spin") <= 2);
        }

        let best_index = position["native"]["best_index"]
            .as_u64()
            .expect("best index") as usize;
        assert!(best_index < move_count);
        assert_eq!(
            position["native"]["best_raw"],
            moves[best_index]["raw"],
            "best_raw must match the best move descriptor"
        );

        assert!(position["rank"]["top1_margin"].as_f64().is_some());
        assert!(position["rank"]["top3_adjacent_min_margin"].as_f64().is_some());
        assert!(position["rank"]["rank_checks_enabled"].as_bool().is_some());
    }

    let _ = fs::remove_file(output_path);
}
