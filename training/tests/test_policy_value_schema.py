from __future__ import annotations

import sys
import tempfile
import unittest
from pathlib import Path

TRAINING_ROOT = Path(__file__).resolve().parents[1]
if str(TRAINING_ROOT) not in sys.path:
    sys.path.insert(0, str(TRAINING_ROOT))

from utils import policy_value_schema as schema


class PolicyValueSchemaTests(unittest.TestCase):
    def test_metadata_round_trip(self) -> None:
        metadata = schema.build_policy_value_metadata(sample_count=3, generation_mode="search_oracle", policy_temperature=1.0)
        schema.validate_policy_value_metadata(metadata)
        self.assertEqual(metadata["schema_version"], schema.PHASE1_SCHEMA_VERSION)
        self.assertEqual(metadata["contract_version"], schema.POLICY_VALUE_CONTRACT_VERSION)
        self.assertEqual(metadata["shared_input_contract"], schema.POLICY_VALUE_SHARED_INPUT_CONTRACT)

    def test_softmax_normalizes_root_scores(self) -> None:
        probs = schema.softmax_root_scores([1.0, 2.0, 3.0], temperature=1.0)
        self.assertAlmostEqual(sum(probs), 1.0, places=6)
        self.assertGreater(probs[2], probs[1])
        self.assertGreater(probs[1], probs[0])

    def test_target_validation_rejects_empty_roots(self) -> None:
        target = schema.PolicyValueTarget(
            schema_version=schema.PHASE1_SCHEMA_VERSION,
            replay_id="r1",
            round_id=0,
            player_id=0,
            frame_id=10,
            group_id="r1:round:0",
            best_move_raw=12,
            best_value=1.0,
            position_complexity=0.5,
            root_scores=[],
            policy_probs=[],
        )
        with self.assertRaises(ValueError):
            schema.validate_policy_value_target(target)

    def test_policy_value_sidecar_paths_and_round_trip(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            data_path = Path(tmpdir) / "training_data.bin"
            target = schema.PolicyValueTarget(
                schema_version=schema.PHASE1_SCHEMA_VERSION,
                replay_id="r1",
                round_id=0,
                player_id=0,
                frame_id=1,
                group_id="r1:round:0",
                best_move_raw=9,
                best_value=1.5,
                position_complexity=0.3,
                root_scores=[(7, 0.5), (9, 1.5)],
                policy_probs=[0.2689414213699951, 0.7310585786300049],
            )
            schema.write_policy_value_metadata(
                data_path,
                sample_count=1,
                generation_mode=schema.GENERATION_MODE_SEARCH_ORACLE,
                policy_temperature=1.0,
                oracle_profile=schema.ORACLE_PROFILE_STRONGER_OFFLINE,
                oracle_beam_width=2000,
                oracle_depth=18,
                oracle_use_tt=True,
            )
            schema.write_policy_value_targets(data_path, [target])

            metadata = schema.load_policy_value_metadata(data_path)
            loaded = schema.load_policy_value_targets(data_path)
            self.assertEqual(metadata["sample_count"], 1)
            self.assertEqual(metadata["oracle_profile"], schema.ORACLE_PROFILE_STRONGER_OFFLINE)
            self.assertEqual(metadata["oracle_beam_width"], 2000)
            self.assertEqual(len(loaded), 1)
            self.assertEqual(loaded[0].best_move_raw, 9)

    def test_player_context_metadata_round_trip(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            data_path = Path(tmpdir) / "training_data.bin"
            schema.write_player_context_metadata(
                data_path,
                sample_count=2,
                recent_horizon=7,
                future_horizon=14,
            )
            metadata = schema.load_player_context_metadata(data_path)
            self.assertEqual(metadata["sample_count"], 2)
            self.assertEqual(metadata["generation_mode"], schema.GENERATION_MODE_PLAYER_CONTEXT)
            self.assertEqual(metadata["recent_horizon"], 7)
            self.assertEqual(metadata["future_horizon"], 14)

    def test_player_context_sidecar_round_trip(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            data_path = Path(tmpdir) / "training_data.bin"
            context = schema.PolicyValuePlayerContext(
                schema_version=schema.PHASE1_PLAYER_CONTEXT_SCHEMA_VERSION,
                replay_id="r1",
                round_id=0,
                player_id=1,
                frame_id=10,
                group_id="r1:round:0",
                spawn_piece="i",
                actual_piece="t",
                actual_move_raw=schema.encode_move_raw(piece="t", x=4, y=18, rotation=1),
                actual_x=4,
                actual_y=18,
                actual_rotation=1,
                actual_hold_used=True,
                actual_lines_cleared=2,
                input_keys=["hold", "rotateCW", "hardDrop"],
                hold_piece="o",
                queue=["s", "z", "j"],
                recent_piece_sequence=["i", "o"],
                future_piece_sequence=["l", "j", "s"],
                recent_hold_usage=[False, True],
                future_hold_usage=[False, False, True],
            )
            schema.write_player_contexts(data_path, [context])

            loaded = schema.load_player_contexts(data_path)
            self.assertEqual(len(loaded), 1)
            self.assertEqual(loaded[0].spawn_piece, "i")
            self.assertEqual(loaded[0].actual_piece, "t")
            self.assertEqual(loaded[0].actual_move_raw, schema.encode_move_raw(piece="t", x=4, y=18, rotation=1))
            self.assertEqual(loaded[0].input_keys, ["hold", "rotateCW", "hardDrop"])
            self.assertEqual(loaded[0].future_piece_sequence, ["l", "j", "s"])

    def test_metadata_validation_rejects_invalid_contract_version(self) -> None:
        metadata = schema.build_policy_value_metadata(sample_count=1, generation_mode="search_oracle", policy_temperature=1.0)
        metadata["contract_version"] = "phase1-only"
        with self.assertRaisesRegex(ValueError, "Invalid contract version"):
            schema.validate_policy_value_metadata(metadata)

    def test_deserialize_policy_value_target_renormalizes_near_unity_probs(self) -> None:
        payload = (
            '{"best_move_raw":9,"best_value":1.5,"frame_id":49,"group_id":"g1","player_id":0,'
            '"policy_probs":[0.5,0.499998870762566],"position_complexity":0.3,'
            '"replay_id":"025929ebe4a4","root_scores":[[7,0.5],[9,1.5]],"round_id":0,'
            '"schema_version":"phase1-v1"}'
        )
        target = schema.deserialize_policy_value_target(payload)
        self.assertAlmostEqual(sum(target.policy_probs), 1.0, places=12)
        self.assertEqual(len(target.policy_probs), 2)

    def test_deserialize_policy_value_target_rejects_materially_bad_probs(self) -> None:
        payload = (
            '{"best_move_raw":9,"best_value":1.5,"frame_id":49,"group_id":"g1","player_id":0,'
            '"policy_probs":[0.5,0.49],"position_complexity":0.3,'
            '"replay_id":"025929ebe4a4","root_scores":[[7,0.5],[9,1.5]],"round_id":0,'
            '"schema_version":"phase1-v1"}'
        )
        with self.assertRaisesRegex(ValueError, "policy_probs must sum to 1"):
            schema.deserialize_policy_value_target(payload)
