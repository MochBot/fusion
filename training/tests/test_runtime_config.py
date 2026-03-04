import importlib.util
import os
import sys
import unittest
try:
    from typing import override
except ImportError:
    from typing_extensions import override
from pathlib import Path


TRAINING_ROOT = Path(__file__).resolve().parents[1]
if str(TRAINING_ROOT) not in sys.path:
    sys.path.insert(0, str(TRAINING_ROOT))

RUNTIME_CONFIG_PATH = TRAINING_ROOT / "utils" / "runtime_config.py"
RUNTIME_CONFIG_SPEC = importlib.util.spec_from_file_location(
    "fusion_training_runtime_config",
    RUNTIME_CONFIG_PATH,
)
if RUNTIME_CONFIG_SPEC is None or RUNTIME_CONFIG_SPEC.loader is None:
    raise RuntimeError(f"Could not load runtime config module from {RUNTIME_CONFIG_PATH}")
RUNTIME_CONFIG_MODULE = importlib.util.module_from_spec(RUNTIME_CONFIG_SPEC)
sys.modules[RUNTIME_CONFIG_SPEC.name] = RUNTIME_CONFIG_MODULE
RUNTIME_CONFIG_SPEC.loader.exec_module(RUNTIME_CONFIG_MODULE)

load_probe_runtime_config = RUNTIME_CONFIG_MODULE.load_probe_runtime_config
load_training_runtime_config = RUNTIME_CONFIG_MODULE.load_training_runtime_config


class RuntimeConfigTests(unittest.TestCase):
    original_env: dict[str, str] = {}

    @override
    def setUp(self) -> None:
        self.original_env = dict(os.environ)

    @override
    def tearDown(self) -> None:
        os.environ.clear()
        os.environ.update(self.original_env)

    def test_probe_runtime_defaults(self) -> None:
        os.environ.clear()

        config = load_probe_runtime_config()

        self.assertEqual(config.app_name, "fusion-gpu-saturation-probe")
        self.assertEqual(config.gpu, "B200")
        self.assertEqual(config.cpu, 8)
        self.assertEqual(config.memory_mib, 40_960)
        self.assertEqual(config.timeout_s, 3_600)
        self.assertEqual(config.data_volume_name, "fusion-training-data")
        self.assertEqual(config.sample_count, 262_144)
        self.assertEqual(config.batch_size, 16_384)
        self.assertTrue(config.stage_to_tmp)
        self.assertEqual(config.fetch_mode, "sample_collate")
        self.assertTrue(config.telemetry_enabled)
        self.assertEqual(config.telemetry_interval_s, 0.5)
        self.assertEqual(config.telemetry_gpu_index, 0)
        self.assertTrue(config.telemetry_record_per_step)

    def test_probe_runtime_env_overrides(self) -> None:
        os.environ["FUSION_PROBE_GPU"] = "L4"
        os.environ["FUSION_PROBE_CPU"] = "6"
        os.environ["FUSION_PROBE_MEMORY_MIB"] = "32768"
        os.environ["FUSION_PROBE_TIMEOUT_S"] = "1800"
        os.environ["FUSION_PROBE_SAMPLE_COUNT"] = "65536"
        os.environ["FUSION_PROBE_BATCH_SIZE"] = "4096"
        os.environ["FUSION_PROBE_STAGE_TO_TMP"] = "false"
        os.environ["FUSION_PROBE_FETCH_MODE"] = "sampler_batches"
        os.environ["FUSION_PROBE_USE_BF16"] = "false"
        os.environ["FUSION_PROBE_TELEMETRY_ENABLED"] = "false"
        os.environ["FUSION_PROBE_TELEMETRY_INTERVAL_S"] = "0.25"
        os.environ["FUSION_PROBE_TELEMETRY_GPU_INDEX"] = "1"
        os.environ["FUSION_PROBE_TELEMETRY_RECORD_PER_STEP"] = "false"

        config = load_probe_runtime_config()

        self.assertEqual(config.gpu, "L4")
        self.assertEqual(config.cpu, 6)
        self.assertEqual(config.memory_mib, 32_768)
        self.assertEqual(config.timeout_s, 1_800)
        self.assertEqual(config.sample_count, 65_536)
        self.assertEqual(config.batch_size, 4_096)
        self.assertFalse(config.stage_to_tmp)
        self.assertEqual(config.fetch_mode, "sampler_batches")
        self.assertFalse(config.use_bf16)
        self.assertFalse(config.telemetry_enabled)
        self.assertEqual(config.telemetry_interval_s, 0.25)
        self.assertEqual(config.telemetry_gpu_index, 1)
        self.assertFalse(config.telemetry_record_per_step)

    def test_training_runtime_defaults(self) -> None:
        os.environ.clear()

        config = load_training_runtime_config()

        self.assertEqual(config.app_name, "fusion-training")
        self.assertEqual(config.gpu, "B200")
        self.assertEqual(config.timeout_s, 7_200)
        self.assertEqual(config.default_num_workers, 10)
        self.assertEqual(config.data_volume_name, "fusion-training-data")
        self.assertEqual(config.checkpoint_volume_name, "fusion-training-checkpoints")
        self.assertEqual(config.optuna_secret_name, "optuna-neon-db")

    def test_training_runtime_env_overrides(self) -> None:
        os.environ["FUSION_TRAINING_GPU"] = "A10"
        os.environ["FUSION_TRAINING_TIMEOUT_S"] = "5400"
        os.environ["FUSION_TRAINING_DEFAULT_NUM_WORKERS"] = "24"
        os.environ["FUSION_TRAINING_DATA_VOLUME"] = "custom-data"
        os.environ["FUSION_TRAINING_CHECKPOINT_VOLUME"] = "custom-checkpoints"
        os.environ["FUSION_TRAINING_OPTUNA_SECRET"] = "custom-secret"

        config = load_training_runtime_config()

        self.assertEqual(config.gpu, "A10")
        self.assertEqual(config.timeout_s, 5_400)
        self.assertEqual(config.default_num_workers, 24)
        self.assertEqual(config.data_volume_name, "custom-data")
        self.assertEqual(config.checkpoint_volume_name, "custom-checkpoints")
        self.assertEqual(config.optuna_secret_name, "custom-secret")

if __name__ == "__main__":
    _ = unittest.main()
