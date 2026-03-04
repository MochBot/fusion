# Copyright The Lightning AI team.
# Licensed under the Apache License, Version 2.0 (the "License");
# you may not use this file except in compliance with the License.
# You may obtain a copy of the License at
#
#     http://www.apache.org/licenses/LICENSE-2.0
#
# Unless required by applicable law or agreed to in writing, software
# distributed under the License is distributed on an "AS IS" BASIS,
# WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
# See the License for the specific language governing permissions and
# limitations under the License.

__all__ = [
    "Accelerator",
    "CPUAccelerator",
    "CUDAAccelerator",
    "MPSAccelerator",
    "XLAAccelerator",
    "find_usable_cuda_devices",
]

import sys

from lightning_fabric.accelerators import find_usable_cuda_devices
from lightning_fabric.accelerators.registry import _AcceleratorRegistry
from lightning_fabric.utilities.registry import _register_classes
from pytorch_lightning.accelerators.accelerator import Accelerator
from pytorch_lightning.accelerators.cpu import CPUAccelerator
from pytorch_lightning.accelerators.cuda import CUDAAccelerator
from pytorch_lightning.accelerators.mps import MPSAccelerator
from pytorch_lightning.accelerators.xla import XLAAccelerator

AcceleratorRegistry = _AcceleratorRegistry()
_register_classes(AcceleratorRegistry, "register_accelerators", sys.modules[__name__], Accelerator)
