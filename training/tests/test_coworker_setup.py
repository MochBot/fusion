from __future__ import annotations

import importlib.util
import os
import sys
import tomllib
import types
from pathlib import Path
from typing import cast


TRAINING_ROOT = Path(__file__).resolve().parents[1]
REPO_ROOT = TRAINING_ROOT.parent


def load_module(path: Path, name: str) -> types.ModuleType:
    spec = importlib.util.spec_from_file_location(name, path)
    assert spec is not None
    module = importlib.util.module_from_spec(spec)
    assert spec.loader is not None
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


def test_pyproject_uses_importable_hatchling_backend() -> None:
    pyproject = tomllib.loads((TRAINING_ROOT / "pyproject.toml").read_text())

    assert pyproject["build-system"]["build-backend"] == "hatchling.build"


def test_root_readme_and_env_template_document_fresh_clone_setup() -> None:
    readme = REPO_ROOT / "README.md"
    env_example = REPO_ROOT / ".env.example"

    assert readme.exists(), "fresh clone needs a root README with setup and test commands"
    assert env_example.exists(), "fresh clone needs an env template for optional private services"

    readme_text = readme.read_text()
    env_text = env_example.read_text()
    for required in [
        "cargo test --lib",
        "cargo test --test presim_validation",
        "cargo clippy -- -D warnings",
        "uv sync",
        "training/training_data_triangle_v1.bin",
        "models/pvc-real-r03.onnx",
        "WIP",
    ]:
        assert required in readme_text
    for required in [
        "GEONODE_PROXY_USER",
        "GEONODE_PROXY_PASS",
        "GEONODE_PROXY_BASE",
        "FUSION_POLICY_VALUE_LABEL_BINARY",
        "FUSION_TRAINING_DATA_VOLUME",
    ]:
        assert required in env_text


def test_xplus_replay_collector_imports_without_private_env_and_uses_repo_relative_default() -> None:
    httpx_stub = types.ModuleType("httpx")
    old_httpx = sys.modules.get("httpx")
    sys.modules["httpx"] = httpx_stub
    old_env = {key: os.environ.pop(key, None) for key in [
        "GEONODE_PROXY_USER",
        "GEONODE_PROXY_PASS",
        "GEONODE_PROXY_BASE",
    ]}
    try:
        module = load_module(
            TRAINING_ROOT / "recovered" / "replay-collector" / "collect_x_xplus_replays.py",
            "collect_x_xplus_replays",
        )
    finally:
        for key, value in old_env.items():
            if value is not None:
                os.environ[key] = value
        if old_httpx is None:
            _ = sys.modules.pop("httpx", None)
        else:
            sys.modules["httpx"] = old_httpx

    root = cast(object, getattr(module, "ROOT"))
    assert isinstance(root, Path)
    assert root == REPO_ROOT / "data" / "replays-x-xplus"


def test_runtime_model_metadata_is_documented_as_missing_large_artifact() -> None:
    manifest = REPO_ROOT / "models" / "pvc-real-r03.onnx.metadata.json"
    readme = REPO_ROOT / "README.md"

    assert manifest.exists()
    assert readme.exists()
    text = readme.read_text()
    assert "models/pvc-real-r03.onnx" in text
    assert "WIP" in text
