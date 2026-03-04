"""Export trained student weights to flat f32 binary blob.

Output format: contiguous little-endian f32 values in WEIGHT_EXPORT_ORDER.
Each Linear layer exported as weight matrix (out_features × in_features, row-major)
followed by bias vector (out_features).

Total layout:
  W1 (192×854 = 163,968 floats) | b1 (192) |
  W2 (96×192 = 18,432 floats)   | b2 (96)  |
  W3 (48×96 = 4,608 floats)     | b3 (48)  |
  W4 (9×48 = 432 floats)        | b4 (9)   |
  = 187,785 floats = 751,140 bytes ≈ 734 KB
"""

from __future__ import annotations

import struct
import sys
from pathlib import Path

import torch

try:
    from ..models.student import StudentNet
    from ..utils.config import WEIGHT_EXPORT_ORDER
except ImportError:
    from models.student import StudentNet
    from utils.config import WEIGHT_EXPORT_ORDER


LEGACY_STUDENT_KEY_MAP = {
    "layer1.weight": "hidden.linear0.weight",
    "layer1.bias": "hidden.linear0.bias",
    "layer2.weight": "hidden.linear1.weight",
    "layer2.bias": "hidden.linear1.bias",
    "layer3.weight": "hidden.linear2.weight",
    "layer3.bias": "hidden.linear2.bias",
}


def export_student_weights(
    checkpoint_path: str | Path,
    output_path: str | Path,
) -> int:
    """Export student model weights from Lightning checkpoint to flat f32 binary.

    Args:
        checkpoint_path: Path to .ckpt file (Lightning checkpoint with student state_dict)
        output_path: Path for output .bin file

    Returns:
        Number of floats written.
    """
    checkpoint_path = Path(checkpoint_path)
    output_path = Path(output_path)

    # Load checkpoint — handle both raw state_dict and Lightning checkpoint
    ckpt = torch.load(checkpoint_path, map_location="cpu", weights_only=True)

    if "state_dict" in ckpt:
        # Lightning checkpoint keys vary by module structure and torch.compile.
        # Normalize to bare StudentNet state_dict keys like:
        #   hidden.linear0.weight / output.weight
        raw_sd = ckpt["state_dict"]
        state_dict = {}
        for k, v in raw_sd.items():
            key = k
            # Strip Lightning module attr prefix (student. or model.)
            for prefix in ("student.", "model.student.", "model."):
                if key.startswith(prefix):
                    key = key[len(prefix):]
                    break
            # Strip torch.compile wrapper prefix
            key = key.removeprefix("_orig_mod.")
            key = LEGACY_STUDENT_KEY_MAP.get(key, key)
            state_dict[key] = v
    else:
        state_dict = ckpt

    # Verify all expected keys exist
    missing = [key for key, _shape in WEIGHT_EXPORT_ORDER if key not in state_dict]
    if missing:
        msg = f"Missing keys in checkpoint: {missing}"
        raise KeyError(msg)

    # Verify shapes match StudentNet
    student = StudentNet()
    expected_sd = student.state_dict()
    for key, _shape in WEIGHT_EXPORT_ORDER:
        expected_shape = expected_sd[key].shape
        actual_shape = state_dict[key].shape
        if expected_shape != actual_shape:
            msg = f"Shape mismatch for {key}: expected {expected_shape}, got {actual_shape}"
            raise ValueError(msg)

    # Write flat f32 binary
    total_floats = 0
    with open(output_path, "wb") as f:
        for key, _shape in WEIGHT_EXPORT_ORDER:
            tensor = state_dict[key].detach().float().contiguous()
            # Row-major (C-contiguous) flattening — matches Rust SIMD reader
            flat = tensor.flatten().numpy()
            f.write(flat.tobytes())  # little-endian f32 on x86
            total_floats += flat.shape[0]

    return total_floats


def main() -> None:
    """CLI: python -m training.scripts.export_weights <checkpoint.ckpt> <output.bin>"""
    if len(sys.argv) != 3:
        print(f"Usage: {sys.argv[0]} <checkpoint.ckpt> <output.bin>")
        sys.exit(1)

    ckpt_path = sys.argv[1]
    out_path = sys.argv[2]

    n = export_student_weights(ckpt_path, out_path)
    print(f"Exported {n} floats ({n * 4} bytes) to {out_path}")


if __name__ == "__main__":
    main()
