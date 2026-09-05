from __future__ import annotations

import concurrent.futures
import contextlib
import importlib.util
import sys
import threading
import types
import urllib.request
from argparse import Namespace
from pathlib import Path

import pytest


def load_module():
    path = Path(__file__).resolve().parents[1] / "recovered" / "replay-collector" / "collect_rank_coverage.py"
    spec = importlib.util.spec_from_file_location("collect_rank_coverage_safety", path)
    assert spec is not None
    module = importlib.util.module_from_spec(spec)
    assert spec.loader is not None
    spec.loader.exec_module(module)
    return module


def collector_args(tmp_path: Path, **overrides) -> Namespace:
    values = {
        "out": str(tmp_path),
        "target": 100,
        "rank_targets": None,
        "player_pool": 10,
        "records_limit": 5,
        "channel_delay": 0.0,
        "inoue_delay": 0.0,
        "channel_proxy": None,
        "inoue_proxy": None,
        "allow_inoue_direct": False,
        "download_workers": 2,
        "download_attempts": 2,
        "channel_workers": 1,
        "channel_attempts": 1,
        "channel_retry_delay": 0.0,
        "max_replays_per_player": 1,
        "use_geonode": False,
        "channel_use_geonode": False,
        "geonode_ports": "9000-9001",
        "geonode_stop_file": str(tmp_path / "STOP_GEONODE"),
        "session_id": "test-session",
        "seed": 1,
    }
    values.update(overrides)
    return Namespace(**values)


def league_entry(player_id: str, rank: str, position: int) -> dict[str, object]:
    return {
        "_id": player_id,
        "username": player_id,
        "league": {"rank": rank, "tr": 20_000 - position},
        "p": {"pri": position, "sec": position, "ter": position},
    }


def valid_replay() -> bytes:
    return (
        b'{"gamemode":"league","replay":{"rounds":'
        b'[[{"replay":{"events":[1]}},{"replay":{"events":[1]}}]]}}'
    )


def test_cli_defaults_use_direct_channel_and_replay_clients(monkeypatch):
    mod = load_module()
    monkeypatch.setattr(sys, "argv", ["collect_rank_coverage.py"])

    args = mod.parse_args()

    assert args.channel_proxy is None
    assert args.inoue_proxy is None


def test_capacity_preflight_fails_before_discovery_and_accounts_for_saved_rows(tmp_path):
    mod = load_module()
    collector = mod.CoverageCollector(
        collector_args(
            tmp_path,
            rank_targets={"ss": 4},
            player_pool=2,
            max_replays_per_player=1,
        )
    )
    collector.saved_rows["ss"].append({"player_id": "existing"})
    discovery_called = False
    collector.prepare = lambda: None

    def discover() -> None:
        nonlocal discovery_called
        discovery_called = True

    collector.discover_players = discover

    with pytest.raises(RuntimeError, match=r"ss.*deficit 3.*capacity 2"):
        collector.run()

    assert discovery_called is False


def test_collect_rank_reserves_only_remaining_target_slots(tmp_path):
    mod = load_module()
    collector = mod.CoverageCollector(
        collector_args(tmp_path, rank_targets={"ss": 2}, download_workers=2)
    )
    collector.players_by_rank["ss"] = [
        {"_id": f"player-{index}", "rank": "ss"} for index in range(4)
    ]
    requested: list[str] = []

    def candidate(_rank: str, player: dict[str, object]):
        player_id = str(player["_id"])
        requested.append(player_id)
        replayid = f"replay-{player_id}"
        return (
            (player, {"replayid": replayid}, valid_replay(), {"rounds": 1, "events": 2}),
            [],
            [],
        )

    def save(player, record, _body, _summary) -> None:
        collector.saved_rows["ss"].append(
            {"player_id": player["_id"], "replayid": record["replayid"]}
        )
        collector.seen_replay_ids.add(str(record["replayid"]))

    collector.collect_player_candidate = candidate
    collector.save_replay = save
    collector.write_meta = lambda: None

    collector.collect_rank("ss")

    assert len(requested) == 2
    assert len(collector.saved_rows["ss"]) == 2


def test_coverage_geonode_stop_file_blocks_proxy_attempt(tmp_path):
    stop_file = tmp_path / "STOP_GEONODE"
    stop_file.touch()
    mod = load_module()
    collector = mod.CoverageCollector(
        collector_args(tmp_path, allow_inoue_direct=True, geonode_stop_file=str(stop_file))
    )
    proxy_calls = 0

    class DirectClient:
        def get_with_label(self, *_args, **_kwargs):
            return 503, b"unavailable", "direct"

    class ProxyClient:
        def get_with_label(self, *_args, **_kwargs):
            nonlocal proxy_calls
            proxy_calls += 1
            return 200, valid_replay(), "geonode:9000"

    collector.inoue_direct = DirectClient()
    collector.inoue_proxy = ProxyClient()

    body, row = collector.download_replay("abc123")

    assert body is None
    assert proxy_calls == 0
    assert row["err"] == "geonode_budget_stop"
    assert row["via"] == "geonode:stopped"


def test_coverage_http_client_rejects_oversized_body_before_unbounded_read():
    mod = load_module()
    client = mod.HttpClient(None)

    class Response:
        status = 200
        headers = {"Content-Length": str(mod.MAX_REPLAY_BODY_BYTES + 1)}

        def __enter__(self):
            return self

        def __exit__(self, *_args):
            return None

        def read(self, _limit):
            raise AssertionError("oversized response must not be read")

    client.opener.open = lambda *_args, **_kwargs: Response()

    assert client.get("https://example.invalid", timeout=1.0) == (413, b"response_too_large")


def test_explicit_rank_targets_skip_unrequested_discovery_and_collection(tmp_path):
    mod = load_module()
    targets = mod.parse_rank_targets("ss=2,s+=2,a+=1,a=1")
    collector = mod.CoverageCollector(collector_args(tmp_path, rank_targets=targets))
    pages = iter(
        [
            {
                "entries": [
                    league_entry("ss-player", "ss", 1),
                    league_entry("s-player", "s", 2),
                    league_entry("a-player", "a", 3),
                ]
            },
            {"entries": []},
        ]
    )
    collector.channel_json = lambda *_args, **_kwargs: next(pages)

    collector.discover_players()
    collected: list[str] = []
    collector.prepare = lambda: None
    collector.discover_players = lambda: None
    collector.write_meta = lambda: None
    collector.collect_rank = collected.append
    collector.run()

    assert collector.active_ranks == ("ss", "s+", "a+", "a")
    assert collector.players_by_rank["s"] == []
    assert collected == ["ss", "s+", "a+", "a"]
    assert collector.state()["target_by_rank"] == targets


def test_direct_http_client_ignores_ambient_proxy_environment(monkeypatch):
    mod = load_module()
    monkeypatch.setenv("HTTPS_PROXY", "http://ambient.invalid:9999")
    captured: list[urllib.request.BaseHandler] = []

    def build_opener(*handlers):
        captured.extend(handlers)
        return types.SimpleNamespace(handlers=[])

    monkeypatch.setattr(mod.urllib.request, "build_opener", build_opener)

    mod.HttpClient(None)

    proxy_handlers = [handler for handler in captured if isinstance(handler, urllib.request.ProxyHandler)]
    assert len(proxy_handlers) == 1
    assert vars(proxy_handlers[0])["proxies"] == {}


def test_replay_download_uses_direct_before_geonode_on_retryable_failure(tmp_path):
    mod = load_module()
    collector = mod.CoverageCollector(collector_args(tmp_path, allow_inoue_direct=True))
    calls: list[str] = []

    class DirectClient:
        def get_with_label(self, *_args, **_kwargs):
            calls.append("direct")
            return 503, b"unavailable", "direct"

    class ProxyClient:
        def get_with_label(self, *_args, **_kwargs):
            calls.append("geonode")
            return 200, valid_replay(), "geonode:9000"

    collector.inoue_direct = DirectClient()
    collector.inoue_proxy = ProxyClient()

    body, row = collector.download_replay("abc123")

    assert body == valid_replay()
    assert calls == ["direct", "geonode"]
    assert row["via"] == "inoue:geonode:9000"
    assert row["attempts"] == 2


def test_replay_download_does_not_use_geonode_after_permanent_direct_status(tmp_path):
    mod = load_module()
    collector = mod.CoverageCollector(collector_args(tmp_path, allow_inoue_direct=True))
    proxy_calls = 0

    class DirectClient:
        def get_with_label(self, *_args, **_kwargs):
            return 404, b"expired", "direct"

    class ProxyClient:
        def get_with_label(self, *_args, **_kwargs):
            nonlocal proxy_calls
            proxy_calls += 1
            return 200, valid_replay(), "geonode:9000"

    collector.inoue_direct = DirectClient()
    collector.inoue_proxy = ProxyClient()

    body, row = collector.download_replay("abc123")

    assert body is None
    assert proxy_calls == 0
    assert row["status"] == 404
    assert row["via"] == "inoue:direct"


def test_concurrent_candidates_claim_replay_before_body_request(tmp_path):
    mod = load_module()
    collector = mod.CoverageCollector(collector_args(tmp_path))
    record = {"replayid": "same-replay", "gamemode": "league"}
    collector.fetch_records = lambda _player_id: [record]
    first_started = threading.Event()
    release_first = threading.Event()
    calls = 0

    def download(_replayid: str):
        nonlocal calls
        calls += 1
        if calls == 1:
            first_started.set()
            assert release_first.wait(timeout=2.0)
        return valid_replay(), {"replayid": "same-replay", "status": 200}

    collector.download_replay = download
    first_player = {"_id": "player-a", "rank": "ss"}
    second_player = {"_id": "player-b", "rank": "ss"}

    with concurrent.futures.ThreadPoolExecutor(max_workers=2) as executor:
        first = executor.submit(collector.collect_player_candidate, "ss", first_player)
        assert first_started.wait(timeout=2.0)
        second = executor.submit(collector.collect_player_candidate, "ss", second_player)
        second_result = second.result(timeout=2.0)
        release_first.set()
        first_result = first.result(timeout=2.0)

    assert first_result[0] is not None
    assert second_result[0] is None
    assert calls == 1


def test_failed_candidate_releases_replay_claim(tmp_path):
    mod = load_module()
    collector = mod.CoverageCollector(collector_args(tmp_path))
    collector.fetch_records = lambda _player_id: [{"replayid": "retry-me", "gamemode": "league"}]
    calls = 0

    def download(_replayid: str):
        nonlocal calls
        calls += 1
        return None, {"replayid": "retry-me", "status": 503, "err": "unavailable"}

    collector.download_replay = download

    first = collector.collect_player_candidate("ss", {"_id": "player-a", "rank": "ss"})
    second = collector.collect_player_candidate("ss", {"_id": "player-b", "rank": "ss"})

    assert first[0] is None
    assert second[0] is None
    assert calls == 2


def test_exception_during_download_releases_replay_claim(tmp_path):
    mod = load_module()
    collector = mod.CoverageCollector(collector_args(tmp_path))
    collector.fetch_records = lambda _player_id: [{"replayid": "retry-me", "gamemode": "league"}]

    def fail_download(_replayid: str):
        raise RuntimeError("transport exploded")

    collector.download_replay = fail_download

    with contextlib.suppress(RuntimeError):
        collector.collect_player_candidate("ss", {"_id": "player-a", "rank": "ss"})

    assert collector.claimed_replay_ids == set()


def test_save_replay_failure_releases_replay_claim(tmp_path):
    mod = load_module()
    collector = mod.CoverageCollector(collector_args(tmp_path))
    collector.players_by_rank["ss"] = [{"_id": "player-a", "rank": "ss"}]
    collector.fetch_records = lambda _player_id: [{"replayid": "persist-me", "gamemode": "league"}]
    collector.download_replay = lambda _replayid: (valid_replay(), {"replayid": "persist-me", "status": 200})
    collector.write_meta = lambda: None

    def fail_save(_player, _record, _body, _summary) -> None:
        raise OSError("disk full")

    collector.save_replay = fail_save

    with pytest.raises(OSError, match="disk full"):
        collector.collect_rank("ss")

    assert collector.claimed_replay_ids == set()
