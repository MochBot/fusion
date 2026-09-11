"""Focused tests for the X/X+/U/SS wave discovery driver.

Covers CLI defaults, current-rank selection, global-newest recency,
cross-rank dedup with ref preservation, pagination exhaustion versus
explicit truncation, page-failure fail-closed behavior, existing-body
split, and the dry-run surface (no body fetch without --download).
"""

from __future__ import annotations

import argparse
import asyncio
import importlib.util
import json
import subprocess
import sys
from pathlib import Path

import pytest

COLLECTOR_DIR = Path(__file__).resolve().parents[1] / "recovered" / "replay-collector"
DRIVER_PATH = COLLECTOR_DIR / "collect_wave_xxplus_u_ss.py"


def load_module(path: Path, name: str):
    spec = importlib.util.spec_from_file_location(name, path)
    assert spec is not None
    module = importlib.util.module_from_spec(spec)
    assert spec.loader is not None
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


def load_wave():
    selection = load_module(COLLECTOR_DIR / "wave_selection.py", "wave_selection_wave_test")
    discovery = load_module(COLLECTOR_DIR / "wave_discovery.py", "wave_discovery_wave_test")
    driver = load_module(DRIVER_PATH, "collect_wave_xxplus_u_ss_test")
    return selection, discovery, driver


def make_ref(selection, replayid: str, ts: str | None, player: str = "player-a", rank: str = "u"):
    return selection.WaveRef(
        replayid=selection.ReplayId(replayid),
        record_id=f"record-{replayid}",
        player_id=selection.PlayerId(player),
        rank=rank,
        ts=ts,
        ts_epoch=selection.parse_ts_epoch(ts),
        opponents=(),
    )


def record_entry(replayid: str, ts: str, pri: int = 1) -> dict[str, object]:
    return {"_id": f"record-{replayid}", "replayid": replayid, "ts": ts,
            "gamemode": "league", "p": {"pri": pri, "sec": 0, "ter": 0}}


def test_cli_defaults_select_full_wave_and_dry_run():
    _selection, _discovery, driver = load_wave()

    args = driver.parse_args([])

    assert args.ranks == ("x", "x+", "u", "ss")
    assert args.u_top_n == 2000
    assert args.ss_top_n == 2000
    assert args.record_pages == 50
    assert args.download is False


def test_cli_rejects_ranks_outside_wave():
    _selection, _discovery, driver = load_wave()

    with pytest.raises(SystemExit):
        driver.parse_args(["--ranks", "x+,s+"])


def test_cli_help_surface_lists_wave_options():
    proc = subprocess.run(
        [sys.executable, str(DRIVER_PATH), "--help"],
        capture_output=True,
        text=True,
        cwd=COLLECTOR_DIR,
        timeout=60,
    )

    assert proc.returncode == 0
    for flag in ("--ranks", "--u-top-n", "--ss-top-n", "--record-pages", "--download"):
        assert flag in proc.stdout


def test_ref_from_record_drops_stubs_and_idless_entries():
    selection, _discovery, _driver = load_wave()

    assert selection.ref_from_record(selection.PlayerId("p"), "u", {"stub": True, "replayid": "r"}) is None
    assert selection.ref_from_record(selection.PlayerId("p"), "u", {"ts": "2026-01-01T00:00:00Z"}) is None
    ref = selection.ref_from_record(
        selection.PlayerId("p"), "u", {"replayid": "r", "ts": "2026-01-01T00:00:00Z"})
    assert ref is not None and ref.replayid == "r"


def test_select_global_newest_orders_across_players_and_dedups():
    selection, _discovery, _driver = load_wave()
    refs = [
        make_ref(selection, "old", "2026-01-01T00:00:00Z", player="a"),
        make_ref(selection, "new", "2026-03-01T00:00:00Z", player="b"),
        make_ref(selection, "mid", "2026-02-01T00:00:00Z", player="c"),
        make_ref(selection, "new", "2026-01-15T00:00:00Z", player="d"),
        make_ref(selection, "unparseable", "not-a-timestamp", player="e"),
    ]

    selected = selection.select_global_newest(refs, 2)

    assert [r.replayid for r in selected] == ["new", "mid"]
    assert selected[0].player_id == "b"


def test_dedup_all_keeps_every_id_once_newest_first():
    selection, _discovery, _driver = load_wave()
    refs = [
        make_ref(selection, "a", "2026-01-01T00:00:00Z", player="p1", rank="x"),
        make_ref(selection, "a", "2026-02-01T00:00:00Z", player="p2", rank="x+"),
        make_ref(selection, "b", "2026-01-15T00:00:00Z", player="p1", rank="x"),
    ]

    selected = selection.dedup_all(refs)

    assert [r.replayid for r in selected] == ["a", "b"]
    assert selected[0].player_id == "p2"


def test_qualifies_for_next_page_stops_only_when_provably_older():
    selection, _discovery, _driver = load_wave()

    assert selection.qualifies_for_next_page(None, False, 100.0) is True
    assert selection.qualifies_for_next_page(50.0, False, 100.0) is False
    assert selection.qualifies_for_next_page(150.0, False, 100.0) is True
    assert selection.qualifies_for_next_page(150.0, True, 100.0) is False


def test_split_existing_separates_coverage_from_missing():
    selection, _discovery, _driver = load_wave()
    refs = [make_ref(selection, "on-disk", "2026-01-01T00:00:00Z"),
            make_ref(selection, "absent", "2026-01-02T00:00:00Z")]

    on_disk, missing = selection.split_existing(refs, {"on-disk"})

    assert on_disk == ["on-disk"]
    assert missing == ["absent"]


def test_inventory_existing_bodies_reads_stems_only(tmp_path):
    selection, _discovery, _driver = load_wave()
    bodies = tmp_path / "x" / "player-a"
    bodies.mkdir(parents=True)
    (bodies / "abc123.ttrm").write_bytes(b"{}")
    (bodies / "notes.txt").write_text("not a replay")

    assert selection.inventory_existing_bodies([tmp_path, tmp_path / "missing"]) == {"abc123"}


def test_leaderboard_stops_past_band_and_reports_complete(monkeypatch):
    _selection, discovery, _driver = load_wave()
    pages = iter([
        {"success": True, "data": {"entries": [
            {"_id": "x1", "league": {"rank": "x"}, "p": {"pri": 1, "sec": 0, "ter": 0}},
            {"_id": "u1", "league": {"rank": "u"}, "p": {"pri": 2, "sec": 0, "ter": 0}},
        ]}},
        {"success": True, "data": {"entries": [
            {"_id": "s1", "league": {"rank": "s+"}, "p": {"pri": 3, "sec": 0, "ter": 0}},
        ]}},
    ])

    async def fetch(_client, _url):
        return next(pages)

    async def no_sleep(_delay: float) -> None:
        return None

    monkeypatch.setattr(discovery, "fetch_json", fetch)
    monkeypatch.setattr(discovery.asyncio, "sleep", no_sleep)
    players, report = asyncio.run(discovery.fetch_leaderboard_players(object(), ("x", "x+", "u", "ss"), 10))

    assert [p["_id"] for p in players["x"]] == ["x1"]
    assert [p["_id"] for p in players["u"]] == ["u1"]
    assert report["complete"] is True
    assert report["stop_reason"] == "past_band_at_s+"


def test_leaderboard_reports_fetch_failure_without_claiming_complete(monkeypatch):
    _selection, discovery, _driver = load_wave()

    async def fetch(_client, _url):
        return None

    monkeypatch.setattr(discovery, "fetch_json", fetch)
    players, report = asyncio.run(discovery.fetch_leaderboard_players(object(), ("x",), 10))

    assert players == {"x": []}
    assert report["complete"] is False
    assert report["stop_reason"] == "fetch_failed"


def test_records_take_all_paginates_to_exhaustion_for_x(monkeypatch):
    _selection, discovery, _driver = load_wave()
    page_one = [record_entry(f"r{i}", "2026-02-01T00:00:00Z", pri=i) for i in range(100)]
    page_two = [record_entry("tail", "2026-01-01T00:00:00Z", pri=101)]

    async def fetch_page(_client, _pid, cursor):
        return page_one if cursor is None else page_two

    monkeypatch.setattr(discovery, "fetch_record_page", fetch_page)

    async def no_sleep(_delay: float) -> None:
        return None

    monkeypatch.setattr(discovery.asyncio, "sleep", no_sleep)
    selected, report = asyncio.run(
        discovery.discover_rank_refs(object(), "x", [{"_id": "p1"}], None, 10))

    assert report["complete"] is True
    assert report["records_scanned"] == 101
    assert len(selected) == 101


def test_records_take_all_reports_cap_instead_of_claiming_complete(monkeypatch):
    _selection, discovery, _driver = load_wave()
    page = [record_entry(f"r{i}", "2026-02-01T00:00:00Z", pri=i) for i in range(100)]

    async def fetch_page(_client, _pid, _cursor):
        return page

    monkeypatch.setattr(discovery, "fetch_record_page", fetch_page)

    async def no_sleep(_delay: float) -> None:
        return None

    monkeypatch.setattr(discovery.asyncio, "sleep", no_sleep)
    _selected, report = asyncio.run(
        discovery.discover_rank_refs(object(), "x+", [{"_id": "p1"}], None, 1))

    assert report["complete"] is False
    assert report["truncated_players"] == ["p1"]


def test_records_page_failure_is_fail_closed_per_player(monkeypatch):
    _selection, discovery, _driver = load_wave()

    async def fetch_page(_client, pid, _cursor):
        if pid == "bad":
            raise RuntimeError("records page failed for bad")
        return [record_entry("ok", "2026-02-01T00:00:00Z")]

    monkeypatch.setattr(discovery, "fetch_record_page", fetch_page)

    async def no_sleep(_delay: float) -> None:
        return None

    monkeypatch.setattr(discovery.asyncio, "sleep", no_sleep)
    selected, report = asyncio.run(
        discovery.discover_rank_refs(object(), "x", [{"_id": "bad"}, {"_id": "good"}], None, 10))

    assert report["complete"] is False
    assert any("bad" in entry for entry in report["failed_players"])
    assert [r.replayid for r in selected] == ["ok"]


def test_u_top_n_follows_leaderboard_order_and_stops_after_target(monkeypatch):
    selection, discovery, _driver = load_wave()
    fresh_page = [record_entry(f"fresh{i}", f"2026-03-{(i % 28) + 1:02d}T00:00:00Z", pri=i) for i in range(100)]
    stale_page = [record_entry(f"stale{i}", "2026-01-01T00:00:00Z", pri=i) for i in range(100)]
    calls: list[str] = []

    async def fetch_page(_client, pid, cursor):
        calls.append(pid)
        return fresh_page if pid == "fresh" else stale_page

    monkeypatch.setattr(discovery, "fetch_record_page", fetch_page)

    async def no_sleep(_delay: float) -> None:
        return None

    monkeypatch.setattr(discovery.asyncio, "sleep", no_sleep)
    selected, report = asyncio.run(
        discovery.discover_rank_refs(object(), "u", [{"_id": "fresh"}, {"_id": "stale"}], 50, 10))

    assert report["complete"] is True
    assert len(selected) == 50
    assert all(r.replayid.startswith("fresh") for r in selected)
    assert calls == ["fresh"]


def test_dry_run_writes_manifest_and_fetches_no_bodies(tmp_path, monkeypatch):
    selection, discovery, driver = load_wave()
    out = tmp_path / "wave"
    bodies: list[str] = []

    async def fake_leaderboard(_client, ranks, _cap):
        return (
            {"x": [{"_id": "p1", "league": {"rank": "x"}}], "x+": [], "u": [], "ss": []},
            {"pages": 1, "players_seen": 1, "stop_reason": "past_band_at_s+", "complete": True},
        )

    async def fake_rank_refs(_client, rank, _entries, _top_n, _max_pages):
        ref = make_ref(selection, "replay-1", "2026-02-01T00:00:00Z", player="p1", rank=rank)
        return [ref], {"players": 1, "records_scanned": 1, "pages": 1,
                       "truncated_players": [], "failed_players": [], "complete": True}

    async def fail_download(*_args, **_kwargs):
        bodies.append("called")
        raise AssertionError("dry-run must not fetch bodies")

    monkeypatch.setattr(driver, "fetch_leaderboard_players", fake_leaderboard)
    monkeypatch.setattr(driver, "discover_rank_refs", fake_rank_refs)
    monkeypatch.setattr(driver, "download_one", fail_download)
    monkeypatch.setattr(driver, "STAMP", "TESTSTAMP")
    args = argparse.Namespace(ranks=("x",), u_top_n=2000, ss_top_n=2000, record_pages=2,
                              leaderboard_pages=2, out=out, existing_root=[],
                              download=False, session_id="test")
    monkeypatch.setattr(driver, "DEFAULT_EXISTING_ROOTS", ())

    manifest, manifest_path = asyncio.run(driver.amain(args))

    assert bodies == []
    assert manifest["download_launched"] is False
    assert manifest["pagination_complete"] is True
    assert manifest["ranks"]["x"]["selected_unique"] == 1
    assert manifest["missing_unique"] == 1
    assert manifest["byte_estimate"] == driver.HISTORICAL_MEAN_BODY_BYTES
    assert manifest_path.exists()
    assert (out / "_meta" / "records.jsonl").exists()
    assert json.loads(manifest_path.read_text())["unique_selected"] == 1


def make_valid_body(size_events: int = 2500) -> bytes:
    side = {"replay": {"events": list(range(size_events))}}
    return json.dumps({"gamemode": "league", "replay": {"rounds": [[side, side]]}}).encode()


VALID_BODY = make_valid_body()


def write_resume_fixture(out: Path) -> Path:
    meta = out / "_meta"
    meta.mkdir(parents=True, exist_ok=True)
    manifest_path = meta / "manifest_TEST.json"
    manifest_path.write_text(json.dumps({"wave": "xxplus-u-ss-01", "unique_selected": 4}))
    with (meta / "records.jsonl").open("w") as handle:
        for rid in ("done-ledger", "done-body", "retry-proxy", "fresh"):
            handle.write(json.dumps({"replayid": rid}) + "\n")
    with (meta / "downloads.jsonl").open("w") as handle:
        handle.write(json.dumps({"replayid": "done-ledger", "status": 200}) + "\n")
        handle.write(json.dumps({"replayid": "retry-proxy", "status": 429,
                                 "err": "ProxyError", "via": "geonode:9000"}) + "\n")
    bodies = out / "bodies"
    bodies.mkdir(parents=True, exist_ok=True)
    (bodies / "done-body.ttrm").write_bytes(VALID_BODY)
    (bodies / "done-ledger.ttrm").write_bytes(VALID_BODY)
    return manifest_path


def resume_args(out: Path, manifest_path: Path, **overrides: object) -> argparse.Namespace:
    base: dict[str, object] = {"ranks": ("x",), "u_top_n": 2000, "ss_top_n": 2000,
                               "record_pages": 2, "leaderboard_pages": 2, "out": out,
                               "existing_root": [], "download": False, "use_geonode": False,
                               "geonode_stop_file": None, "resume": True, "manifest": manifest_path,
                               "download_concurrency": 4, "session_id": "resume-test"}
    base.update(overrides)
    return argparse.Namespace(**base)


def test_resume_reuses_manifest_with_no_discovery(tmp_path, monkeypatch):
    _selection, discovery, driver = load_wave()
    out = tmp_path / "wave"
    manifest_path = write_resume_fixture(out)
    attempted: list[str] = []

    async def fail_discovery(*_args: object, **_kwargs: object) -> object:
        raise AssertionError("resume must not run discovery")

    async def fake_download_one(_gate: object, _direct: object, _geo: object,
                                replayid: str, out_path: Path, _stop: object) -> object:
        attempted.append(replayid)
        out_path.parent.mkdir(parents=True, exist_ok=True)
        out_path.write_bytes(VALID_BODY)
        return driver.collector.DLOutcome(replayid, 200, len(VALID_BODY), None, 0, "direct", 1, 1.0)

    monkeypatch.setattr(driver, "fetch_leaderboard_players", fail_discovery)
    monkeypatch.setattr(driver, "discover_rank_refs", fail_discovery)
    monkeypatch.setattr(driver, "run_discovery", fail_discovery)
    monkeypatch.setattr(driver, "download_one", fake_download_one)

    manifest, returned = asyncio.run(driver.amain(resume_args(out, manifest_path)))

    assert returned == manifest_path
    assert manifest["resume_summary"]["attempted"] == 2
    assert sorted(attempted) == ["fresh", "retry-proxy"]
    assert (out / "_meta" / "leaderboard.jsonl").exists() is False


def test_resume_download_uses_bounded_actual_concurrency_and_flushes(tmp_path, monkeypatch):
    _selection, _discovery, driver = load_wave()
    out = tmp_path / "wave"
    meta = out / "_meta"
    meta.mkdir(parents=True, exist_ok=True)
    queue = [f"r{i}" for i in range(12)]
    state = {"current": 0, "max_seen": 0}

    async def fake_download_one(gate: object, _direct: object, _geo: object,
                                replayid: str, out_path: Path, _stop: object) -> object:
        async with gate:
            state["current"] += 1
            state["max_seen"] = max(state["max_seen"], state["current"])
            await asyncio.sleep(0.01)
            out_path.parent.mkdir(parents=True, exist_ok=True)
            out_path.write_bytes(VALID_BODY)
            state["current"] -= 1
        return driver.collector.DLOutcome(replayid, 200, len(VALID_BODY), None, 0, "direct", 1, 1.0)

    monkeypatch.setattr(driver, "download_one", fake_download_one)
    args = resume_args(out, None, manifest=None, download_concurrency=4)

    summary = asyncio.run(
        driver.download_queue_concurrent(args, queue, None, resume_manifest_path=None))

    assert summary["attempted"] == 12
    assert summary["success"] == 12
    assert state["max_seen"] > 1
    assert state["max_seen"] <= 4
    lines = (meta / "downloads.jsonl").read_text().splitlines()
    assert len(lines) == 12
    assert {json.loads(line)["replayid"] for line in lines} == set(queue)


def test_proxy_probe_reports_sanitized_classification(monkeypatch):
    _selection, _discovery, driver = load_wave()
    secret_url = "http://user:secret@proxy.geonode.io:9000"

    class FakeStream:
        async def __aenter__(self) -> object:
            raise driver.httpx.ProxyError(f"proxy {secret_url} failed")
        async def __aexit__(self, *exc: object) -> bool:
            return False

    class FakeClient:
        def stream(self, *args: object, **kwargs: object) -> FakeStream:
            return FakeStream()
        async def aclose(self) -> None:
            return None

    monkeypatch.setattr(driver, "make_geonode_client", lambda *a, **k: FakeClient())

    working, classification = asyncio.run(driver.diagnose_proxy_once("test", "probe-id"))

    assert working is False
    assert classification == "ProxyError"
    assert "geonode" not in classification
    assert "secret" not in classification


def test_proxy_probe_missing_config_without_credentials(monkeypatch):
    _selection, _discovery, driver = load_wave()

    def raise_missing(*args: object, **kwargs: object) -> object:
        raise RuntimeError("GEONODE_PROXY_USER must be set")

    monkeypatch.setattr(driver, "make_geonode_client", raise_missing)

    working, classification = asyncio.run(driver.diagnose_proxy_once("test", "probe-id"))

    assert working is False
    assert classification == "proxy_config_missing"


def test_proxy_url_applies_geonode_type_suffix_to_bare_login(monkeypatch):
    _selection, _discovery, driver = load_wave()
    collector = driver.collector

    monkeypatch.setenv("GEONODE_PROXY_USER", "base_login")
    monkeypatch.setenv("GEONODE_PROXY_PASS", "secret")
    monkeypatch.setenv("GEONODE_PROXY_HOST", "us.proxy.geonode.io:9000")

    url = collector.proxy_url(9000)
    assert url.startswith("http://base_login-type-residential:secret@us.proxy.geonode.io:9000")

    country_url = collector.proxy_url(9001, country="US")
    assert "base_login-type-residential-country-us:secret@" in country_url

    monkeypatch.setenv("GEONODE_PROXY_USER", "base_login-type-datacenter")
    suffixed = collector.proxy_url(9000)
    assert suffixed.startswith("http://base_login-type-datacenter:secret@")


def test_resume_reuses_valid_cross_corpus_body_without_fetch(tmp_path, monkeypatch):
    _selection, _discovery, driver = load_wave()
    out = tmp_path / "wave"
    manifest_path = write_resume_fixture(out)
    corpus = tmp_path / "corpus-objects"
    shard = corpus / "fr"
    shard.mkdir(parents=True)
    (shard / "fresh.ttrm").write_bytes(VALID_BODY)
    attempted: list[str] = []

    async def fake_download_one(_gate: object, _direct: object, _geo: object,
                                replayid: str, out_path: Path, _stop: object) -> object:
        attempted.append(replayid)
        out_path.parent.mkdir(parents=True, exist_ok=True)
        out_path.write_bytes(VALID_BODY)
        return driver.collector.DLOutcome(replayid, 200, len(VALID_BODY), None, 0, "direct", 1, 1.0)

    monkeypatch.setattr(driver, "download_one", fake_download_one)

    manifest, _ = asyncio.run(
        driver.amain(resume_args(out, manifest_path, existing_root=[corpus])))

    assert attempted == ["retry-proxy"]
    assert (out / "bodies" / "fresh.ttrm").read_bytes() == VALID_BODY
    summary = manifest["resume_summary"]
    assert summary["reused_cross_corpus"] == 1
    assert summary["reused_ids"] == ["fresh"]
    provenance = (out / "_meta" / "reused_provenance.jsonl").read_text().splitlines()
    assert {json.loads(line)["replayid"] for line in provenance} == {"fresh"}


def write_wave_pair(tmp_path: Path) -> tuple[Path, Path]:
    """Two wave dirs with distinct records selections for resume-scope tests.

    Given: an empty tmp dir.
    When: populated with wave-a (records: from-manifest-A) and
    wave-b (records: from-output-B), each with its own manifest.
    Then: returns (out_a, out_b) with disjoint selections.
    """
    out_a = tmp_path / "wave-a"
    out_b = tmp_path / "wave-b"
    for out, rids in ((out_a, ("from-manifest-A",)), (out_b, ("from-output-B",))):
        meta = out / "_meta"
        meta.mkdir(parents=True, exist_ok=True)
        (meta / "manifest_TEST.json").write_text(json.dumps({"wave": "xxplus-u-ss-01"}))
        with (meta / "records.jsonl").open("w") as handle:
            for rid in rids:
                handle.write(json.dumps({"replayid": rid}) + "\n")
    return out_a, out_b


def test_resolve_existing_roots_explicit_replaces_defaults():
    """Explicit --existing-root wins with the wave dir first; empty keeps defaults.

    Given: a wave dir, custom roots, and the project defaults.
    When: roots are resolved with and without explicit overrides.
    Then: explicit roots replace (not extend) the defaults in both cases.
    """
    _selection, _discovery, driver = load_wave()
    wave = Path("/tmp/wave-x")
    custom = [Path("/tmp/custom-a"), Path("/tmp/custom-b")]

    assert driver.resolve_existing_roots(wave, custom) == [wave, *custom]
    assert driver.resolve_existing_roots(wave, []) == [wave, *list(driver.DEFAULT_EXISTING_ROOTS)]


def test_run_download_honors_explicit_existing_root(tmp_path, monkeypatch):
    """run_download consumes --existing-root and skips IDs found in it.

    Given: a wave out dir whose records.jsonl wants two IDs, one of which
    already has a body in an explicit external corpus dir.
    When: run_download runs with existing_root=[corpus] and a fake fetcher.
    Then: only the missing ID is fetched; the corpus ID is never queued.
    """
    _selection, _discovery, driver = load_wave()
    out = tmp_path / "wave"
    meta = out / "_meta"
    meta.mkdir(parents=True, exist_ok=True)
    with (meta / "records.jsonl").open("w") as handle:
        for rid in ("already-here", "needs-fetch"):
            handle.write(json.dumps({"replayid": rid}) + "\n")
    corpus = tmp_path / "external-corpus"
    (corpus / "x").mkdir(parents=True)
    (corpus / "x" / "already-here.ttrm").write_bytes(VALID_BODY)
    attempted: list[str] = []

    async def fake_download_one(_gate: object, _direct: object, _geo: object,
                                replayid: str, out_path: Path, _stop: object) -> object:
        attempted.append(replayid)
        out_path.parent.mkdir(parents=True, exist_ok=True)
        out_path.write_bytes(VALID_BODY)
        return driver.collector.DLOutcome(replayid, 200, len(VALID_BODY), None, 0, "direct", 1, 1.0)

    monkeypatch.setattr(driver, "download_one", fake_download_one)

    class FakeClient:
        async def aclose(self) -> None:
            return None

    monkeypatch.setattr(driver, "make_direct_client", lambda **_kwargs: FakeClient())
    args = argparse.Namespace(out=out, existing_root=[corpus], session_id="test",
                              use_geonode=False, geonode_stop_file=None,
                              download_concurrency=4)

    asyncio.run(driver.run_download(args, {}))

    assert attempted == ["needs-fetch"]


def test_resume_rejects_manifest_outside_out_meta(tmp_path, monkeypatch):
    """Resume fails closed when the manifest lives outside <out>/_meta.

    Given: manifest from wave-a but --out pointing at wave-b (disjoint records).
    When: run_resume is invoked.
    Then: ValueError names --out, raised before any records read, body
    check/copy, directory creation, or network client construction.
    """
    _selection, _discovery, driver = load_wave()
    out_a, out_b = write_wave_pair(tmp_path)
    manifest_a = out_a / "_meta" / "manifest_TEST.json"

    def tripwire(*_args: object, **_kwargs: object) -> object:
        raise AssertionError("resume must fail before records/ledger/body/network use")

    monkeypatch.setattr(driver, "load_resume_wanted", tripwire)
    monkeypatch.setattr(driver, "ledger_succeeded_ids", tripwire)
    monkeypatch.setattr(driver, "validated_body_ids", tripwire)
    monkeypatch.setattr(driver, "reuse_cross_corpus_bodies", tripwire)
    monkeypatch.setattr(driver, "download_queue_concurrent", tripwire)
    monkeypatch.setattr(driver, "make_direct_client", tripwire)

    with pytest.raises(ValueError, match="--out"):
        asyncio.run(driver.run_resume(resume_args(out_b, manifest_a)))


def test_resume_accepts_matching_default_and_normalized_manifest(tmp_path, monkeypatch):
    """Matching explicit, default-selected, and normalized manifests all resume.

    Given: wave-a with one wanted ID and no ledger/bodies.
    When: resumed via explicit manifest, default selection, and
    dot-dot-normalized aliases of both --out and --manifest.
    Then: each run queues wave-a's records (never wave-b's) and downloads them.
    """
    _selection, _discovery, driver = load_wave()
    out_a, out_b = write_wave_pair(tmp_path)
    _ = out_b
    manifest_a = out_a / "_meta" / "manifest_TEST.json"
    (tmp_path / "sub").mkdir(exist_ok=True)
    (out_a / "_meta" / "extra").mkdir(exist_ok=True)
    cases = [
        (out_a, manifest_a),
        (out_a, None),
        (tmp_path / "sub" / ".." / "wave-a",
         out_a / "_meta" / "extra" / ".." / "manifest_TEST.json"),
    ]
    for out_arg, manifest_arg in cases:
        attempted: list[str] = []

        async def fake_download_one(_gate: object, _direct: object, _geo: object,
                                    replayid: str, out_path: Path, _stop: object) -> object:
            attempted.append(replayid)
            out_path.parent.mkdir(parents=True, exist_ok=True)
            out_path.write_bytes(VALID_BODY)
            return driver.collector.DLOutcome(replayid, 200, len(VALID_BODY), None, 0, "direct", 1, 1.0)

        monkeypatch.setattr(driver, "download_one", fake_download_one)

        manifest, returned = asyncio.run(driver.amain(resume_args(out_arg, manifest_arg)))

        assert attempted == ["from-manifest-A"]
        assert manifest["resume_summary"]["attempted"] == 1
        if manifest_arg is not None:
            assert returned == manifest_arg
        else:
            assert returned.name == "manifest_TEST.json"
        for stale in (out_a / "bodies" / "from-manifest-A.ttrm",):
            if stale.exists():
                stale.unlink()


def test_resume_cli_mismatch_fails_before_network_or_writes(tmp_path, monkeypatch):
    """The real CLI rejects a cross-wave manifest before any network or writes.

    Given: manifest from wave-a with --out pointing at wave-b.
    When: driver.main parses real CLI argv for --resume.
    Then: it fails with --out guidance, performs no fetches, creates no
    bodies dir under wave-b, and writes no downloads ledger there.
    """
    _selection, _discovery, driver = load_wave()
    out_a, out_b = write_wave_pair(tmp_path)
    attempted: list[str] = []

    async def tripwire_queue(*_args: object, **_kwargs: object) -> object:
        attempted.append("queued")
        raise AssertionError("no download queue after manifest mismatch")

    def tripwire_client(*_args: object, **_kwargs: object) -> object:
        raise AssertionError("no network client after manifest mismatch")

    monkeypatch.setattr(driver, "download_queue_concurrent", tripwire_queue)
    monkeypatch.setattr(driver, "make_direct_client", tripwire_client)
    monkeypatch.setattr(driver, "make_geonode_client", tripwire_client)

    with pytest.raises(ValueError, match="--out"):
        driver.main(["--resume", "--manifest", str(out_a / "_meta" / "manifest_TEST.json"),
                     "--out", str(out_b)])

    assert attempted == []
    assert not (out_b / "bodies").exists()
    assert not (out_b / "_meta" / "downloads.jsonl").exists()


def test_resume_prefers_valid_external_body_over_undersized_local(tmp_path, monkeypatch):
    """Resume reuses a valid explicit external body despite an undersized local stub.

    Given: wave out with records (solo,), an undersized local
    bodies/solo.ttrm, and an explicit external corpus holding a valid
    solo.ttrm.
    When: resume runs with existing_root=[corpus] and a fake fetcher.
    Then: solo is reused from the external corpus, never queued, and the
    local body is overwritten with the valid bytes.
    """
    _selection, _discovery, driver = load_wave()
    out = tmp_path / "wave"
    meta = out / "_meta"
    meta.mkdir(parents=True, exist_ok=True)
    manifest_path = meta / "manifest_TEST.json"
    manifest_path.write_text(json.dumps({"wave": "xxplus-u-ss-01", "unique_selected": 1}))
    with (meta / "records.jsonl").open("w") as handle:
        handle.write(json.dumps({"replayid": "solo"}) + "\n")
    bodies = out / "bodies"
    bodies.mkdir(parents=True, exist_ok=True)
    (bodies / "solo.ttrm").write_bytes(b"partial")
    corpus = tmp_path / "external-corpus"
    (corpus / "x").mkdir(parents=True)
    (corpus / "x" / "solo.ttrm").write_bytes(VALID_BODY)
    attempted: list[str] = []

    async def fake_download_one(_gate: object, _direct: object, _geo: object,
                                replayid: str, out_path: Path, _stop: object) -> object:
        attempted.append(replayid)
        out_path.parent.mkdir(parents=True, exist_ok=True)
        out_path.write_bytes(VALID_BODY)
        return driver.collector.DLOutcome(replayid, 200, len(VALID_BODY), None, 0, "direct", 1, 1.0)

    monkeypatch.setattr(driver, "download_one", fake_download_one)

    manifest, _ = asyncio.run(
        driver.amain(resume_args(out, manifest_path, existing_root=[corpus])))

    assert attempted == []
    assert (out / "bodies" / "solo.ttrm").read_bytes() == VALID_BODY
    assert manifest["resume_summary"]["reused_cross_corpus"] == 1
    assert manifest["resume_summary"]["reused_ids"] == ["solo"]


def test_resume_refetches_ledger_success_with_missing_body(tmp_path, monkeypatch):
    _selection, _discovery, driver = load_wave()
    out = tmp_path / "wave"
    meta = out / "_meta"
    meta.mkdir(parents=True, exist_ok=True)
    manifest_path = meta / "manifest_TEST.json"
    manifest_path.write_text(json.dumps({"wave": "xxplus-u-ss-01", "unique_selected": 2}))
    with (meta / "records.jsonl").open("w") as handle:
        for rid in ("ghost", "live"):
            handle.write(json.dumps({"replayid": rid}) + "\n")
    with (meta / "downloads.jsonl").open("w") as handle:
        handle.write(json.dumps({"replayid": "ghost", "status": 200}) + "\n")
    attempted: list[str] = []

    async def fake_download_one(_gate: object, _direct: object, _geo: object,
                                replayid: str, out_path: Path, _stop: object) -> object:
        attempted.append(replayid)
        out_path.parent.mkdir(parents=True, exist_ok=True)
        out_path.write_bytes(VALID_BODY)
        return driver.collector.DLOutcome(replayid, 200, len(VALID_BODY), None, 0, "direct", 1, 1.0)

    monkeypatch.setattr(driver, "download_one", fake_download_one)
    args = resume_args(out, manifest_path, existing_root=[tmp_path / "empty-corpus"])

    manifest, _ = asyncio.run(driver.amain(args))

    assert sorted(attempted) == ["ghost", "live"]
    assert manifest["resume_summary"]["ledger_missing_body"] == ["ghost"]
    assert manifest["resume_summary"]["attempted"] == 2
