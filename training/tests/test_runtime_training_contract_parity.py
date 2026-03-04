from __future__ import annotations

import unittest
import sys
from pathlib import Path

TRAINING_ROOT = Path(__file__).resolve().parents[1]
if str(TRAINING_ROOT) not in sys.path:
    sys.path.insert(0, str(TRAINING_ROOT))

from utils import example_schema as schema


def _load_fixture() -> dict[str, tuple[str, ...] | str]:
    fixture_path = Path(__file__).resolve().parent / "fixtures" / "phase0_contract_fixture.txt"
    parsed: dict[str, tuple[str, ...] | str] = {}
    for line in fixture_path.read_text().splitlines():
        if not line.strip():
            continue
        key, value = line.split("=", 1)
        parsed[key] = tuple(value.split(",")) if "," in value else value
    return parsed


class RuntimeTrainingParityTests(unittest.TestCase):
    def test_runtime_external_piece_order_is_documented_in_schema(self) -> None:
        fixture = _load_fixture()
        self.assertEqual(schema.RUNTIME_EXTERNAL_PIECE_ORDER, fixture["runtime_external_piece_order"])
        self.assertEqual(schema.PIECE_ORDER, fixture["training_piece_order"])
        self.assertNotEqual(schema.RUNTIME_EXTERNAL_PIECE_ORDER, schema.PIECE_ORDER)

    def test_metadata_carries_runtime_piece_order_for_cross_layer_checks(self) -> None:
        fixture = _load_fixture()
        metadata = schema.build_dataset_metadata(sample_count=3)
        self.assertEqual(metadata["schema_version"], fixture["schema_version"])
        self.assertEqual(tuple(metadata["runtime_external_piece_order"]), fixture["runtime_external_piece_order"])
        self.assertEqual(tuple(metadata["scalar_order"]), fixture["scalar_order"])


if __name__ == "__main__":
    unittest.main()
