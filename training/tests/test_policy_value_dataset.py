from __future__ import annotations

import sys
import tempfile
import unittest
from pathlib import Path

import numpy as np

TRAINING_ROOT = Path(__file__).resolve().parents[1]
if str(TRAINING_ROOT) not in sys.path:
    sys.path.insert(0, str(TRAINING_ROOT))

from data.policy_value_dataset import (
    PolicyValueTrainingDataset,
    collate_policy_value_batch,
    decode_move_raw,
    piece_id,
)
from utils.example_schema import write_dataset_metadata, group_ids_path
from utils.policy_value_schema import (
    PHASE1_PLAYER_CONTEXT_SCHEMA_VERSION,
    PHASE1_SCHEMA_VERSION,
    PolicyValuePlayerContext,
    PolicyValueTarget,
    encode_move_raw,
    write_player_context_metadata,
    write_player_contexts,
    write_policy_value_targets,
)


class PolicyValueDatasetTests(unittest.TestCase):
    def _write_base_dataset(self, path: Path, sample_count: int) -> None:
        rows = np.zeros((sample_count, 859), dtype=np.float32)
        rows.tofile(path)
        np.asarray([1] * sample_count, dtype=np.uint64).tofile(group_ids_path(path))
        write_dataset_metadata(path, sample_count)

    def test_decode_move_raw_extracts_fields(self) -> None:
        move_raw = 5 | (3 << 6) | (2 << 10) | (1 << 13) | (1 << 15)
        decoded = decode_move_raw(move_raw)
        self.assertEqual(decoded["y"], 5)
        self.assertEqual(decoded["x"], 3)
        self.assertEqual(decoded["piece"], 2)
        self.assertEqual(decoded["rotation"], 1)
        self.assertEqual(decoded["spin"], 1)

    def test_piece_id_matches_move_raw_piece_contract(self) -> None:
        self.assertEqual(piece_id("i"), 0)
        self.assertEqual(piece_id("o"), 1)
        self.assertEqual(piece_id("t"), 2)
        self.assertEqual(piece_id("l"), 3)
        self.assertEqual(piece_id("j"), 4)
        self.assertEqual(piece_id("s"), 5)
        self.assertEqual(piece_id("z"), 6)

    def test_dataset_loads_policy_value_sidecar(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            data_path = Path(tmpdir) / "training_data.bin"
            self._write_base_dataset(data_path, 1)
            write_policy_value_targets(
                data_path,
                [
                    PolicyValueTarget(
                        schema_version=PHASE1_SCHEMA_VERSION,
                        replay_id="r1",
                        round_id=0,
                        player_id=0,
                        frame_id=1,
                        group_id="r1:round:0",
                        best_move_raw=7,
                        best_value=1.5,
                        position_complexity=0.2,
                        root_scores=[(7, 1.5), (9, 0.5)],
                        policy_probs=[0.73, 0.27],
                    )
                ],
            )
            dataset = PolicyValueTrainingDataset(data_path)
            sample = dataset[0]
            self.assertEqual(sample["candidate_move_features"].shape[0], 2)
            self.assertEqual(sample["policy_probs"].shape[0], 2)

    def test_dataset_loads_player_context_primary_lane(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            data_path = Path(tmpdir) / "training_data.bin"
            self._write_base_dataset(data_path, 1)
            write_policy_value_targets(
                data_path,
                [
                    PolicyValueTarget(
                        schema_version=PHASE1_SCHEMA_VERSION,
                        replay_id="r1",
                        round_id=0,
                        player_id=0,
                        frame_id=1,
                        group_id="r1:round:0",
                        best_move_raw=7,
                        best_value=1.5,
                        position_complexity=0.2,
                        root_scores=[(7, 1.5), (9, 0.5)],
                        policy_probs=[0.73, 0.27],
                    )
                ],
            )
            write_player_context_metadata(data_path, sample_count=1, recent_horizon=7, future_horizon=14)
            write_player_contexts(
                data_path,
                [
                    PolicyValuePlayerContext(
                        schema_version=PHASE1_PLAYER_CONTEXT_SCHEMA_VERSION,
                        replay_id="r1",
                        round_id=0,
                        player_id=0,
                        frame_id=1,
                        group_id="r1:round:0",
                        spawn_piece="i",
                        actual_piece="i",
                        actual_move_raw=7,
                        actual_x=0,
                        actual_y=7,
                        actual_rotation=0,
                        actual_hold_used=False,
                        actual_lines_cleared=0,
                        input_keys=["hardDrop"],
                        hold_piece=None,
                        queue=["i", "t"],
                        recent_piece_sequence=["s", "z"],
                        future_piece_sequence=["l", "j"],
                        recent_hold_usage=[False, True],
                        future_hold_usage=[False, False],
                    )
                ],
            )

            dataset = PolicyValueTrainingDataset(data_path, supervision_mode="player_context_primary")
            sample = dataset[0]
            self.assertTrue(bool(sample["player_policy_available"]))
            self.assertEqual(int(sample["player_policy_index"]), 0)
            self.assertEqual(tuple(sample["player_aux_context_features"].shape), (56,))
            self.assertEqual(sample["player_aux_future_piece_ids"].tolist()[:2], [3, 4])
            self.assertEqual(sample["player_aux_future_hold_usage"].tolist()[:2], [0.0, 0.0])

    def test_dataset_prefers_exact_actual_move_raw_over_tuple_match(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            data_path = Path(tmpdir) / "training_data.bin"
            self._write_base_dataset(data_path, 1)
            exact_move = encode_move_raw(piece="t", x=4, y=18, rotation=1)
            wrong_piece_same_tuple = encode_move_raw(piece="i", x=4, y=18, rotation=1)
            write_policy_value_targets(
                data_path,
                [
                    PolicyValueTarget(
                        schema_version=PHASE1_SCHEMA_VERSION,
                        replay_id="r1",
                        round_id=0,
                        player_id=0,
                        frame_id=1,
                        group_id="r1:round:0",
                        best_move_raw=exact_move,
                        best_value=1.5,
                        position_complexity=0.2,
                        root_scores=[(wrong_piece_same_tuple, 0.5), (exact_move, 1.5)],
                        policy_probs=[0.2, 0.8],
                    )
                ],
            )
            write_player_context_metadata(data_path, sample_count=1, recent_horizon=7, future_horizon=14)
            write_player_contexts(
                data_path,
                [
                    PolicyValuePlayerContext(
                        schema_version=PHASE1_PLAYER_CONTEXT_SCHEMA_VERSION,
                        replay_id="r1",
                        round_id=0,
                        player_id=0,
                        frame_id=1,
                        group_id="r1:round:0",
                        spawn_piece="t",
                        actual_piece="t",
                        actual_move_raw=exact_move,
                        actual_x=4,
                        actual_y=18,
                        actual_rotation=1,
                        actual_hold_used=False,
                        actual_lines_cleared=0,
                        input_keys=["rotateCW", "hardDrop"],
                        hold_piece=None,
                        queue=["i", "o"],
                        recent_piece_sequence=[],
                        future_piece_sequence=[],
                        recent_hold_usage=[],
                        future_hold_usage=[],
                    )
                ],
            )
            sample = PolicyValueTrainingDataset(data_path, supervision_mode="player_context_primary")[0]
            self.assertTrue(bool(sample["player_policy_available"]))
            self.assertEqual(int(sample["player_policy_index"]), 1)

    def test_dataset_requires_exact_actual_move_raw_without_tuple_fallback(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            data_path = Path(tmpdir) / "training_data.bin"
            self._write_base_dataset(data_path, 1)
            candidate_move = encode_move_raw(piece="i", x=4, y=18, rotation=1)
            replay_move = encode_move_raw(piece="t", x=4, y=18, rotation=1)
            write_policy_value_targets(
                data_path,
                [
                    PolicyValueTarget(
                        schema_version=PHASE1_SCHEMA_VERSION,
                        replay_id="r1",
                        round_id=0,
                        player_id=0,
                        frame_id=1,
                        group_id="r1:round:0",
                        best_move_raw=candidate_move,
                        best_value=1.5,
                        position_complexity=0.2,
                        root_scores=[(candidate_move, 1.5)],
                        policy_probs=[1.0],
                    )
                ],
            )
            write_player_context_metadata(data_path, sample_count=1, recent_horizon=7, future_horizon=14)
            write_player_contexts(
                data_path,
                [
                    PolicyValuePlayerContext(
                        schema_version=PHASE1_PLAYER_CONTEXT_SCHEMA_VERSION,
                        replay_id="r1",
                        round_id=0,
                        player_id=0,
                        frame_id=1,
                        group_id="r1:round:0",
                        spawn_piece="t",
                        actual_piece="t",
                        actual_move_raw=replay_move,
                        actual_x=4,
                        actual_y=18,
                        actual_rotation=1,
                        actual_hold_used=False,
                        actual_lines_cleared=0,
                        input_keys=["rotateCW", "hardDrop"],
                        hold_piece=None,
                        queue=["i", "o"],
                        recent_piece_sequence=[],
                        future_piece_sequence=[],
                        recent_hold_usage=[],
                        future_hold_usage=[],
                    )
                ],
            )
            sample = PolicyValueTrainingDataset(data_path, supervision_mode="player_context_primary")[0]
            self.assertFalse(bool(sample["player_policy_available"]))
            self.assertEqual(int(sample["player_policy_index"]), -1)

    def test_collate_pads_candidates(self) -> None:
        sample_a = {
            "features": np.zeros(854, dtype=np.float32),
            "candidate_move_features": np.zeros((2, 14), dtype=np.float32),
            "candidate_move_raw": np.asarray([7, 9], dtype=np.int64),
            "search_policy_probs": np.asarray([0.6, 0.4], dtype=np.float32),
            "search_best_value": np.asarray(1.0, dtype=np.float32),
            "search_best_index": np.asarray(0, dtype=np.int64),
            "player_policy_index": np.asarray(0, dtype=np.int64),
            "player_policy_available": np.asarray(True),
            "player_aux_context_features": np.zeros(56, dtype=np.float32),
            "player_aux_future_piece_ids": np.asarray([0, 1], dtype=np.int64),
            "player_aux_future_hold_usage": np.asarray([0.0, 1.0], dtype=np.float32),
        }
        sample_b = {
            "features": np.zeros(854, dtype=np.float32),
            "candidate_move_features": np.zeros((1, 14), dtype=np.float32),
            "candidate_move_raw": np.asarray([5], dtype=np.int64),
            "search_policy_probs": np.asarray([1.0], dtype=np.float32),
            "search_best_value": np.asarray(0.5, dtype=np.float32),
            "search_best_index": np.asarray(0, dtype=np.int64),
            "player_policy_index": np.asarray(-1, dtype=np.int64),
            "player_policy_available": np.asarray(False),
            "player_aux_context_features": np.zeros(56, dtype=np.float32),
            "player_aux_future_piece_ids": np.asarray([6, 6], dtype=np.int64),
            "player_aux_future_hold_usage": np.asarray([1.0, 1.0], dtype=np.float32),
        }
        import torch
        batch = collate_policy_value_batch(
            [
                {key: torch.from_numpy(value) if isinstance(value, np.ndarray) else torch.tensor(value) for key, value in sample_a.items()},
                {key: torch.from_numpy(value) if isinstance(value, np.ndarray) else torch.tensor(value) for key, value in sample_b.items()},
            ]
        )
        self.assertEqual(tuple(batch["candidate_move_features"].shape), (2, 2, 14))
        self.assertFalse(bool(batch["candidate_mask"][1, 1]))
        self.assertEqual(batch["player_policy_index"].tolist(), [0, -1])
        self.assertEqual(batch["player_policy_available"].tolist(), [True, False])
        self.assertEqual(tuple(batch["player_aux_context_features"].shape), (2, 56))
