from __future__ import annotations

import importlib.util
import shutil
import sys
import tempfile
import unittest
from importlib.abc import Loader
from importlib.machinery import ModuleSpec
from pathlib import Path
from types import ModuleType
from typing import cast


class ModalAppBootstrapTests(unittest.TestCase):
    def test_top_level_modal_app_copy_bootstraps_training_imports(self) -> None:
        repo_root = Path(__file__).resolve().parents[2]
        source_modal_app = repo_root / "training" / "scripts" / "modal_app.py"
        source_training_dir = repo_root / "training"

        with tempfile.TemporaryDirectory() as tmpdir:
            temp_root = Path(tmpdir)
            copied_modal_app = temp_root / "modal_app.py"
            _ = shutil.copy2(source_modal_app, copied_modal_app)
            _ = shutil.copytree(source_training_dir / "scripts", temp_root / "training" / "scripts")
            _ = shutil.copytree(source_training_dir / "utils", temp_root / "training" / "utils")

            original_sys_path = list(sys.path)
            original_modules = dict(sys.modules)
            try:
                sys.path = [entry for entry in sys.path if str(temp_root) not in entry]
                for name in list(sys.modules):
                    if name == "training" or name.startswith("training.") or name.startswith("scripts."):
                        _ = sys.modules.pop(name, None)

                spec = importlib.util.spec_from_file_location("modal_app", copied_modal_app)
                if spec is None or spec.loader is None:
                    self.fail("expected spec and loader for copied modal_app")

                module = cast(ModuleType, importlib.util.module_from_spec(spec))
                loader = cast(Loader, spec.loader)
                loader.exec_module(module)

                preprocess_module = cast(ModuleType, module._import_training_script_module("preprocess_replays"))
                pipeline_module = cast(ModuleType, module._import_training_script_module("policy_value_pipeline"))
                self.assertTrue(hasattr(module, "PROFILE"))
                self.assertTrue(hasattr(preprocess_module, "split_replay_files"))
                self.assertTrue(hasattr(pipeline_module, "artifact_readiness"))
            finally:
                sys.path = original_sys_path
                sys.modules.clear()
                sys.modules.update(original_modules)
