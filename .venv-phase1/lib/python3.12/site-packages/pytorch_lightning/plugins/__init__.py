from typing import Union

from lightning_fabric.plugins import CheckpointIO, ClusterEnvironment, TorchCheckpointIO, XLACheckpointIO
from pytorch_lightning.plugins.io.async_plugin import AsyncCheckpointIO
from pytorch_lightning.plugins.layer_sync import LayerSync, TorchSyncBatchNorm
from pytorch_lightning.plugins.precision.amp import MixedPrecision
from pytorch_lightning.plugins.precision.bitsandbytes import BitsandbytesPrecision
from pytorch_lightning.plugins.precision.deepspeed import DeepSpeedPrecision
from pytorch_lightning.plugins.precision.double import DoublePrecision
from pytorch_lightning.plugins.precision.fsdp import FSDPPrecision
from pytorch_lightning.plugins.precision.half import HalfPrecision
from pytorch_lightning.plugins.precision.precision import Precision
from pytorch_lightning.plugins.precision.transformer_engine import TransformerEnginePrecision
from pytorch_lightning.plugins.precision.xla import XLAPrecision

_PLUGIN_INPUT = Union[Precision, ClusterEnvironment, CheckpointIO, LayerSync]

__all__ = [
    "AsyncCheckpointIO",
    "CheckpointIO",
    "TorchCheckpointIO",
    "XLACheckpointIO",
    "BitsandbytesPrecision",
    "DeepSpeedPrecision",
    "DoublePrecision",
    "HalfPrecision",
    "MixedPrecision",
    "Precision",
    "TransformerEnginePrecision",
    "FSDPPrecision",
    "XLAPrecision",
    "LayerSync",
    "TorchSyncBatchNorm",
]
