from __future__ import annotations

import json
import sys
import tempfile
import unittest
from pathlib import Path

import numpy as np

TRAINING_ROOT = Path(__file__).resolve().parents[1]
if str(TRAINING_ROOT) not in sys.path:
    sys.path.insert(0, str(TRAINING_ROOT))

from scripts.preprocess_replays import (
    merge_preprocessed_shards,
    preprocess_directory,
    process_file,
    split_replay_files,
)
from utils.example_schema import group_ids_path, load_dataset_metadata
from utils.policy_value_schema import (
    load_player_context_metadata,
    load_player_contexts,
    load_policy_value_oracle_requests,
    policy_value_player_context_path,
    policy_value_requests_path,
)


def _player_replay(*, frame: int, winner: bool) -> dict[str, object]:
    return {
        "replay": {
            "frames": 10,
            "events": [
                {
                    "type": "full",
                    "data": {
                        "game": {
                            "board": [],
                            "bag": ["o", "t", "s", "z", "j", "l", "i"],
                            "hold": {},
                            "falling": {"type": "i", "x": 3, "y": 0, "r": 0},
                        },
                        "stats": {"combo": 0, "btb": 0, "lines": 0, "piecesplaced": 0},
                    },
                },
                {"type": "keydown", "frame": frame, "data": {"key": "hardDrop"}},
                {
                    "type": "end",
                    "data": {
                        "gameoverreason": "winner" if winner else "topout",
                        "stats": {"garbage": {"sent": 4 if winner else 0}},
                    },
                },
            ],
        }
    }


class PreprocessContractTests(unittest.TestCase):
    def test_split_replay_files_preserves_all_inputs_once(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            input_dir = Path(tmpdir) / "replays"
            input_dir.mkdir()
            for index in range(5):
                replay = {"replay": {"rounds": [[_player_replay(frame=1, winner=True), _player_replay(frame=2, winner=False)]]}}
                (input_dir / f"sample-{index}.ttrm").write_text(json.dumps(replay))

            shards = split_replay_files(input_dir, shard_count=3)
            self.assertEqual(len(shards), 3)
            flattened = [path.name for shard in shards for path in shard]
            self.assertEqual(sorted(flattened), [f"sample-{index}.ttrm" for index in range(5)])
            self.assertEqual(len(flattened), len(set(flattened)))

    def test_process_file_aligns_opponent_snapshot_by_frame(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            replay_path = Path(tmpdir) / "sample.ttrm"
            replay = {"replay": {"rounds": [[_player_replay(frame=2, winner=True), _player_replay(frame=1, winner=False)]]}}
            replay_path.write_text(json.dumps(replay))

            samples = process_file(replay_path)
            self.assertEqual(len(samples), 2)
            p1_early = next(sample for sample in samples if sample.identity.player_id == 1)
            p0_late = next(sample for sample in samples if sample.identity.player_id == 0)
            self.assertEqual(p1_early.identity.group_id, p0_late.identity.group_id)
            self.assertEqual(float(p1_early.opponent_board.sum()), 0.0)
            self.assertGreater(float(p0_late.opponent_board.sum()), 0.0)

    def test_preprocess_directory_writes_metadata_and_groups(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            input_dir = Path(tmpdir) / "replays"
            output_path = Path(tmpdir) / "training_data.bin"
            input_dir.mkdir()
            replay = {"replay": {"rounds": [[_player_replay(frame=1, winner=True), _player_replay(frame=2, winner=False)]]}}
            (input_dir / "sample.ttrm").write_text(json.dumps(replay))

            sample_count = preprocess_directory(input_dir, output_path, num_workers=1)
            metadata = load_dataset_metadata(output_path)
            self.assertEqual(sample_count, 2)
            self.assertEqual(metadata["sample_count"], 2)
            self.assertTrue(group_ids_path(output_path).exists())
            group_values = np.fromfile(group_ids_path(output_path), dtype=np.uint64)
            self.assertEqual(len(group_values), 2)
            self.assertEqual(group_values[0], group_values[1])
            self.assertTrue(policy_value_requests_path(output_path).exists())

            requests = load_policy_value_oracle_requests(output_path)
            self.assertEqual(len(requests), 2)
            ordered_requests = sorted(requests, key=lambda request: request.frame_id)
            early, late = ordered_requests
            self.assertEqual(early.group_id, late.group_id)
            self.assertEqual(sum(early.opponent_board_rows), 0)
            self.assertGreater(sum(late.opponent_board_rows), 0)

    def test_preprocess_directory_writes_player_context_sidecar(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            input_dir = Path(tmpdir) / "replays"
            output_path = Path(tmpdir) / "training_data.bin"
            input_dir.mkdir()
            replay = {"replay": {"rounds": [[_player_replay(frame=1, winner=True), _player_replay(frame=2, winner=False)]]}}
            (input_dir / "sample.ttrm").write_text(json.dumps(replay))

            sample_count = preprocess_directory(input_dir, output_path, num_workers=1)
            self.assertEqual(sample_count, 2)
            self.assertTrue(policy_value_player_context_path(output_path).exists())

            metadata = load_player_context_metadata(output_path)
            self.assertEqual(metadata["sample_count"], 2)
            self.assertEqual(metadata["recent_horizon"], 7)
            self.assertEqual(metadata["future_horizon"], 14)

            contexts = load_player_contexts(output_path)
            self.assertEqual(len(contexts), 2)
            ordered = sorted(contexts, key=lambda context: context.frame_id)
            early, late = ordered
            self.assertEqual(early.group_id, late.group_id)
            self.assertEqual(early.input_keys, ["hardDrop"])
            self.assertEqual(early.spawn_piece, "i")
            self.assertEqual(early.actual_piece, "i")
            self.assertFalse(early.actual_hold_used)
            self.assertEqual(early.recent_piece_sequence, [])
            self.assertEqual(early.future_piece_sequence, [])
            self.assertGreaterEqual(early.actual_y, 0)

    def test_preprocess_directory_preserves_request_and_context_row_alignment(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            input_dir = Path(tmpdir) / "replays"
            output_path = Path(tmpdir) / "training_data.bin"
            input_dir.mkdir()
            replay = {"replay": {"rounds": [[_player_replay(frame=2, winner=True), _player_replay(frame=1, winner=False)]]}}
            (input_dir / "sample.ttrm").write_text(json.dumps(replay))

            sample_count = preprocess_directory(input_dir, output_path, num_workers=1)
            self.assertEqual(sample_count, 2)

            requests = load_policy_value_oracle_requests(output_path)
            contexts = load_player_contexts(output_path)
            self.assertEqual(
                [
                    (request.replay_id, request.round_id, request.player_id, request.frame_id)
                    for request in requests
                ],
                [
                    (context.replay_id, context.round_id, context.player_id, context.frame_id)
                    for context in contexts
                ],
            )

    def test_merge_preprocessed_shards_matches_single_process_outputs(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            input_dir = Path(tmpdir) / "replays"
            input_dir.mkdir()
            for index in range(4):
                replay = {"replay": {"rounds": [[_player_replay(frame=index + 1, winner=(index % 2 == 0)), _player_replay(frame=index + 2, winner=(index % 2 == 1))]]}}
                (input_dir / f"sample-{index}.ttrm").write_text(json.dumps(replay))

            single_output = Path(tmpdir) / "single.bin"
            merged_output = Path(tmpdir) / "merged.bin"
            shard_root = Path(tmpdir) / "shards"

            single_count = preprocess_directory(input_dir, single_output, num_workers=1)
            shards = split_replay_files(input_dir, shard_count=2)
            for shard_index, shard_files in enumerate(shards):
                shard_input_dir = shard_root / f"input-{shard_index:04d}"
                shard_input_dir.mkdir(parents=True)
                for shard_file in shard_files:
                    (shard_input_dir / shard_file.name).write_text(shard_file.read_text())
                preprocess_directory(shard_input_dir, shard_root / f"shard-{shard_index:04d}.bin", num_workers=1)

            merged_count = merge_preprocessed_shards(
                [shard_root / "shard-0000.bin", shard_root / "shard-0001.bin"],
                merged_output,
                expected_sample_count=single_count,
            )

            self.assertEqual(merged_count, single_count)
            self.assertEqual(merged_output.read_bytes(), single_output.read_bytes())
            self.assertEqual(group_ids_path(merged_output).read_bytes(), group_ids_path(single_output).read_bytes())
            self.assertEqual(policy_value_requests_path(merged_output).read_text(), policy_value_requests_path(single_output).read_text())
            self.assertEqual(policy_value_player_context_path(merged_output).read_text(), policy_value_player_context_path(single_output).read_text())
            self.assertEqual(load_dataset_metadata(merged_output), load_dataset_metadata(single_output))
            self.assertEqual(load_player_context_metadata(merged_output), load_player_context_metadata(single_output))


if __name__ == "__main__":
    unittest.main()
