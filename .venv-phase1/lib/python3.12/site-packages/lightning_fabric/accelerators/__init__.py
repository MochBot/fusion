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
import sys

from lightning_fabric.accelerators.accelerator import Accelerator
from lightning_fabric.accelerators.cpu import CPUAccelerator  # noqa: F401
from lightning_fabric.accelerators.cuda import CUDAAccelerator, find_usable_cuda_devices  # noqa: F401
from lightning_fabric.accelerators.mps import MPSAccelerator  # noqa: F401
from lightning_fabric.accelerators.registry import _AcceleratorRegistry
from lightning_fabric.accelerators.xla import XLAAccelerator  # noqa: F401
from lightning_fabric.utilities.registry import _register_classes

ACCELERATOR_REGISTRY = _AcceleratorRegistry()
_register_classes(ACCELERATOR_REGISTRY, "register_accelerators", sys.modules[__name__], Accelerator)
