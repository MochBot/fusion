import importlib.util
import sys
import tempfile
import unittest
from pathlib import Path

import numpy as np


TRAINING_ROOT = Path(__file__).resolve().parents[1]
if str(TRAINING_ROOT) not in sys.path:
    sys.path.insert(0, str(TRAINING_ROOT))

PROBE_PATH = TRAINING_ROOT / "scripts" / "gpu_saturation_probe.py"
PROBE_SPEC = importlib.util.spec_from_file_location(
    "fusion_gpu_saturation_probe",
    PROBE_PATH,
)
if PROBE_SPEC is None or PROBE_SPEC.loader is None:
    raise RuntimeError(f"Could not load probe module from {PROBE_PATH}")
PROBE_MODULE = importlib.util.module_from_spec(PROBE_SPEC)
sys.modules[PROBE_SPEC.name] = PROBE_MODULE
PROBE_SPEC.loader.exec_module(PROBE_MODULE)

build_strided_indices = PROBE_MODULE.build_strided_indices
build_batch_index_groups = PROBE_MODULE.build_batch_index_groups
infer_num_samples = PROBE_MODULE.infer_num_samples
parse_int_csv = PROBE_MODULE.parse_int_csv
phase_class_from_features = PROBE_MODULE.phase_class_from_features
phase_classes_from_feature_batch = PROBE_MODULE.phase_classes_from_feature_batch
build_probe_tensors = PROBE_MODULE.build_probe_tensors
collate_probe_batch = PROBE_MODULE.collate_probe_batch
finalize_probe_batch = PROBE_MODULE.finalize_probe_batch
prepare_training_data = PROBE_MODULE.prepare_training_data
summarize_step_records = PROBE_MODULE.summarize_step_records


class GpuSaturationProbeTests(unittest.TestCase):
    def test_build_strided_indices_spreads_evenly(self) -> None:
        self.assertEqual(build_strided_indices(100, 10), [0, 10, 20, 30, 40, 50, 60, 70, 80, 90])

    def test_build_strided_indices_clamps_to_dataset(self) -> None:
        self.assertEqual(build_strided_indices(4, 10), [0, 1, 2, 3])

    def test_build_batch_index_groups_drops_partial_tail(self) -> None:
        self.assertEqual(
            build_batch_index_groups([0, 1, 2, 3, 4], 2),
            [[0, 1], [2, 3]],
        )

    def test_parse_int_csv_requires_values(self) -> None:
        with self.assertRaises(ValueError):
            parse_int_csv(" , ")

    def test_parse_int_csv_trims_whitespace(self) -> None:
        self.assertEqual(parse_int_csv(" 2, 4 ,8 "), [2, 4, 8])

    def test_infer_num_samples_checks_alignment(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            file_path = Path(tmpdir) / "samples.bin"
            file_path.write_bytes(b"\0" * 24)
            _ = self.assertEqual(infer_num_samples(file_path, 8), 3)

    def test_infer_num_samples_rejects_misaligned_file(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            file_path = Path(tmpdir) / "bad.bin"
            file_path.write_bytes(b"\0" * 10)
            with self.assertRaises(ValueError):
                _ = infer_num_samples(file_path, 8)

    def test_summarize_step_records_returns_aggregates(self) -> None:
        summary = summarize_step_records(
            [
                {
                    "step_wall_s": 0.8,
                    "step_cuda_s": 0.5,
                    "data_wait_s": 0.2,
                    "cuda_allocated_gib": 1.5,
                    "cuda_reserved_gib": 2.0,
                },
                {
                    "step_wall_s": 1.2,
                    "step_cuda_s": 0.7,
                    "data_wait_s": 0.3,
                    "cuda_allocated_gib": 1.8,
                    "cuda_reserved_gib": 2.3,
                },
            ]
        )

        self.assertEqual(summary["record_count"], 2)
        self.assertAlmostEqual(summary["mean_step_wall_s"], 1.0)
        self.assertAlmostEqual(summary["mean_step_cuda_s"], 0.6)
        self.assertAlmostEqual(summary["mean_data_wait_s"], 0.25)
        self.assertAlmostEqual(summary["max_cuda_allocated_gib"], 1.8)
        self.assertAlmostEqual(summary["max_cuda_reserved_gib"], 2.3)

    def test_phase_class_from_features_uses_tail_value(self) -> None:
        features = np.zeros(854, dtype=np.float32)
        features[-1] = -0.0000024

        phase_class = phase_class_from_features(features, 3)

        self.assertEqual(phase_class, 2)

    def test_collate_probe_batch_stacks_once_per_batch(self) -> None:
        sample_a = np.arange(854, dtype=np.float32)
        sample_b = np.arange(854, dtype=np.float32) + 1000.0

        features, regression_targets, phase_targets = collate_probe_batch(
            [(sample_a, 1), (sample_b, 2)],
            num_regression_heads=6,
            num_phase_classes=3,
        )

        self.assertEqual(tuple(features.shape), (2, 854))
        self.assertEqual(tuple(regression_targets.shape), (2, 6))
        self.assertEqual(tuple(phase_targets.shape), (2,))
        self.assertEqual(phase_targets.tolist(), [1, 2])
        self.assertEqual(features[0, 0].item(), 0.0)
        self.assertEqual(features[1, 0].item(), 1000.0)
        self.assertEqual(regression_targets[0].tolist(), [0.0, 1.0, 2.0, 3.0, 4.0, 5.0])

    def test_phase_classes_from_feature_batch_vectorizes_tail_value(self) -> None:
        feature_batch = np.zeros((2, 854), dtype=np.float32)
        feature_batch[0, -1] = -0.0000024
        feature_batch[1, -1] = 0.0000012

        phase_classes = phase_classes_from_feature_batch(feature_batch, 3)

        self.assertEqual(phase_classes.tolist(), [2, 1])

    def test_build_probe_tensors_materializes_batch_once(self) -> None:
        feature_batch = np.arange(2 * 854, dtype=np.float32).reshape(2, 854)

        features, regression_targets, phase_targets = build_probe_tensors(
            feature_batch,
            num_regression_heads=6,
            num_phase_classes=3,
        )

        self.assertEqual(tuple(features.shape), (2, 854))
        self.assertEqual(tuple(regression_targets.shape), (2, 6))
        self.assertEqual(tuple(phase_targets.shape), (2,))
        self.assertEqual(regression_targets[1, 0].item(), feature_batch[1, 0])

    def test_finalize_probe_batch_passthroughs_prebatched_tensors(self) -> None:
        features = PROBE_MODULE.torch.ones((2, 854), dtype=PROBE_MODULE.torch.float32)
        regression_targets = features[:, :6]
        phase_targets = PROBE_MODULE.torch.tensor([0, 1], dtype=PROBE_MODULE.torch.long)

        finalized = finalize_probe_batch(
            (features, regression_targets, phase_targets),
            num_regression_heads=6,
            num_phase_classes=3,
        )

        self.assertIs(finalized[0], features)
        self.assertIs(finalized[1], regression_targets)
        self.assertIs(finalized[2], phase_targets)

    def test_prepare_training_data_can_skip_tmp_staging(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            remote_path = Path(tmpdir) / "training_data.bin"
            local_path = Path(tmpdir) / "tmp.bin"
            remote_path.write_bytes(b"\0" * 64)

            result = prepare_training_data(
                remote_path=remote_path,
                local_path=local_path,
                stage_to_tmp=False,
            )

            self.assertEqual(result["remote_path"], str(remote_path))
            self.assertEqual(result["local_path"], str(remote_path))
            self.assertFalse(result["stage_to_tmp"])
            self.assertEqual(result["copy_time_s"], 0.0)
            self.assertFalse(local_path.exists())


if __name__ == "__main__":
    _ = unittest.main()
