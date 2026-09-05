from __future__ import annotations

import argparse
import asyncio
import datetime as dt
import importlib.util
import json
import sys
import types
from collections.abc import AsyncIterator
from pathlib import Path

import httpx
import pytest
from typing_extensions import override

COLLECTOR_DIR = Path(__file__).resolve().parents[1] / "recovered" / "replay-collector"


def load_module(path: Path, name: str):
    spec = importlib.util.spec_from_file_location(name, path)
    assert spec is not None
    module = importlib.util.module_from_spec(spec)
    assert spec.loader is not None
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


def load_collectors(monkeypatch):
    xplus = load_module(COLLECTOR_DIR / "collect_x_xplus_replays.py", "collect_x_xplus_replays")
    monkeypatch.setattr(sys, "argv", ["collect_x_xplus_incremental.py"])
    incremental = load_module(
        COLLECTOR_DIR / "collect_x_xplus_incremental.py",
        "collect_x_xplus_incremental_test",
    )
    return xplus, incremental


def valid_replay_body() -> bytes:
    return (
        b'{"gamemode":"league","replay":{"rounds":'
        b'[[{"replay":{"events":[1]}},{"replay":{"events":[1]}}]]}}'
    )


def test_adaptive_admission_applies_lower_and_higher_limits(monkeypatch):
    xplus, _incremental = load_collectors(monkeypatch)

    async def exercise() -> None:
        admission = xplus.AdaptiveAdmission(2)
        await admission.acquire()
        await admission.acquire()

        await admission.set_limit(1)
        waiting = asyncio.create_task(admission.acquire())
        await asyncio.sleep(0)
        assert not waiting.done()

        await admission.release()
        await asyncio.sleep(0)
        assert not waiting.done()

        await admission.set_limit(2)
        await asyncio.wait_for(waiting, timeout=1.0)
        await admission.release()
        await admission.release()

    asyncio.run(exercise())


def test_xplus_direct_client_ignores_ambient_proxy_environment(monkeypatch):
    xplus, _incremental = load_collectors(monkeypatch)
    captured: dict[str, object] = {}

    class Client:
        def __init__(self, **kwargs):
            captured.update(kwargs)

    monkeypatch.setattr(xplus.httpx, "AsyncClient", Client)

    xplus.make_direct_client(session_id="metadata")

    assert captured["trust_env"] is False
    assert "proxy" not in captured


def test_xplus_download_uses_direct_then_geonode_only_for_retryable_status(tmp_path, monkeypatch):
    xplus, _incremental = load_collectors(monkeypatch)
    calls: list[str] = []

    def direct_response(_request: httpx.Request) -> httpx.Response:
        calls.append("direct")
        return httpx.Response(503, content=b"unavailable")

    def proxy_response(_request: httpx.Request) -> httpx.Response:
        calls.append("geonode")
        return httpx.Response(200, content=valid_replay_body())

    async def run_download():
        direct = httpx.AsyncClient(transport=httpx.MockTransport(direct_response), trust_env=False)
        proxy = httpx.AsyncClient(transport=httpx.MockTransport(proxy_response), trust_env=False)
        try:
            return await xplus.download_one(
                asyncio.Semaphore(1),
                direct,
                {9000: proxy},
                "abc123",
                tmp_path / "abc123.ttrm",
            )
        finally:
            await direct.aclose()
            await proxy.aclose()

    outcome = asyncio.run(run_download())

    assert calls == ["direct", "geonode"]
    assert outcome.status == 200
    assert outcome.via == "geonode:9000"
    assert outcome.attempts == 2


def test_xplus_download_does_not_use_geonode_for_permanent_direct_status(tmp_path, monkeypatch):
    xplus, _incremental = load_collectors(monkeypatch)
    proxy_calls = 0

    def direct_response(_request: httpx.Request) -> httpx.Response:
        return httpx.Response(404, content=b"expired")

    def proxy_response(_request: httpx.Request) -> httpx.Response:
        nonlocal proxy_calls
        proxy_calls += 1
        return httpx.Response(200, content=valid_replay_body())

    async def run_download():
        direct = httpx.AsyncClient(transport=httpx.MockTransport(direct_response), trust_env=False)
        proxy = httpx.AsyncClient(transport=httpx.MockTransport(proxy_response), trust_env=False)
        try:
            return await xplus.download_one(
                asyncio.Semaphore(1),
                direct,
                {9000: proxy},
                "abc123",
                tmp_path / "abc123.ttrm",
            )
        finally:
            await direct.aclose()
            await proxy.aclose()

    outcome = asyncio.run(run_download())

    assert outcome.status == 404
    assert outcome.via == "direct"
    assert proxy_calls == 0


def test_xplus_geonode_stop_file_blocks_proxy_attempt(tmp_path, monkeypatch):
    xplus, _incremental = load_collectors(monkeypatch)
    stop_file = tmp_path / "STOP_GEONODE"
    stop_file.touch()
    proxy_calls = 0

    def direct_response(_request: httpx.Request) -> httpx.Response:
        return httpx.Response(503, content=b"unavailable")

    def proxy_response(_request: httpx.Request) -> httpx.Response:
        nonlocal proxy_calls
        proxy_calls += 1
        return httpx.Response(200, content=valid_replay_body())

    async def run_download():
        direct = httpx.AsyncClient(transport=httpx.MockTransport(direct_response), trust_env=False)
        proxy = httpx.AsyncClient(transport=httpx.MockTransport(proxy_response), trust_env=False)
        try:
            return await xplus.download_one(
                asyncio.Semaphore(1), direct, {9000: proxy}, "abc123", tmp_path / "abc123.ttrm", stop_file
            )
        finally:
            await direct.aclose()
            await proxy.aclose()

    outcome = asyncio.run(run_download())

    assert proxy_calls == 0
    assert outcome.err == "geonode_budget_stop"
    assert outcome.via == "geonode:stopped"


def test_xplus_streamed_200_failure_does_not_survive_blocked_fallback(tmp_path, monkeypatch):
    xplus, _incremental = load_collectors(monkeypatch)
    stop_file = tmp_path / "STOP_GEONODE"
    stop_file.touch()
    out_path = tmp_path / "abc123.ttrm"
    proxy_calls = 0

    class FailingStream(httpx.AsyncByteStream):
        @override
        async def __aiter__(self) -> AsyncIterator[bytes]:
            yield b'{"partial":'
            raise httpx.RemoteProtocolError("stream interrupted")

    def direct_response(_request: httpx.Request) -> httpx.Response:
        return httpx.Response(200, stream=FailingStream())

    def proxy_response(_request: httpx.Request) -> httpx.Response:
        nonlocal proxy_calls
        proxy_calls += 1
        return httpx.Response(200, content=b"{}")

    async def no_sleep(_delay: float) -> None:
        return None

    async def run_download():
        direct = httpx.AsyncClient(transport=httpx.MockTransport(direct_response), trust_env=False)
        proxy = httpx.AsyncClient(transport=httpx.MockTransport(proxy_response), trust_env=False)
        try:
            return await xplus.download_one(
                asyncio.Semaphore(1), direct, {9000: proxy}, "abc123", out_path, stop_file
            )
        finally:
            await direct.aclose()
            await proxy.aclose()

    monkeypatch.setattr(xplus.asyncio, "sleep", no_sleep)
    outcome = asyncio.run(run_download())

    assert proxy_calls == 0
    assert outcome.status is None
    assert outcome.bytes_in == 0
    assert outcome.err == "geonode_budget_stop"
    assert not out_path.exists()


def test_xplus_download_rejects_oversized_body_without_writing_file(tmp_path, monkeypatch):
    xplus, _incremental = load_collectors(monkeypatch)
    out_path = tmp_path / "abc123.ttrm"

    def response(_request: httpx.Request) -> httpx.Response:
        return httpx.Response(200, content=b"{" + b"x" * xplus.MAX_REPLAY_BODY_BYTES)

    async def run_download():
        direct = httpx.AsyncClient(transport=httpx.MockTransport(response), trust_env=False)
        try:
            return await xplus.download_one(
                asyncio.Semaphore(1), direct, None, "abc123", out_path
            )
        finally:
            await direct.aclose()

    outcome = asyncio.run(run_download())

    assert outcome.err == "response_too_large"
    assert outcome.bytes_in == xplus.MAX_REPLAY_BODY_BYTES + 1
    assert not out_path.exists()


def test_xplus_repeated_200_invalid_json_returns_non_success_without_file(tmp_path, monkeypatch):
    xplus, _incremental = load_collectors(monkeypatch)
    out_path = tmp_path / "abc123.ttrm"

    def response(_request: httpx.Request) -> httpx.Response:
        return httpx.Response(200, content=b"<html>not a replay</html>")

    async def no_sleep(_delay: float) -> None:
        return None

    async def run_download():
        direct = httpx.AsyncClient(transport=httpx.MockTransport(response), trust_env=False)
        try:
            return await xplus.download_one(
                asyncio.Semaphore(1), direct, None, "abc123", out_path
            )
        finally:
            await direct.aclose()

    monkeypatch.setattr(xplus.asyncio, "sleep", no_sleep)
    outcome = asyncio.run(run_download())

    assert outcome.status != 200
    assert outcome.err == "invalid_body"
    assert not out_path.exists()


def test_xplus_repeated_200_truncated_json_returns_non_success_without_file(tmp_path, monkeypatch):
    xplus, _incremental = load_collectors(monkeypatch)
    out_path = tmp_path / "abc123.ttrm"

    def response(_request: httpx.Request) -> httpx.Response:
        return httpx.Response(200, content=b'{"gamemode":"league","replay":')

    async def no_sleep(_delay: float) -> None:
        return None

    async def run_download():
        direct = httpx.AsyncClient(transport=httpx.MockTransport(response), trust_env=False)
        try:
            return await xplus.download_one(
                asyncio.Semaphore(1), direct, None, "abc123", out_path
            )
        finally:
            await direct.aclose()

    monkeypatch.setattr(xplus.asyncio, "sleep", no_sleep)
    outcome = asyncio.run(run_download())

    assert outcome.status != 200
    assert outcome.err == "invalid_body"
    assert not out_path.exists()


def test_xplus_valid_league_body_writes_file_and_returns_200(tmp_path, monkeypatch):
    xplus, _incremental = load_collectors(monkeypatch)
    out_path = tmp_path / "abc123.ttrm"

    def response(_request: httpx.Request) -> httpx.Response:
        return httpx.Response(200, content=valid_replay_body())

    async def run_download():
        direct = httpx.AsyncClient(transport=httpx.MockTransport(response), trust_env=False)
        try:
            return await xplus.download_one(
                asyncio.Semaphore(1), direct, None, "abc123", out_path
            )
        finally:
            await direct.aclose()

    outcome = asyncio.run(run_download())

    assert outcome.status == 200
    assert outcome.err is None
    assert out_path.read_bytes() == valid_replay_body()


def test_incremental_cli_defaults_to_bounded_xplus_window(monkeypatch):
    _xplus, incremental = load_collectors(monkeypatch)

    defaults = incremental.parse_args([])
    explicit = incremental.parse_args(["--ranks", "x+", "--days", "5", "--record-pages", "2"])
    since = incremental.parse_args(["--since", "2026-07-01T00:00:00Z"])

    assert defaults.ranks == ("x+",)
    assert defaults.days == 3.0
    assert defaults.record_pages == 10
    assert explicit.ranks == ("x+",)
    assert explicit.days == 5.0
    assert explicit.record_pages == 2
    assert since.since == dt.datetime(2026, 7, 1, tzinfo=dt.UTC)


def test_live_xplus_cohort_uses_fresh_rank_for_promotion_and_demotion(monkeypatch):
    _xplus, incremental = load_collectors(monkeypatch)
    cached = [
        {"_id": "promoted", "rank": "x"},
        {"_id": "demoted", "rank": "x+"},
    ]
    live_entries = [
        {"_id": "promoted", "league": {"rank": "x+"}},
        {"_id": "demoted", "league": {"rank": "u"}},
    ]

    selected = incremental.select_live_xplus_players(cached, live_entries)

    assert [player["_id"] for player in selected] == ["promoted"]
    assert selected[0]["rank"] == "x+"


def test_live_xplus_cohort_includes_fresh_player_missing_from_cache(monkeypatch):
    _xplus, incremental = load_collectors(monkeypatch)

    selected = incremental.select_live_xplus_players(
        [], [{"_id": "fresh", "username": "Fresh", "league": {"rank": "x+"}}]
    )

    assert selected == [{"_id": "fresh", "username": "Fresh", "rank": "x+"}]


def test_incremental_main_uses_fresh_xplus_cohort_when_players_cache_is_missing(
    tmp_path, monkeypatch
):
    _xplus, incremental = load_collectors(monkeypatch)
    meta = tmp_path / "_meta"
    meta.mkdir()
    players_path = meta / "players.jsonl"
    selected_players: list[str] = []

    class Client:
        async def aclose(self) -> None:
            return None

    async def live_leaderboard(_client):
        return [{"_id": "fresh", "username": "Fresh", "league": {"rank": "x+"}}]

    async def records(_client, player_id: str, _cutoff, _max_pages: int):
        selected_players.append(player_id)
        return []

    monkeypatch.setattr(incremental, "ROOT", tmp_path)
    monkeypatch.setattr(incremental, "META_DIR", meta)
    monkeypatch.setattr(incremental, "PLAYERS_PATH", players_path)
    monkeypatch.setattr(incremental, "DOWNLOAD_LOG_PATH", meta / "downloads.jsonl")
    monkeypatch.setattr(incremental, "MANIFEST_PATH", meta / "incremental-test.json")
    monkeypatch.setattr(incremental, "make_direct_client", lambda **_kwargs: Client())
    monkeypatch.setattr(incremental, "fetch_live_leaderboard", live_leaderboard)
    monkeypatch.setattr(incremental, "fetch_records_since", records)
    args = argparse.Namespace(
        days=3.0,
        since=dt.datetime(2026, 7, 1, tzinfo=dt.UTC),
        ranks=("x+",),
        record_pages=2,
        use_geonode=False,
        geonode_stop_file=meta / "STOP_GEONODE",
    )

    asyncio.run(incremental.main(args))

    assert not players_path.exists()
    assert selected_players == ["fresh"]


def test_records_pagination_includes_second_page_and_stops_at_cutoff(monkeypatch):
    _xplus, incremental = load_collectors(monkeypatch)
    cutoff = dt.datetime(2026, 7, 1, tzinfo=dt.UTC)
    pages = iter(
        [
            {"success": True, "data": {"entries": [{"replayid": "new-1", "ts": "2026-07-03T00:00:00Z", "p": {"pri": 1, "sec": 2, "ter": 3}}]}},
            {"success": True, "data": {"entries": [{"replayid": "new-2", "ts": "2026-07-02T00:00:00Z", "p": {"pri": 4, "sec": 5, "ter": 6}}, {"replayid": "old", "ts": "2026-06-30T00:00:00Z", "p": {"pri": 7, "sec": 8, "ter": 9}}]}},
        ]
    )

    async def fetch(_client, _url):
        return next(pages)

    monkeypatch.setattr(incremental, "fetch_json", fetch)
    records = asyncio.run(incremental.fetch_records_since(object(), "player", cutoff, max_pages=3))

    assert [record["replayid"] for record in records] == ["new-1", "new-2", "old"]


def test_records_pagination_fails_closed_on_page_failure(monkeypatch):
    _xplus, incremental = load_collectors(monkeypatch)
    cutoff = dt.datetime(2026, 7, 1, tzinfo=dt.UTC)
    pages = iter([
        {"success": True, "data": {"entries": [{"replayid": "new", "ts": "2026-07-03T00:00:00Z", "p": {"pri": 1, "sec": 2, "ter": 3}}]}},
        None,
    ])

    async def fetch(_client, _url):
        return next(pages)

    monkeypatch.setattr(incremental, "fetch_json", fetch)

    with pytest.raises(RuntimeError, match="records page 2 failed"):
        asyncio.run(incremental.fetch_records_since(object(), "player", cutoff, max_pages=3))


def test_records_pagination_fails_closed_when_cap_precedes_cutoff(monkeypatch):
    _xplus, incremental = load_collectors(monkeypatch)
    cutoff = dt.datetime(2026, 7, 1, tzinfo=dt.UTC)

    async def fetch(_client, _url):
        return {"success": True, "data": {"entries": [{"replayid": "new", "ts": "2026-07-03T00:00:00Z", "p": {"pri": 1, "sec": 2, "ter": 3}}]}}

    monkeypatch.setattr(incremental, "fetch_json", fetch)

    with pytest.raises(RuntimeError, match="page cap 1 before cutoff"):
        asyncio.run(incremental.fetch_records_since(object(), "player", cutoff, max_pages=1))


def test_incremental_dedupe_reads_download_ledger_and_ttrm_files(tmp_path, monkeypatch):
    _xplus, incremental = load_collectors(monkeypatch)
    log_path = tmp_path / "_meta" / "downloads.jsonl"
    log_path.parent.mkdir()
    log_path.write_text('{"replayid":"logged","status":200}\n')
    replay_path = tmp_path / "x+" / "player-a" / "on-disk.ttrm"
    replay_path.parent.mkdir(parents=True)
    replay_path.write_bytes(b"{}")

    already = incremental.load_already_downloaded(tmp_path, log_path)

    assert already == {"logged", "on-disk"}


def test_incremental_filters_current_cohort_and_replay_refs_to_xplus(monkeypatch):
    _xplus, incremental = load_collectors(monkeypatch)
    now = dt.datetime.now(dt.UTC)
    players = [
        {"_id": "plus", "rank": "x+"},
        {"_id": "demoted", "rank": "x+"},
        {"_id": "x-player", "rank": "x"},
    ]
    rank_map = {
        "plus": {"rank": "x+"},
        "demoted": {"rank": "u"},
        "x-player": {"rank": "x"},
    }
    selected = incremental.select_players(players, rank_map, ("x+",))
    excluded = {"known"}
    records = [
        {"replayid": "known", "ts": now.isoformat(), "gamemode": "league"},
        {"replayid": "fresh", "ts": now.isoformat(), "gamemode": "league"},
        {"replayid": "fresh", "ts": now.isoformat(), "gamemode": "league"},
        {"replayid": "old", "ts": (now - dt.timedelta(days=10)).isoformat(), "gamemode": "league"},
    ]

    refs = incremental.new_replay_refs(
        ("plus", "x+", records),
        now - dt.timedelta(days=3),
        excluded,
    )
    x_refs = incremental.new_replay_refs(
        ("x-player", "x", [{"replayid": "never", "ts": now.isoformat()}]),
        now - dt.timedelta(days=3),
        excluded,
    )

    assert [player["_id"] for player in selected] == ["plus"]
    assert [ref["replayid"] for ref in refs] == ["fresh"]
    assert x_refs == []


def test_incremental_main_polls_only_xplus_and_rejects_invalid_200_body(tmp_path, monkeypatch):
    _xplus, incremental = load_collectors(monkeypatch)
    meta = tmp_path / "_meta"
    meta.mkdir()
    players_path = meta / "players.jsonl"
    ranks_path = meta / "ranks.json"
    replay_index_path = meta / "replay_index.jsonl"
    download_log_path = meta / "downloads.jsonl"
    players_path.write_text(
        '\n'.join(
            [
                '{"_id":"plus","rank":"x+"}',
                '{"_id":"x-player","rank":"x"}',
                '{"_id":"demoted","rank":"x+"}',
            ]
        )
        + "\n"
    )
    ranks_path.write_text('{"plus":{"rank":"x+"},"x-player":{"rank":"x"},"demoted":{"rank":"u"}}')
    download_log_path.write_text('{"replayid":"logged","status":200}\n')
    disk_path = tmp_path / "x+" / "plus" / "on-disk.ttrm"
    disk_path.parent.mkdir(parents=True)
    disk_path.write_bytes(b"{}")

    monkeypatch.setattr(incremental, "ROOT", tmp_path)
    monkeypatch.setattr(incremental, "META_DIR", meta)
    monkeypatch.setattr(incremental, "PLAYERS_PATH", players_path)
    monkeypatch.setattr(incremental, "RANKS_PATH", ranks_path)
    monkeypatch.setattr(incremental, "REPLAY_INDEX_PATH", replay_index_path)
    monkeypatch.setattr(incremental, "DOWNLOAD_LOG_PATH", download_log_path)
    monkeypatch.setattr(incremental, "MANIFEST_PATH", meta / "incremental-test.json", raising=False)

    class Client:
        async def aclose(self) -> None:
            return None

    monkeypatch.setattr(incremental, "make_direct_client", lambda **_kwargs: Client())
    poll_calls: list[str] = []
    now = dt.datetime.now(dt.UTC).isoformat()

    async def fetch_records(_client, player_id: str, _cutoff, max_pages: int):
        poll_calls.append(player_id)
        assert max_pages == 1
        return [
            {"replayid": "logged", "ts": now, "gamemode": "league"},
            {"replayid": "on-disk", "ts": now, "gamemode": "league"},
            {"replayid": "fresh", "ts": now, "gamemode": "league"},
            {"replayid": "fresh", "ts": now, "gamemode": "league"},
        ]

    monkeypatch.setattr(
        incremental,
        "fetch_live_leaderboard",
        lambda _client: asyncio.sleep(
            0,
            result=[
                {"_id": "plus", "league": {"rank": "x+"}},
                {"_id": "x-player", "league": {"rank": "x"}},
                {"_id": "demoted", "league": {"rank": "u"}},
            ],
        ),
    )
    monkeypatch.setattr(incremental, "fetch_records_since", fetch_records)
    body_requests: list[str] = []

    async def download(_sem, _direct, _geonode, replayid: str, out_path: Path, _stop_file):
        body_requests.append(replayid)
        out_path.parent.mkdir(parents=True, exist_ok=True)
        out_path.write_bytes(b"{}")
        return types.SimpleNamespace(
            replayid=replayid,
            status=200,
            bytes_in=2,
            err=None,
            port=0,
            via="direct",
            attempts=1,
            elapsed_ms=1.0,
        )

    monkeypatch.setattr(incremental, "download_one", download)
    monkeypatch.setattr(
        incremental,
        "output_path_for",
        lambda replayid, rank, player_id: tmp_path / rank / player_id / f"{replayid}.ttrm",
    )
    args = argparse.Namespace(
        days=3.0,
        ranks=("x+",),
        record_pages=1,
        use_geonode=False,
        geonode_stop_file=meta / "STOP_GEONODE",
    )

    asyncio.run(incremental.main(args))

    assert poll_calls == ["plus"]
    assert body_requests == ["fresh"]
    assert not (tmp_path / "x+" / "plus" / "fresh.ttrm").exists()
    assert replay_index_path.read_text() == ""
    assert json.loads((meta / "incremental-test.json").read_text())["downloaded_ok"] == 0
