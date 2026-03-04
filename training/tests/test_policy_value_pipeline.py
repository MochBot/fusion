import json
import sys
import tempfile
import types
import unittest
from pathlib import Path
from typing import cast
from unittest.mock import Mock, patch

import numpy as np

TRAINING_ROOT = Path(__file__).resolve().parents[1]
if str(TRAINING_ROOT) not in sys.path:
    sys.path.insert(0, str(TRAINING_ROOT))

from scripts.policy_value_pipeline import (
    POLICY_VALUE_BATCH_SIZE_BY_PROFILE,
    POLICY_VALUE_NUM_WORKERS_BY_PROFILE,
    artifact_readiness,
    missing_policy_value_artifact_paths,
    policy_value_batch_size_for_profile,
    policy_value_num_workers_for_profile,
    required_policy_value_artifact_paths,
    validate_policy_value_artifacts,
)
from scripts.gpu_profiles import get_profile
from utils.example_schema import group_ids_path, write_dataset_metadata
from utils.policy_value_schema import (
    GENERATION_MODE_SEARCH_ORACLE,
    write_player_context_metadata,
    write_policy_value_metadata,
)


class PolicyValuePipelineTests(unittest.TestCase):
    def _write_ready_policy_value_artifacts(self, base: Path, *, sample_count: int = 1) -> None:
        base.write_bytes(b"\x00" * (859 * 4 * sample_count))
        group_ids_path(base).write_bytes(np.asarray([1] * sample_count, dtype=np.uint64).tobytes())
        write_dataset_metadata(base, sample_count=sample_count)
        write_policy_value_metadata(
            base,
            sample_count=sample_count,
            generation_mode=GENERATION_MODE_SEARCH_ORACLE,
            policy_temperature=1.0,
        )

    def test_required_artifact_paths_cover_phase0_and_phase1_sidecars(self) -> None:
        paths = required_policy_value_artifact_paths("training/training_data.bin")
        self.assertEqual(
            [path.name for path in paths],
            [
                "training_data.bin",
                "training_data.bin.metadata.json",
                "training_data.bin.groups.u64",
                "training_data.bin.policy_value.requests.jsonl",
                "training_data.bin.policy_value.jsonl",
                "training_data.bin.policy_value.metadata.json",
            ],
        )

    def test_required_artifact_paths_include_player_context_sidecars_for_player_lane(self) -> None:
        paths = required_policy_value_artifact_paths(
            "training/training_data.bin",
            supervision_mode="player_context_primary",
        )
        self.assertEqual(
            [path.name for path in paths],
            [
                "training_data.bin",
                "training_data.bin.metadata.json",
                "training_data.bin.groups.u64",
                "training_data.bin.policy_value.requests.jsonl",
                "training_data.bin.policy_value.jsonl",
                "training_data.bin.policy_value.metadata.json",
                "training_data.bin.policy_value.player_context.jsonl",
                "training_data.bin.policy_value.player_context.metadata.json",
            ],
        )

    def test_missing_artifact_paths_reports_only_absent_files(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            base = Path(tmpdir) / "training_data.bin"
            required = required_policy_value_artifact_paths(base)
            for path in required[:3]:
                path.write_text("ok")
            missing = missing_policy_value_artifact_paths(base)
            self.assertEqual([path.name for path in missing], [path.name for path in required[3:]])

    def test_a10_defaults_are_conservative(self) -> None:
        self.assertEqual(policy_value_batch_size_for_profile("a10"), POLICY_VALUE_BATCH_SIZE_BY_PROFILE["a10"])
        self.assertEqual(policy_value_num_workers_for_profile("a10"), POLICY_VALUE_NUM_WORKERS_BY_PROFILE["a10"])
        self.assertEqual(policy_value_batch_size_for_profile("a10"), 1024)
        self.assertEqual(policy_value_num_workers_for_profile("a10"), 4)

    def test_h100_and_h200_selectors_exist_without_new_policy_defaults(self) -> None:
        self.assertEqual(get_profile("h100").resources.gpu, "H100!")
        self.assertEqual(get_profile("h200").resources.gpu, "H200")
        self.assertEqual(policy_value_batch_size_for_profile("h100"), 512)
        self.assertEqual(policy_value_num_workers_for_profile("h100"), 4)
        self.assertEqual(policy_value_batch_size_for_profile("h200"), 512)
        self.assertEqual(policy_value_num_workers_for_profile("h200"), 4)

    def test_validate_policy_value_artifacts_rejects_zero_sample_placeholders(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            base = Path(tmpdir) / "training_data.bin"
            base.write_bytes(b"")
            group_ids_path(base).write_bytes(b"")
            write_dataset_metadata(base, sample_count=0)
            base.with_name(f"{base.name}.policy_value.requests.jsonl").write_text("")
            base.with_name(f"{base.name}.policy_value.jsonl").write_text("")
            write_policy_value_metadata(
                base,
                sample_count=0,
                generation_mode=GENERATION_MODE_SEARCH_ORACLE,
                policy_temperature=1.0,
            )

            with self.assertRaisesRegex(ValueError, "sample_count"):
                validate_policy_value_artifacts(base)

    def test_artifact_readiness_reports_invalid_when_files_exist_but_samples_are_zero(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            base = Path(tmpdir) / "training_data.bin"
            base.write_bytes(b"")
            group_ids_path(base).write_bytes(b"")
            write_dataset_metadata(base, sample_count=0)
            base.with_name(f"{base.name}.policy_value.requests.jsonl").write_text("")
            base.with_name(f"{base.name}.policy_value.jsonl").write_text("")
            write_policy_value_metadata(
                base,
                sample_count=0,
                generation_mode=GENERATION_MODE_SEARCH_ORACLE,
                policy_temperature=1.0,
            )

            readiness = artifact_readiness(base)
            self.assertFalse(readiness.ready)
            self.assertFalse(readiness.dataset_ready)
            self.assertFalse(readiness.labels_ready)
            self.assertEqual(readiness.sample_count, 0)
            self.assertTrue(any("sample_count" in reason for reason in readiness.reasons))

    def test_artifact_readiness_reports_ready_for_valid_sidecars(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            base = Path(tmpdir) / "training_data.bin"
            self._write_ready_policy_value_artifacts(base)
            base.with_name(f"{base.name}.policy_value.requests.jsonl").write_text(
                json.dumps({"replay_id": "r1", "round_id": 0, "player_id": 0, "frame_id": 1}) + "\n"
            )
            base.with_name(f"{base.name}.policy_value.jsonl").write_text(
                json.dumps({"replay_id": "r1", "round_id": 0, "player_id": 0, "frame_id": 1}) + "\n"
            )

            readiness = artifact_readiness(base)
            self.assertTrue(readiness.ready)
            self.assertTrue(readiness.dataset_ready)
            self.assertTrue(readiness.labels_ready)
            self.assertEqual(readiness.sample_count, 1)
            self.assertEqual(readiness.missing_paths, [])

    def test_player_context_primary_readiness_requires_player_context_sidecars(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            base = Path(tmpdir) / "training_data.bin"
            self._write_ready_policy_value_artifacts(base)
            base.with_name(f"{base.name}.policy_value.requests.jsonl").write_text(
                json.dumps({"replay_id": "r1", "round_id": 0, "player_id": 0, "frame_id": 1}) + "\n"
            )
            base.with_name(f"{base.name}.policy_value.jsonl").write_text(
                json.dumps({"replay_id": "r1", "round_id": 0, "player_id": 0, "frame_id": 1}) + "\n"
            )

            readiness = artifact_readiness(base, supervision_mode="player_context_primary")
            self.assertFalse(readiness.ready)
            self.assertFalse(readiness.player_context_ready)
            self.assertTrue(any("player_context" in path.name for path in readiness.missing_paths))

            base.with_name(f"{base.name}.policy_value.player_context.jsonl").write_text(
                json.dumps({"replay_id": "r1", "round_id": 0, "player_id": 0, "frame_id": 1}) + "\n"
            )
            write_player_context_metadata(base, sample_count=1, recent_horizon=7, future_horizon=14)
            readiness = artifact_readiness(base, supervision_mode="player_context_primary")
            self.assertTrue(readiness.ready)
            self.assertTrue(readiness.player_context_ready)

    def test_artifact_readiness_rejects_label_identity_mismatch(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            base = Path(tmpdir) / "training_data.bin"
            self._write_ready_policy_value_artifacts(base)
            base.with_name(f"{base.name}.policy_value.requests.jsonl").write_text(
                "\n".join(
                    [
                        json.dumps({"replay_id": "r1", "round_id": 0, "player_id": 0, "frame_id": 1}),
                        json.dumps({"replay_id": "r1", "round_id": 0, "player_id": 1, "frame_id": 2}),
                    ]
                )
                + "\n"
            )
            base.with_name(f"{base.name}.policy_value.jsonl").write_text(
                "\n".join(
                    [
                        json.dumps({"replay_id": "foreign", "round_id": 0, "player_id": 0, "frame_id": 1}),
                        json.dumps({"replay_id": "foreign", "round_id": 0, "player_id": 1, "frame_id": 2}),
                    ]
                )
                + "\n"
            )
            write_dataset_metadata(base, sample_count=2)
            group_ids_path(base).write_bytes(np.asarray([1, 1], dtype=np.uint64).tobytes())
            base.write_bytes(b"\x00" * (859 * 4 * 2))
            write_policy_value_metadata(
                base,
                sample_count=2,
                generation_mode=GENERATION_MODE_SEARCH_ORACLE,
                policy_temperature=1.0,
            )

            readiness = artifact_readiness(base)
            self.assertFalse(readiness.ready)
            self.assertFalse(readiness.labels_ready)
            self.assertTrue(any("identity mismatch" in reason for reason in readiness.reasons))

    def test_artifact_readiness_rejects_player_context_identity_mismatch(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            base = Path(tmpdir) / "training_data.bin"
            self._write_ready_policy_value_artifacts(base, sample_count=2)
            base.with_name(f"{base.name}.policy_value.requests.jsonl").write_text(
                "\n".join(
                    [
                        json.dumps({"replay_id": "r1", "round_id": 0, "player_id": 0, "frame_id": 1}),
                        json.dumps({"replay_id": "r1", "round_id": 0, "player_id": 1, "frame_id": 2}),
                    ]
                )
                + "\n"
            )
            base.with_name(f"{base.name}.policy_value.jsonl").write_text(
                "\n".join(
                    [
                        json.dumps({"replay_id": "r1", "round_id": 0, "player_id": 0, "frame_id": 1}),
                        json.dumps({"replay_id": "r1", "round_id": 0, "player_id": 1, "frame_id": 2}),
                    ]
                )
                + "\n"
            )
            base.with_name(f"{base.name}.policy_value.player_context.jsonl").write_text(
                "\n".join(
                    [
                        json.dumps({"replay_id": "r1", "round_id": 0, "player_id": 1, "frame_id": 2}),
                        json.dumps({"replay_id": "r1", "round_id": 0, "player_id": 0, "frame_id": 1}),
                    ]
                )
                + "\n"
            )
            write_player_context_metadata(base, sample_count=2, recent_horizon=7, future_horizon=14)

            readiness = artifact_readiness(base, supervision_mode="player_context_primary")
            self.assertFalse(readiness.ready)
            self.assertFalse(readiness.player_context_ready)
            self.assertTrue(any("player-context identity mismatch" in reason for reason in readiness.reasons))

    def test_prepare_policy_value_artifacts_regenerates_invalid_existing_outputs(self) -> None:
        lightning_stub = types.ModuleType("lightning")
        lightning_pytorch_stub = types.ModuleType("lightning.pytorch")
        lightning_pytorch_utilities_stub = types.ModuleType("lightning.pytorch.utilities")
        lightning_pytorch_types_stub = types.ModuleType("lightning.pytorch.utilities.types")
        setattr(lightning_stub, "LightningModule", object)
        setattr(lightning_stub, "LightningDataModule", object)
        setattr(lightning_pytorch_types_stub, "OptimizerLRScheduler", object)
        sys.modules.setdefault("lightning", lightning_stub)
        sys.modules.setdefault("lightning.pytorch", lightning_pytorch_stub)
        sys.modules.setdefault("lightning.pytorch.utilities", lightning_pytorch_utilities_stub)
        sys.modules.setdefault("lightning.pytorch.utilities.types", lightning_pytorch_types_stub)

        from scripts.modal_app import prepare_policy_value_artifacts

        with tempfile.TemporaryDirectory() as tmpdir:
            base = Path(tmpdir) / "training_data.bin"
            replay_dir = Path(tmpdir) / "replays"
            replay_dir.mkdir()
            base.write_bytes(b"")
            group_ids_path(base).write_bytes(b"")
            write_dataset_metadata(base, sample_count=0)
            base.with_name(f"{base.name}.policy_value.requests.jsonl").write_text("")
            base.with_name(f"{base.name}.policy_value.jsonl").write_text("")
            write_policy_value_metadata(
                base,
                sample_count=0,
                generation_mode=GENERATION_MODE_SEARCH_ORACLE,
                policy_temperature=1.0,
            )

            with patch("scripts.modal_app.preprocess_directory") as preprocess_mock, patch(
                "scripts.modal_app.generate_policy_value_labels_for_dataset"
            ) as labels_mock:
                preprocess_mock.return_value = 1
                labels_mock.return_value = base.with_name(f"{base.name}.policy_value.jsonl")
                with self.assertRaisesRegex(RuntimeError, "invalid after preprocessing|produced invalid artifacts"):
                    prepare_policy_value_artifacts(
                        local_data_path=str(base),
                        replay_dir=str(replay_dir),
                        max_files=0,
                        num_workers=1,
                    )
                preprocess_mock.assert_called_once()
                labels_mock.assert_not_called()

    def test_launch_modal_player_context_artifact_pipeline_caps_worker_count(self) -> None:
        lightning_stub = types.ModuleType("lightning")
        lightning_pytorch_stub = types.ModuleType("lightning.pytorch")
        lightning_pytorch_utilities_stub = types.ModuleType("lightning.pytorch.utilities")
        lightning_pytorch_types_stub = types.ModuleType("lightning.pytorch.utilities.types")
        setattr(lightning_stub, "LightningModule", object)
        setattr(lightning_stub, "LightningDataModule", object)
        setattr(lightning_pytorch_types_stub, "OptimizerLRScheduler", object)
        sys.modules.setdefault("lightning", lightning_stub)
        sys.modules.setdefault("lightning.pytorch", lightning_pytorch_stub)
        sys.modules.setdefault("lightning.pytorch.utilities", lightning_pytorch_utilities_stub)
        sys.modules.setdefault("lightning.pytorch.utilities.types", lightning_pytorch_types_stub)

        from scripts import modal_app

        with tempfile.TemporaryDirectory() as tmpdir:
            replay_dir = Path(tmpdir) / "replays"
            replay_dir.mkdir()
            (replay_dir / "sample.ttrm").write_text('{"replay":{"rounds":[]}}')

            with patch("scripts.modal_app.launch_modal_player_context_artifact_pipeline_remote") as remote_mock, patch(
                "scripts.modal_app.split_replay_files",
                return_value=[
                    [replay_dir / "sample.ttrm"],
                    [replay_dir / "sample.ttrm"],
                    [replay_dir / "sample.ttrm"],
                ],
            ) as split_mock:
                modal_app.launch_modal_player_context_artifact_pipeline(
                    replay_dir=str(replay_dir),
                    local_output_path=str(Path(tmpdir) / "training_data.bin"),
                    shard_count=120,
                )

            split_mock.assert_called_once()
            self.assertEqual(split_mock.call_args.kwargs["shard_count"], 70)
            remote_mock.remote.assert_called_once()
            self.assertEqual(remote_mock.remote.call_args.kwargs["shard_count"], 3)

    def test_launch_modal_player_context_artifact_pipeline_rejects_non_player_mode(self) -> None:
        lightning_stub = types.ModuleType("lightning")
        lightning_pytorch_stub = types.ModuleType("lightning.pytorch")
        lightning_pytorch_utilities_stub = types.ModuleType("lightning.pytorch.utilities")
        lightning_pytorch_types_stub = types.ModuleType("lightning.pytorch.utilities.types")
        setattr(lightning_stub, "LightningModule", object)
        setattr(lightning_stub, "LightningDataModule", object)
        setattr(lightning_pytorch_types_stub, "OptimizerLRScheduler", object)
        sys.modules.setdefault("lightning", lightning_stub)
        sys.modules.setdefault("lightning.pytorch", lightning_pytorch_stub)
        sys.modules.setdefault("lightning.pytorch.utilities", lightning_pytorch_utilities_stub)
        sys.modules.setdefault("lightning.pytorch.utilities.types", lightning_pytorch_types_stub)

        from scripts import modal_app

        with self.assertRaisesRegex(ValueError, "player_context_primary"):
            modal_app.launch_modal_player_context_artifact_pipeline(
                supervision_mode="search_control",
            )

    def test_launch_modal_player_context_artifact_pipeline_remote_skips_empty_shards(self) -> None:
        lightning_stub = types.ModuleType("lightning")
        lightning_pytorch_stub = types.ModuleType("lightning.pytorch")
        lightning_pytorch_utilities_stub = types.ModuleType("lightning.pytorch.utilities")
        lightning_pytorch_types_stub = types.ModuleType("lightning.pytorch.utilities.types")
        setattr(lightning_stub, "LightningModule", object)
        setattr(lightning_stub, "LightningDataModule", object)
        setattr(lightning_pytorch_types_stub, "OptimizerLRScheduler", object)
        sys.modules.setdefault("lightning", lightning_stub)
        sys.modules.setdefault("lightning.pytorch", lightning_pytorch_stub)
        sys.modules.setdefault("lightning.pytorch.utilities", lightning_pytorch_utilities_stub)
        sys.modules.setdefault("lightning.pytorch.utilities.types", lightning_pytorch_types_stub)

        from scripts import modal_app

        shard_calls = [Mock(), Mock()]
        shard_calls[0].get.return_value = {"shard_output_relpath": "shards/empty.bin", "sample_count": 0}
        shard_calls[1].get.return_value = {"shard_output_relpath": "shards/full.bin", "sample_count": 5}

        with patch("scripts.modal_app.preprocess_player_context_shard_remote.spawn", side_effect=shard_calls), patch(
            "scripts.modal_app.merge_preprocessed_shards", return_value=5
        ) as merge_mock, patch("scripts.modal_app._validate_player_context_dataset_artifacts", return_value={"sample_count": 5, "request_count": 5, "context_count": 5}), patch(
            "scripts.modal_app.shutil.copy2"
        ), patch("scripts.modal_app.data_vol.commit"), patch("scripts.modal_app.data_vol.reload") as reload_mock:
            result = modal_app.launch_modal_player_context_artifact_pipeline_remote.local(
                data_filename="training_data.bin",
                run_id="run-1",
                shard_replay_relpaths=[["a.ttrm"], ["b.ttrm"]],
                shard_count=2,
                shard_num_workers=1,
            )

        reload_mock.assert_called_once()
        merge_mock.assert_called_once()
        merged_shards = merge_mock.call_args.args[0]
        self.assertEqual([str(path) for path in merged_shards], [f"{modal_app.DATA_DIR}/shards/full.bin"])
        self.assertEqual(result["merged_count"], 5)

    def test_launch_policy_value_sweep_uploads_once_and_spawns_multiple_runs(self) -> None:
        lightning_stub = types.ModuleType("lightning")
        lightning_pytorch_stub = types.ModuleType("lightning.pytorch")
        lightning_pytorch_utilities_stub = types.ModuleType("lightning.pytorch.utilities")
        lightning_pytorch_types_stub = types.ModuleType("lightning.pytorch.utilities.types")
        setattr(lightning_stub, "LightningModule", object)
        setattr(lightning_stub, "LightningDataModule", object)
        setattr(lightning_pytorch_types_stub, "OptimizerLRScheduler", object)
        sys.modules.setdefault("lightning", lightning_stub)
        sys.modules.setdefault("lightning.pytorch", lightning_pytorch_stub)
        sys.modules.setdefault("lightning.pytorch.utilities", lightning_pytorch_utilities_stub)
        sys.modules.setdefault("lightning.pytorch.utilities.types", lightning_pytorch_types_stub)

        from scripts import modal_app

        with tempfile.TemporaryDirectory() as tmpdir:
            base = Path(tmpdir) / "training_data.bin"
            base.write_bytes(b"\x00" * (859 * 4))
            group_ids_path(base).write_bytes((1).to_bytes(8, "little"))
            write_dataset_metadata(base, sample_count=1)
            base.with_name(f"{base.name}.policy_value.requests.jsonl").write_text('{"request":1}\n')
            base.with_name(f"{base.name}.policy_value.jsonl").write_text('{"label":1}\n')
            write_policy_value_metadata(
                base,
                sample_count=1,
                generation_mode=GENERATION_MODE_SEARCH_ORACLE,
                policy_temperature=1.0,
            )
            base.with_name(f"{base.name}.policy_value.player_context.jsonl").write_text('{"context":1}\n')
            write_player_context_metadata(base, sample_count=1, recent_horizon=7, future_horizon=14)

            with patch("scripts.modal_app.run_policy_value_sweep_remote") as remote_mock, patch(
                "scripts.modal_app.required_policy_value_artifact_paths",
                return_value=[
                    base,
                    base.with_name(f"{base.name}.metadata.json"),
                    group_ids_path(base),
                    base.with_name(f"{base.name}.policy_value.requests.jsonl"),
                    base.with_name(f"{base.name}.policy_value.jsonl"),
                    base.with_name(f"{base.name}.policy_value.metadata.json"),
                    base.with_name(f"{base.name}.policy_value.player_context.jsonl"),
                    base.with_name(f"{base.name}.policy_value.player_context.metadata.json"),
                ],
            ), patch("scripts.modal_app.validate_policy_value_artifacts"), patch("scripts.modal_app.data_vol.batch_upload") as upload_mock:
                upload_mock.return_value.__enter__.return_value = Mock()
                modal_app.launch_policy_value_sweep(
                    local_data_path=str(base),
                    supervision_mode="player_context_primary",
                    prepare_artifacts=False,
                    learning_rates="3e-4,1e-4",
                    weight_decays="1e-5",
                    batch_sizes="256",
                    max_epochs=5,
                )

            remote_mock.remote.assert_called_once()
            run_specs = remote_mock.remote.call_args.kwargs["run_specs"]
            self.assertEqual(len(run_specs), 2)
            self.assertEqual({spec["lr"] for spec in run_specs}, {3e-4, 1e-4})
            self.assertEqual({spec["weight_decay"] for spec in run_specs}, {1e-5})
            self.assertEqual({spec["batch_size"] for spec in run_specs}, {256})
            self.assertTrue(all(spec["run_id"].startswith("policy-value-") for spec in run_specs))

    def test_run_policy_value_sweep_remote_spawns_each_run_spec(self) -> None:
        lightning_stub = types.ModuleType("lightning")
        lightning_pytorch_stub = types.ModuleType("lightning.pytorch")
        lightning_pytorch_utilities_stub = types.ModuleType("lightning.pytorch.utilities")
        lightning_pytorch_types_stub = types.ModuleType("lightning.pytorch.utilities.types")
        setattr(lightning_stub, "LightningModule", object)
        setattr(lightning_stub, "LightningDataModule", object)
        setattr(lightning_pytorch_types_stub, "OptimizerLRScheduler", object)
        sys.modules.setdefault("lightning", lightning_stub)
        sys.modules.setdefault("lightning.pytorch", lightning_pytorch_stub)
        sys.modules.setdefault("lightning.pytorch.utilities", lightning_pytorch_utilities_stub)
        sys.modules.setdefault("lightning.pytorch.utilities.types", lightning_pytorch_types_stub)

        from scripts import modal_app

        first_call = Mock(object_id="call-1")
        second_call = Mock(object_id="call-2")
        with patch("scripts.modal_app.train_policy_value_remote.spawn", side_effect=[first_call, second_call]) as spawn_mock:
            result = modal_app.run_policy_value_sweep_remote.local(
                data_filename="training_data.bin",
                supervision_mode="player_context_primary",
                run_specs=[
                    {"run_id": "run-a", "batch_size": 256, "num_workers": 4, "max_epochs": 5, "lr": 3e-4, "weight_decay": 1e-5},
                    {"run_id": "run-b", "batch_size": 512, "num_workers": 4, "max_epochs": 5, "lr": 1e-4, "weight_decay": 1e-5},
                ],
            )

        self.assertEqual(spawn_mock.call_count, 2)
        self.assertEqual(result["run_count"], 2)
        runs = cast(list[dict[str, object]], result["runs"])
        self.assertEqual([entry["call_id"] for entry in runs], ["call-1", "call-2"])

    def test_launch_policy_value_sweep_smoke_uses_tiny_defaults(self) -> None:
        lightning_stub = types.ModuleType("lightning")
        lightning_pytorch_stub = types.ModuleType("lightning.pytorch")
        lightning_pytorch_utilities_stub = types.ModuleType("lightning.pytorch.utilities")
        lightning_pytorch_types_stub = types.ModuleType("lightning.pytorch.utilities.types")
        setattr(lightning_stub, "LightningModule", object)
        setattr(lightning_stub, "LightningDataModule", object)
        setattr(lightning_pytorch_types_stub, "OptimizerLRScheduler", object)
        sys.modules.setdefault("lightning", lightning_stub)
        sys.modules.setdefault("lightning.pytorch", lightning_pytorch_stub)
        sys.modules.setdefault("lightning.pytorch.utilities", lightning_pytorch_utilities_stub)
        sys.modules.setdefault("lightning.pytorch.utilities.types", lightning_pytorch_types_stub)

        from scripts import modal_app

        smoke_path = Path("/tmp/test-smoke.bin")
        with patch("scripts.modal_app._write_policy_value_sweep_smoke_artifacts", return_value=smoke_path) as write_mock, patch(
            "scripts.modal_app.launch_policy_value_sweep"
        ) as sweep_mock:
            modal_app.launch_policy_value_sweep_smoke(local_data_path=str(smoke_path))

        write_mock.assert_called_once_with(str(smoke_path))
        sweep_mock.assert_called_once_with(
            local_data_path=str(smoke_path),
            prepare_artifacts=False,
            supervision_mode="player_context_primary",
            learning_rates="3e-4,1e-4",
            weight_decays="1e-5",
            batch_sizes="1",
            num_worker_values="0",
            max_epochs=1,
        )

    def test_upload_policy_value_dataset_artifacts_tolerates_existing_volume_files(self) -> None:
        lightning_stub = types.ModuleType("lightning")
        lightning_pytorch_stub = types.ModuleType("lightning.pytorch")
        lightning_pytorch_utilities_stub = types.ModuleType("lightning.pytorch.utilities")
        lightning_pytorch_types_stub = types.ModuleType("lightning.pytorch.utilities.types")
        setattr(lightning_stub, "LightningModule", object)
        setattr(lightning_stub, "LightningDataModule", object)
        setattr(lightning_pytorch_types_stub, "OptimizerLRScheduler", object)
        sys.modules.setdefault("lightning", lightning_stub)
        sys.modules.setdefault("lightning.pytorch", lightning_pytorch_stub)
        sys.modules.setdefault("lightning.pytorch.utilities", lightning_pytorch_utilities_stub)
        sys.modules.setdefault("lightning.pytorch.utilities.types", lightning_pytorch_types_stub)

        from scripts import modal_app

        with tempfile.TemporaryDirectory() as tmpdir:
            base = Path(tmpdir) / "training_data.bin"
            base.write_bytes(b"\x00" * (859 * 4))
            group_ids_path(base).write_bytes((1).to_bytes(8, "little"))
            write_dataset_metadata(base, sample_count=1)
            base.with_name(f"{base.name}.policy_value.requests.jsonl").write_text('{"request":1}\n')
            base.with_name(f"{base.name}.policy_value.jsonl").write_text('{"label":1}\n')
            write_policy_value_metadata(
                base,
                sample_count=1,
                generation_mode=GENERATION_MODE_SEARCH_ORACLE,
                policy_temperature=1.0,
            )

            batch_mock = Mock()
            batch_mock.put_file.side_effect = [None, None, None, FileExistsError("already exists")]
            manager = Mock()
            manager.__enter__ = Mock(return_value=batch_mock)
            manager.__exit__ = Mock(side_effect=FileExistsError("already exists"))

            with patch("scripts.modal_app.data_vol.batch_upload", return_value=manager):
                uploaded = modal_app._upload_policy_value_dataset_artifacts(base)

            self.assertEqual(
                uploaded,
                [
                    "training_data.bin",
                    "training_data.bin.metadata.json",
                    "training_data.bin.groups.u64",
                    "training_data.bin.policy_value.requests.jsonl",
                ],
            )

    def test_upload_modal_label_binary_tolerates_existing_volume_file(self) -> None:
        lightning_stub = types.ModuleType("lightning")
        lightning_pytorch_stub = types.ModuleType("lightning.pytorch")
        lightning_pytorch_utilities_stub = types.ModuleType("lightning.pytorch.utilities")
        lightning_pytorch_types_stub = types.ModuleType("lightning.pytorch.utilities.types")
        setattr(lightning_stub, "LightningModule", object)
        setattr(lightning_stub, "LightningDataModule", object)
        setattr(lightning_pytorch_types_stub, "OptimizerLRScheduler", object)
        sys.modules.setdefault("lightning", lightning_stub)
        sys.modules.setdefault("lightning.pytorch", lightning_pytorch_stub)
        sys.modules.setdefault("lightning.pytorch.utilities", lightning_pytorch_utilities_stub)
        sys.modules.setdefault("lightning.pytorch.utilities.types", lightning_pytorch_types_stub)

        from scripts import modal_app

        with tempfile.TemporaryDirectory() as tmpdir:
            binary = Path(tmpdir) / "generate_policy_value_labels"
            binary.write_bytes(b"binary")
            batch_mock = Mock()
            manager = Mock()
            manager.__enter__ = Mock(return_value=batch_mock)
            manager.__exit__ = Mock(side_effect=FileExistsError("already exists"))

            with patch("scripts.modal_app.data_vol.batch_upload", return_value=manager):
                uploaded = modal_app._upload_modal_label_binary(binary)

            batch_mock.put_file.assert_called_once_with(
                str(binary),
                str(modal_app.MODAL_LABEL_BINARY_RELATIVE_PATH),
            )
            self.assertEqual(uploaded, str(modal_app.MODAL_LABEL_BINARY_RELATIVE_PATH))

    def test_launch_modal_policy_value_label_pipeline_tolerates_existing_request_shards(self) -> None:
        lightning_stub = types.ModuleType("lightning")
        lightning_pytorch_stub = types.ModuleType("lightning.pytorch")
        lightning_pytorch_utilities_stub = types.ModuleType("lightning.pytorch.utilities")
        lightning_pytorch_types_stub = types.ModuleType("lightning.pytorch.utilities.types")
        setattr(lightning_stub, "LightningModule", object)
        setattr(lightning_stub, "LightningDataModule", object)
        setattr(lightning_pytorch_types_stub, "OptimizerLRScheduler", object)
        sys.modules.setdefault("lightning", lightning_stub)
        sys.modules.setdefault("lightning.pytorch", lightning_pytorch_stub)
        sys.modules.setdefault("lightning.pytorch.utilities", lightning_pytorch_utilities_stub)
        sys.modules.setdefault("lightning.pytorch.utilities.types", lightning_pytorch_types_stub)

        from scripts import modal_app

        with tempfile.TemporaryDirectory() as tmpdir:
            base = Path(tmpdir) / "training_data.bin"
            base.write_bytes(b"\x00" * (859 * 4))
            group_ids_path(base).write_bytes((1).to_bytes(8, "little"))
            write_dataset_metadata(base, sample_count=1)
            base.with_name(f"{base.name}.policy_value.requests.jsonl").write_text('{"request":1}\n')
            base.with_name(f"{base.name}.policy_value.jsonl").write_text('{"label":1}\n')
            write_policy_value_metadata(
                base,
                sample_count=1,
                generation_mode=GENERATION_MODE_SEARCH_ORACLE,
                policy_temperature=1.0,
            )

            manager = Mock()
            manager.__enter__ = Mock(return_value=Mock())
            manager.__exit__ = Mock(side_effect=FileExistsError("already exists"))

            with patch("scripts.modal_app.split_requests_file", return_value=[Path("/tmp/requests-0000.jsonl")]), patch(
                "scripts.modal_app._upload_policy_value_dataset_artifacts",
                return_value=["training_data.bin", "training_data.bin.metadata.json", "training_data.bin.groups.u64", "training_data.bin.policy_value.requests.jsonl"],
            ), patch(
                "scripts.modal_app._upload_modal_label_binary",
                return_value=str(modal_app.MODAL_LABEL_BINARY_RELATIVE_PATH),
            ), patch("scripts.modal_app.data_vol.batch_upload", return_value=manager), patch(
                "scripts.modal_app.run_modal_policy_value_label_pipeline"
            ) as remote_mock:
                remote_mock.remote.return_value = {"merged_count": 1}
                modal_app.launch_modal_policy_value_label_pipeline(
                    local_data_path=str(base),
                    shard_count=1,
                )

            remote_mock.remote.assert_called_once_with(
                data_filename=base.name,
                shard_count=1,
            )

    def test_launch_modal_policy_value_label_pipeline_uploads_shards_before_tempdir_cleanup(self) -> None:
        lightning_stub = types.ModuleType("lightning")
        lightning_pytorch_stub = types.ModuleType("lightning.pytorch")
        lightning_pytorch_utilities_stub = types.ModuleType("lightning.pytorch.utilities")
        lightning_pytorch_types_stub = types.ModuleType("lightning.pytorch.utilities.types")
        setattr(lightning_stub, "LightningModule", object)
        setattr(lightning_stub, "LightningDataModule", object)
        setattr(lightning_pytorch_types_stub, "OptimizerLRScheduler", object)
        sys.modules.setdefault("lightning", lightning_stub)
        sys.modules.setdefault("lightning.pytorch", lightning_pytorch_stub)
        sys.modules.setdefault("lightning.pytorch.utilities", lightning_pytorch_utilities_stub)
        sys.modules.setdefault("lightning.pytorch.utilities.types", lightning_pytorch_types_stub)

        from scripts import modal_app

        with tempfile.TemporaryDirectory() as tmpdir:
            base = Path(tmpdir) / "training_data.bin"
            base.write_bytes(b"\x00" * (859 * 4))
            group_ids_path(base).write_bytes((1).to_bytes(8, "little"))
            write_dataset_metadata(base, sample_count=1)
            base.with_name(f"{base.name}.policy_value.requests.jsonl").write_text('{"request":1}\n')
            base.with_name(f"{base.name}.policy_value.jsonl").write_text('{"label":1}\n')
            write_policy_value_metadata(
                base,
                sample_count=1,
                generation_mode=GENERATION_MODE_SEARCH_ORACLE,
                policy_temperature=1.0,
            )

            shard_file = Path(tmpdir) / "requests-0000.policy_value.requests.jsonl"
            shard_file.write_text('{"request":1}\n')

            batch_mock = Mock()
            manager = Mock()
            manager.__enter__ = Mock(return_value=batch_mock)
            manager.__exit__ = Mock(return_value=False)

            with patch("scripts.modal_app.split_requests_file", return_value=[shard_file]), patch(
                "scripts.modal_app._upload_policy_value_dataset_artifacts",
                return_value=["training_data.bin", "training_data.bin.metadata.json", "training_data.bin.groups.u64", "training_data.bin.policy_value.requests.jsonl"],
            ), patch(
                "scripts.modal_app._upload_modal_label_binary",
                return_value=str(modal_app.MODAL_LABEL_BINARY_RELATIVE_PATH),
            ), patch("scripts.modal_app.data_vol.batch_upload", return_value=manager), patch(
                "scripts.modal_app.run_modal_policy_value_label_pipeline"
            ) as remote_mock:
                remote_mock.remote.return_value = {"merged_count": 1}
                modal_app.launch_modal_policy_value_label_pipeline(
                    local_data_path=str(base),
                    shard_count=1,
                )

            batch_mock.put_file.assert_called_once_with(
                str(shard_file),
                str(modal_app._policy_value_shard_requests_relpath(base.name, 0)),
            )

    def test_run_modal_policy_value_label_pipeline_reloads_volume_before_merge(self) -> None:
        lightning_stub = types.ModuleType("lightning")
        lightning_pytorch_stub = types.ModuleType("lightning.pytorch")
        lightning_pytorch_utilities_stub = types.ModuleType("lightning.pytorch.utilities")
        lightning_pytorch_types_stub = types.ModuleType("lightning.pytorch.utilities.types")
        setattr(lightning_stub, "LightningModule", object)
        setattr(lightning_stub, "LightningDataModule", object)
        setattr(lightning_pytorch_types_stub, "OptimizerLRScheduler", object)
        sys.modules.setdefault("lightning", lightning_stub)
        sys.modules.setdefault("lightning.pytorch", lightning_pytorch_stub)
        sys.modules.setdefault("lightning.pytorch.utilities", lightning_pytorch_utilities_stub)
        sys.modules.setdefault("lightning.pytorch.utilities.types", lightning_pytorch_types_stub)

        from scripts import modal_app

        fake_calls = [Mock(), Mock()]
        fake_calls[0].get.return_value = {"labels_relpath": "policy_value_shards/training_data.bin/labels-0000.policy_value.jsonl", "sample_count": 1}
        fake_calls[1].get.return_value = {"labels_relpath": "policy_value_shards/training_data.bin/labels-0001.policy_value.jsonl", "sample_count": 1}

        with patch("scripts.modal_app.generate_policy_value_label_shard_remote.spawn", side_effect=fake_calls), patch(
            "scripts.generate_policy_value_labels.merge_label_shards", return_value=2
        ) as merge_mock, patch("scripts.modal_app.artifact_readiness") as readiness_mock, patch(
            "scripts.modal_app.required_policy_value_artifact_paths",
            return_value=[Path("/data/training_data.bin"), Path("/data/training_data.bin.metadata.json"), Path("/data/training_data.bin.groups.u64"), Path("/data/training_data.bin.policy_value.requests.jsonl"), Path("/data/training_data.bin.policy_value.jsonl"), Path("/data/training_data.bin.policy_value.metadata.json")],
        ), patch("scripts.modal_app.data_vol.reload") as reload_mock, patch("scripts.modal_app.data_vol.commit"):
            readiness_mock.side_effect = [
                Mock(dataset_ready=True, sample_count=2, reasons=[]),
                Mock(ready=True, reasons=[]),
            ]
            result = modal_app.run_modal_policy_value_label_pipeline.local(
                data_filename="training_data.bin",
                shard_count=2,
            )

        reload_mock.assert_called_once()
        merge_mock.assert_called_once()
        self.assertEqual(result["merged_count"], 2)

    def test_resume_modal_policy_value_label_merge_remote_reloads_volume_before_merge(self) -> None:
        lightning_stub = types.ModuleType("lightning")
        lightning_pytorch_stub = types.ModuleType("lightning.pytorch")
        lightning_pytorch_utilities_stub = types.ModuleType("lightning.pytorch.utilities")
        lightning_pytorch_types_stub = types.ModuleType("lightning.pytorch.utilities.types")
        setattr(lightning_stub, "LightningModule", object)
        setattr(lightning_stub, "LightningDataModule", object)
        setattr(lightning_pytorch_types_stub, "OptimizerLRScheduler", object)
        sys.modules.setdefault("lightning", lightning_stub)
        sys.modules.setdefault("lightning.pytorch", lightning_pytorch_stub)
        sys.modules.setdefault("lightning.pytorch.utilities", lightning_pytorch_utilities_stub)
        sys.modules.setdefault("lightning.pytorch.utilities.types", lightning_pytorch_types_stub)

        from scripts import modal_app

        with patch("scripts.generate_policy_value_labels.merge_label_shards", return_value=2) as merge_mock, patch(
            "scripts.modal_app.artifact_readiness"
        ) as readiness_mock, patch("scripts.modal_app.required_policy_value_artifact_paths", return_value=[Path("/data/training_data.bin"), Path("/data/training_data.bin.metadata.json"), Path("/data/training_data.bin.groups.u64"), Path("/data/training_data.bin.policy_value.requests.jsonl"), Path("/data/training_data.bin.policy_value.jsonl"), Path("/data/training_data.bin.policy_value.metadata.json")]), patch(
            "scripts.modal_app.data_vol.reload"
        ) as reload_mock, patch("scripts.modal_app.data_vol.commit"):
            readiness_mock.side_effect = [
                Mock(dataset_ready=True, sample_count=2, reasons=[]),
                Mock(ready=True, reasons=[]),
            ]
            result = modal_app.resume_modal_policy_value_label_merge_remote.local(
                data_filename="training_data.bin",
                shard_count=2,
            )

        reload_mock.assert_called_once()
        merge_mock.assert_called_once()
        self.assertEqual(result["merged_count"], 2)

    def test_launch_modal_policy_value_label_merge_resume_calls_remote(self) -> None:
        lightning_stub = types.ModuleType("lightning")
        lightning_pytorch_stub = types.ModuleType("lightning.pytorch")
        lightning_pytorch_utilities_stub = types.ModuleType("lightning.pytorch.utilities")
        lightning_pytorch_types_stub = types.ModuleType("lightning.pytorch.utilities.types")
        setattr(lightning_stub, "LightningModule", object)
        setattr(lightning_stub, "LightningDataModule", object)
        setattr(lightning_pytorch_types_stub, "OptimizerLRScheduler", object)
        sys.modules.setdefault("lightning", lightning_stub)
        sys.modules.setdefault("lightning.pytorch", lightning_pytorch_stub)
        sys.modules.setdefault("lightning.pytorch.utilities", lightning_pytorch_utilities_stub)
        sys.modules.setdefault("lightning.pytorch.utilities.types", lightning_pytorch_types_stub)

        from scripts import modal_app

        with patch("scripts.modal_app.resume_modal_policy_value_label_merge_remote") as remote_mock:
            remote_mock.remote.return_value = {"merged_count": 533314}
            modal_app.launch_modal_policy_value_label_merge_resume(
                local_data_path="training/training_data.bin",
                shard_count=70,
            )

        remote_mock.remote.assert_called_once_with(
            data_filename="training_data.bin",
            shard_count=70,
        )

    def test_repair_modal_policy_value_label_shards_remote_reruns_selected_shards(self) -> None:
        lightning_stub = types.ModuleType("lightning")
        lightning_pytorch_stub = types.ModuleType("lightning.pytorch")
        lightning_pytorch_utilities_stub = types.ModuleType("lightning.pytorch.utilities")
        lightning_pytorch_types_stub = types.ModuleType("lightning.pytorch.utilities.types")
        setattr(lightning_stub, "LightningModule", object)
        setattr(lightning_stub, "LightningDataModule", object)
        setattr(lightning_pytorch_types_stub, "OptimizerLRScheduler", object)
        sys.modules.setdefault("lightning", lightning_stub)
        sys.modules.setdefault("lightning.pytorch", lightning_pytorch_stub)
        sys.modules.setdefault("lightning.pytorch.utilities", lightning_pytorch_utilities_stub)
        sys.modules.setdefault("lightning.pytorch.utilities.types", lightning_pytorch_types_stub)

        from scripts import modal_app

        fake_calls = [Mock(), Mock()]
        fake_calls[0].get.return_value = {"labels_relpath": "policy_value_shards/training_data.bin/labels-0051.policy_value.jsonl", "sample_count": 7619}
        fake_calls[1].get.return_value = {"labels_relpath": "policy_value_shards/training_data.bin/labels-0069.policy_value.jsonl", "sample_count": 7618}

        with patch("scripts.modal_app.generate_policy_value_label_shard_remote.spawn", side_effect=fake_calls) as spawn_mock, patch(
            "scripts.modal_app._resume_modal_policy_value_label_merge",
            return_value=533314,
        ) as resume_mock, patch("scripts.modal_app.artifact_readiness") as readiness_mock:
            readiness_mock.return_value = Mock(dataset_ready=True, sample_count=533314, reasons=[])
            result = modal_app.repair_modal_policy_value_label_shards_remote.local(
                data_filename="training_data.bin",
                shard_indices=[69, 51, 69],
                shard_count=70,
            )

        self.assertEqual(spawn_mock.call_count, 2)
        self.assertEqual(
            [call.kwargs["requests_relpath"] for call in spawn_mock.call_args_list],
            [
                str(modal_app._policy_value_shard_requests_relpath("training_data.bin", 51)),
                str(modal_app._policy_value_shard_requests_relpath("training_data.bin", 69)),
            ],
        )
        resume_mock.assert_called_once_with(Path("/data/training_data.bin"), 70, 533314)
        self.assertEqual(result["repaired_shards"], [51, 69])
        self.assertEqual(result["merged_count"], 533314)

    def test_launch_modal_policy_value_label_shard_repair_calls_remote(self) -> None:
        lightning_stub = types.ModuleType("lightning")
        lightning_pytorch_stub = types.ModuleType("lightning.pytorch")
        lightning_pytorch_utilities_stub = types.ModuleType("lightning.pytorch.utilities")
        lightning_pytorch_types_stub = types.ModuleType("lightning.pytorch.utilities.types")
        setattr(lightning_stub, "LightningModule", object)
        setattr(lightning_stub, "LightningDataModule", object)
        setattr(lightning_pytorch_types_stub, "OptimizerLRScheduler", object)
        sys.modules.setdefault("lightning", lightning_stub)
        sys.modules.setdefault("lightning.pytorch", lightning_pytorch_stub)
        sys.modules.setdefault("lightning.pytorch.utilities", lightning_pytorch_utilities_stub)
        sys.modules.setdefault("lightning.pytorch.utilities.types", lightning_pytorch_types_stub)

        from scripts import modal_app

        with patch("scripts.modal_app.repair_modal_policy_value_label_shards_remote") as remote_mock:
            remote_mock.remote.return_value = {"merged_count": 533314, "repaired_shards": [51, 69]}
            modal_app.launch_modal_policy_value_label_shard_repair(
                local_data_path="training/training_data.bin",
                shard_indices="69,51,69",
                shard_count=70,
            )

        remote_mock.remote.assert_called_once_with(
            data_filename="training_data.bin",
            shard_indices=[51, 69],
            shard_count=70,
        )


if __name__ == "__main__":
    unittest.main()
