from __future__ import annotations

import tempfile
import unittest
from pathlib import Path
import sys

import numpy as np

TRAINING_ROOT = Path(__file__).resolve().parents[1]
if str(TRAINING_ROOT) not in sys.path:
    sys.path.insert(0, str(TRAINING_ROOT))

from utils import example_schema as schema


class ExampleSchemaTests(unittest.TestCase):
    def _example(self) -> schema.CanonicalExample:
        return schema.CanonicalExample(
            identity=schema.ExampleIdentity(
                schema_version=schema.SCHEMA_VERSION,
                replay_id="replay-1",
                round_id=0,
                player_id=0,
                frame_id=12,
                group_id="replay-1:round:0",
            ),
            player_board=np.zeros((40, 10), dtype=np.float32),
            opponent_board=np.zeros((40, 10), dtype=np.float32),
            current_piece="i",
            hold_piece="t",
            queue=("j", "l", "o"),
            combo=4,
            b2b=2,
            lines_cleared_total=17,
            pending_garbage=3,
            bag_number=5,
            game_outcome=1.0,
            lines_sent=0.5,
            b2b_after=0.2,
            position_normalized=0.25,
            time_to_topout=0.8,
        )

    def test_metadata_round_trip_validates(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            data_path = Path(tmpdir) / "training_data.bin"
            schema.write_dataset_metadata(data_path, sample_count=7)
            metadata = schema.load_dataset_metadata(data_path)
            self.assertEqual(metadata["schema_version"], schema.SCHEMA_VERSION)
            self.assertEqual(tuple(metadata["scalar_order"]), schema.SCALAR_ORDER)
            self.assertEqual(tuple(metadata["piece_order"]), schema.PIECE_ORDER)

    def test_flatten_example_uses_named_scalar_slots(self) -> None:
        flat = schema.flatten_example(self._example())
        self.assertEqual(flat.shape, (859,))
        scalar_start = schema.FEATURE_LAYOUT["scalars"][0]
        self.assertAlmostEqual(flat[scalar_start + schema.scalar_slot("combo")], 0.2)
        self.assertAlmostEqual(flat[scalar_start + schema.scalar_slot("bag_number")], 0.25)

    def test_validate_dataset_metadata_rejects_wrong_scalar_order(self) -> None:
        metadata = schema.build_dataset_metadata(sample_count=1)
        metadata["scalar_order"] = ["bag_number", "combo", "b2b", "lines", "garbage_pending"]
        with self.assertRaises(ValueError):
            schema.validate_dataset_metadata(metadata)


if __name__ == "__main__":
    unittest.main()
