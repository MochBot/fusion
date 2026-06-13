from __future__ import annotations

import json
import os
import sys
import tempfile
import unittest
from pathlib import Path
from typing import cast
from unittest.mock import patch

TRAINING_ROOT = Path(__file__).resolve().parents[1]
if str(TRAINING_ROOT) not in sys.path:
    sys.path.insert(0, str(TRAINING_ROOT))

from utils.policy_value_schema import deserialize_policy_value_target, serialize_policy_value_target
from scripts.generate_policy_value_labels import (
    PolicyValueOracleRequest,
    RELEASE_BINARY_ENV_VAR,
    build_policy_value_target,
    deserialize_policy_value_oracle_request,
    default_labels_output_path,
    default_requests_input_path,
    ensure_modal_compatible_release_binary,
    ensure_release_binary,
    generate_policy_value_labels_for_dataset,
    merge_label_shards,
    serialize_policy_value_oracle_request,
    split_requests_file,
)


class GeneratePolicyValueLabelTests(unittest.TestCase):
    def test_build_target_uses_best_root_score(self) -> None:
        target = build_policy_value_target(
            replay_id="r1",
            round_id=0,
            player_id=1,
            frame_id=99,
            group_id="r1:round:0",
            root_scores=[(101, 2.5), (205, 5.0), (333, 1.5)],
            best_value=5.0,
            position_complexity=0.8,
            temperature=1.0,
        )
        self.assertEqual(target.best_move_raw, 205)
        self.assertAlmostEqual(sum(target.policy_probs), 1.0, places=6)
        self.assertEqual(len(target.root_scores), len(target.policy_probs))

    def test_target_round_trip_serialization(self) -> None:
        target = build_policy_value_target(
            replay_id="r2",
            round_id=1,
            player_id=0,
            frame_id=5,
            group_id="r2:round:1",
            root_scores=[(7, 0.5), (9, 1.5)],
            best_value=1.5,
            position_complexity=0.2,
            temperature=0.75,
        )
        encoded = serialize_policy_value_target(target)
        decoded = deserialize_policy_value_target(encoded)
        self.assertEqual(decoded.best_move_raw, target.best_move_raw)
        self.assertEqual(decoded.root_scores, target.root_scores)
        self.assertEqual(decoded.policy_probs, target.policy_probs)

    def test_oracle_request_round_trip_serialization(self) -> None:
        request = PolicyValueOracleRequest(
            schema_version="phase1-v1",
            replay_id="r3",
            round_id=2,
            player_id=1,
            frame_id=33,
            group_id="r3:round:2",
            player_board_rows=[0, 0, 0],
            opponent_board_rows=[1, 2, 3],
            current_piece="t",
            hold_piece="i",
            queue=["o", "s", "z"],
            combo=2,
            b2b=1,
            lines=10,
            pending_garbage=4,
            bag_number=2,
        )
        encoded = serialize_policy_value_oracle_request(request)
        decoded = deserialize_policy_value_oracle_request(encoded)
        self.assertEqual(decoded.current_piece, "t")
        self.assertEqual(decoded.queue, ["o", "s", "z"])
        self.assertEqual(decoded.opponent_board_rows, [1, 2, 3])

    def test_default_paths_follow_sidecar_contract(self) -> None:
        data_path = Path("/tmp/training_data.bin")
        self.assertTrue(str(default_requests_input_path(data_path)).endswith(".policy_value.requests.jsonl"))
        self.assertTrue(str(default_labels_output_path(data_path)).endswith(".policy_value.jsonl"))

    def test_generate_policy_value_labels_uses_release_binary(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            data_path = Path(tmpdir) / "training_data.bin"
            requests_path = default_requests_input_path(data_path)
            _ = requests_path.write_text("{}\n")
            binary_path = self._write_fake_label_binary(Path(tmpdir))
            with patch("scripts.generate_policy_value_labels._ensure_release_binary", return_value=binary_path):
                labels_path = generate_policy_value_labels_for_dataset(data_path, num_workers=1)
            self.assertTrue(labels_path.exists())
            self.assertEqual(labels_path.read_text().count("\n"), 1)
            metadata_path = Path(f"{data_path}.policy_value.metadata.json")
            metadata = cast(dict[str, object], json.loads(metadata_path.read_text()))
            self.assertEqual(metadata["sample_count"], 1)

    def test_generate_policy_value_labels_merges_chunked_outputs(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            data_path = Path(tmpdir) / "training_data.bin"
            requests_path = default_requests_input_path(data_path)
            _ = requests_path.write_text("{}\n{}\n{}\n")
            binary_path = self._write_fake_label_binary(Path(tmpdir))
            with patch("scripts.generate_policy_value_labels._ensure_release_binary", return_value=binary_path):
                with patch("scripts.generate_policy_value_labels.CHUNK_MIN_REQUESTS", 2):
                    labels_path = generate_policy_value_labels_for_dataset(data_path, num_workers=2)
            lines = [line for line in labels_path.read_text().splitlines() if line.strip()]
            self.assertEqual(len(lines), 3)
            metadata_path = Path(f"{data_path}.policy_value.metadata.json")
            metadata = cast(dict[str, object], json.loads(metadata_path.read_text()))
            self.assertEqual(metadata["sample_count"], 3)

    def test_generate_policy_value_labels_rejects_empty_request_sidecar(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            data_path = Path(tmpdir) / "training_data.bin"
            requests_path = default_requests_input_path(data_path)
            _ = requests_path.write_text("")
            with self.assertRaisesRegex(ValueError, "request sidecar is empty"):
                _ = generate_policy_value_labels_for_dataset(data_path)

    def test_split_requests_file_preserves_order_across_shards(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            requests_path = Path(tmpdir) / "training_data.bin.policy_value.requests.jsonl"
            _ = requests_path.write_text("".join(f'{{"sample": {idx}}}\n' for idx in range(5)))

            shard_paths = split_requests_file(requests_path, Path(tmpdir) / "shards", shard_count=3)

            self.assertEqual(len(shard_paths), 3)
            self.assertEqual(
                [sum(1 for line in path.read_text().splitlines() if line.strip()) for path in shard_paths],
                [2, 2, 1],
            )
            merged_requests = "".join(path.read_text() for path in shard_paths)
            self.assertEqual(merged_requests, requests_path.read_text())

    def test_merge_label_shards_writes_final_metadata(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            shard_dir = Path(tmpdir) / "shards"
            shard_dir.mkdir()
            shard_a = shard_dir / "labels-0000.policy_value.jsonl"
            shard_b = shard_dir / "labels-0001.policy_value.jsonl"
            _ = shard_a.write_text('{"sample": 0}\n{"sample": 1}\n')
            _ = shard_b.write_text('{"sample": 2}\n')

            labels_path = Path(tmpdir) / "training_data.bin.policy_value.jsonl"
            merged_count = merge_label_shards([shard_a, shard_b], labels_path, expected_count=3)

            self.assertEqual(merged_count, 3)
            self.assertEqual(labels_path.read_text().count("\n"), 3)
            metadata_path = Path(tmpdir) / "training_data.bin.policy_value.metadata.json"
            metadata = cast(dict[str, object], json.loads(metadata_path.read_text()))
            self.assertEqual(metadata["sample_count"], 3)

    def test_ensure_release_binary_prefers_env_override(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            binary_path = self._write_fake_label_binary(Path(tmpdir))
            with patch.dict(os.environ, {RELEASE_BINARY_ENV_VAR: str(binary_path)}, clear=False):
                self.assertEqual(ensure_release_binary(), binary_path)

    def test_modal_release_binary_requires_linux_x86_host_or_override(self) -> None:
        with patch.dict(os.environ, {RELEASE_BINARY_ENV_VAR: ""}, clear=False), patch(
            "scripts.generate_policy_value_labels.platform.system", return_value="Darwin"
        ), patch("scripts.generate_policy_value_labels.platform.machine", return_value="arm64"):
            with self.assertRaisesRegex(RuntimeError, "Linux x86_64"):
                ensure_modal_compatible_release_binary()

    def _write_fake_label_binary(self, tmpdir: Path) -> Path:
        binary_path = tmpdir / "fake_generate_policy_value_labels.py"
        _ = binary_path.write_text(
            """#!/usr/bin/env python3
import json
import sys
from pathlib import Path

input_path = Path(sys.argv[1])
output_path = Path(sys.argv[2])
count = 0
with input_path.open() as src, output_path.open('w') as dst:
    for line in src:
        if not line.strip():
            continue
        dst.write(json.dumps({'sample': count}) + '\\n')
        count += 1

metadata_path = Path(str(output_path).removesuffix('.policy_value.jsonl') + '.policy_value.metadata.json')
metadata = {
    'schema_version': 'phase1-v1',
    'contract_version': 'policy-value-v2',
    'generation_mode': 'search_oracle',
    'policy_temperature': 1.0,
    'sample_count': count,
    'move_id_contract': 'Move.raw',
    'shared_input_contract': 'policy-value-shared-core-v2',
    'runtime_compatible_shared_inputs': True,
    'oracle_profile': 'stronger_offline_oracle',
    'oracle_beam_width': 2000,
    'oracle_depth': 18,
    'oracle_use_tt': True,
}
metadata_path.write_text(json.dumps(metadata) + '\\n')
print(f'generated_labels={count}')
print(f'output_path={output_path}')
print(f'metadata_path={metadata_path}')
"""
        )
        _ = os.chmod(binary_path, 0o755)
        return binary_path
