from __future__ import annotations

import argparse
import asyncio
import importlib.util
import json
import sys
from collections.abc import Iterator
from pathlib import Path
from types import ModuleType
from typing import Literal, assert_never

import httpx
import pytest


def load_helpers() -> ModuleType:
    source = Path(__file__).with_name("test_wave_xxplus_u_ss.py")
    spec = importlib.util.spec_from_file_location("wave_destination_helpers", source)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


@pytest.mark.parametrize("root_kind", ["external", "parent", "alias"])
def test_resume_reuses_external_body_despite_partial_destination(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch,
    root_kind: Literal["external", "parent", "alias"],
) -> None:
    helpers = load_helpers()
    _, _, driver = helpers.load_wave()
    corpus = tmp_path / "corpus"
    out = corpus / "wave"
    meta = out / "_meta"
    meta.mkdir(parents=True)
    manifest = meta / "manifest_TEST.json"
    manifest.write_text(json.dumps({"wave": "test", "unique_selected": 1}))
    (meta / "records.jsonl").write_text('{"replayid":"shared"}\n')
    (out / "bodies").mkdir()
    destination = out / "bodies" / "shared.ttrm"
    destination.write_bytes(b"partial")
    external = corpus / "external"
    external.mkdir()
    (external / "shared.ttrm").write_bytes(helpers.VALID_BODY)
    match root_kind:
        case "external":
            root = external
        case "parent":
            root = corpus
        case "alias":
            root = tmp_path / "alias"
            root.symlink_to(corpus, target_is_directory=True)
        case unexpected:
            assert_never(unexpected)
    original_glob = Path.glob

    def destination_first(root: Path, pattern: str) -> Iterator[Path]:
        return iter(sorted(original_glob(root, pattern),
                           key=lambda path: path.resolve() != destination.resolve()))

    monkeypatch.setattr(Path, "glob", destination_first)
    calls: list[list[str]] = []

    async def capture_download(
        args: argparse.Namespace, queue: list[str],
        clients: dict[int, httpx.AsyncClient] | None,
        *, resume_manifest_path: Path | None = None,
    ) -> dict[str, int]:
        calls.append(list(queue))
        return {"attempted": len(queue)}

    monkeypatch.setattr(driver, "download_queue_concurrent", capture_download)
    result, _ = asyncio.run(driver.run_resume(
        helpers.resume_args(out, manifest, existing_root=[root])
    ))

    assert calls == [[]]
    assert result["resume_summary"]["reused_cross_corpus"] == 1
    assert destination.read_bytes() == helpers.VALID_BODY
