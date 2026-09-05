from __future__ import annotations

import importlib.util
import types
import urllib.error
from pathlib import Path


def load_module():
    path = Path(__file__).resolve().parents[1] / "recovered" / "replay-collector" / "collect_rank_coverage.py"
    spec = importlib.util.spec_from_file_location("collect_rank_coverage", path)
    assert spec is not None
    module = importlib.util.module_from_spec(spec)
    assert spec.loader is not None
    spec.loader.exec_module(module)
    return module


def test_player_from_league_entry_filters_to_target_ranks():
    mod = load_module()
    ss_entry = {
        "_id": "player-a",
        "username": "Stacker",
        "country": "US",
        "league": {"rank": "ss", "tr": 25000.5, "bestrank": "u", "glicko": 1800, "rd": 70},
        "p": {"pri": 1, "sec": 2, "ter": 3},
    }
    x_entry = {**ss_entry, "_id": "player-b", "league": {"rank": "x"}}

    assert mod.player_from_league_entry(ss_entry)["rank"] == "ss"
    assert mod.player_from_league_entry(x_entry) is None


def test_output_path_routes_by_rank_player_and_replay_id(tmp_path):
    mod = load_module()

    path = mod.output_path_for(tmp_path, "s+", "player-a", "abc123")

    assert path == tmp_path / "s+" / "player-a" / "abc123.ttrm"


def test_replay_index_row_uses_manifest_schema():
    mod = load_module()
    player = {"_id": "player-a", "username": "Stacker", "country": "CA", "rank": "a+"}
    record = {
        "_id": "record-a",
        "replayid": "abc123",
        "ts": "2026-06-01T00:00:00Z",
        "gamemode": "league",
        "pb": False,
        "otherusers": [{"id": "opponent-a"}],
    }

    row = mod.replay_index_row(player, record, "a+/player-a/abc123.ttrm", rounds=3, events=42)

    assert row == {
        "replayid": "abc123",
        "record_id": "record-a",
        "player_id": "player-a",
        "rank": "a+",
        "ts": "2026-06-01T00:00:00Z",
        "gamemode": "league",
        "pb": False,
        "opponents": ["opponent-a"],
        "on_disk": True,
        "path": "a+/player-a/abc123.ttrm",
        "rounds": 3,
        "events": 42,
        "country": "CA",
        "username": "Stacker",
    }


def test_validate_ttrm_accepts_trainable_league_replay():
    mod = load_module()
    payload = {
        "id": "abc123",
        "gamemode": "league",
        "ts": "2026-06-01T00:00:00Z",
        "users": [{"_id": "p1"}, {"_id": "p2"}],
        "replay": {
            "rounds": [
                [
                    {"replay": {"events": [{"type": "start"}, {"type": "keydown", "data": {"key": "moveLeft"}}]}},
                    {"replay": {"events": [{"type": "start"}, {"type": "full"}]}},
                ]
            ]
        },
    }

    summary = mod.validate_ttrm_payload(payload)

    assert summary == {"rounds": 1, "events": 4}


def test_validate_ttrm_rejects_non_league_replay():
    mod = load_module()
    payload = {"gamemode": "40l", "replay": {"rounds": [[{"replay": {"events": [1]}}, {"replay": {"events": [1]}}]]}}

    try:
        mod.validate_ttrm_payload(payload)
    except ValueError as exc:
        assert "gamemode" in str(exc)
    else:
        raise AssertionError("expected non-league replay to be rejected")


def test_http_client_returns_status_zero_for_transport_error():
    mod = load_module()
    client = mod.HttpClient(None)

    def fail_open(_req, timeout):
        raise urllib.error.URLError("ssl eof")

    client.opener.open = fail_open

    status, body = client.get("https://example.invalid", timeout=1.0)

    assert status == 0
    assert b"ssl eof" in body


def test_inoue_proxy_is_used_when_no_direct_client_is_configured():
    mod = load_module()
    args = types.SimpleNamespace(
        out="unused",
        target=1,
        player_pool=1,
        records_limit=1,
        channel_delay=0.0,
        inoue_delay=0.0,
        channel_proxy=None,
        inoue_proxy="http://127.0.0.1:7897",
        allow_inoue_direct=False,
        session_id="test-session",
        seed=1,
    )
    collector = mod.CoverageCollector(args)

    class ProxyClient:
        def get(self, *_args, **_kwargs):
            return 200, b'{"gamemode":"league"}'

    collector.inoue_proxy = ProxyClient()

    body, row = collector.download_replay("abc123")

    assert body == b'{"gamemode":"league"}'
    assert row["via"] == "inoue:proxy"


def test_prepare_reconciles_valid_files_missing_from_manifest(tmp_path):
    mod = load_module()
    replay_dir = tmp_path / "s+" / "player-a"
    replay_dir.mkdir(parents=True)
    (replay_dir / "abc123.ttrm").write_text(
        '{"id":"abc123","gamemode":"league","ts":"2026-06-01T00:00:00Z",'
        '"users":[{"_id":"player-a","username":"Stacker","country":"US"}],'
        '"replay":{"rounds":[[{"replay":{"events":[1]}},{"replay":{"events":[1]}}]]}}'
    )
    args = types.SimpleNamespace(
        out=str(tmp_path),
        target=1,
        player_pool=1,
        records_limit=1,
        channel_delay=0.0,
        inoue_delay=0.0,
        channel_proxy=None,
        inoue_proxy="http://127.0.0.1:7897",
        allow_inoue_direct=False,
        session_id="test-session",
        seed=1,
    )
    collector = mod.CoverageCollector(args)

    collector.prepare()

    assert collector.seen_replay_ids == {"abc123"}
    assert len(collector.saved_rows["s+"]) == 1
    assert collector.saved_rows["s+"][0]["path"] == "s+/player-a/abc123.ttrm"


def test_prepare_preserves_existing_download_and_failure_ledgers(tmp_path):
    mod = load_module()
    meta = tmp_path / "_meta"
    meta.mkdir()
    (meta / "downloads.jsonl").write_text('{"replayid":"ok","status":200}\n')
    (meta / "reattempt_queue.jsonl").write_text('{"replayid":"retry","status":500}\n')
    args = types.SimpleNamespace(
        out=str(tmp_path),
        target=1,
        player_pool=1,
        records_limit=1,
        channel_delay=0.0,
        inoue_delay=0.0,
        channel_proxy=None,
        inoue_proxy="http://127.0.0.1:7897",
        allow_inoue_direct=False,
        session_id="test-session",
        seed=1,
    )
    collector = mod.CoverageCollector(args)

    collector.prepare()

    assert collector.download_rows == [{"replayid": "ok", "status": 200}]
    assert collector.fail_rows == [{"replayid": "retry", "status": 500}]


def test_collector_uses_configured_download_workers(tmp_path):
    mod = load_module()
    args = types.SimpleNamespace(
        out=str(tmp_path),
        target=1,
        player_pool=1,
        records_limit=1,
        channel_delay=0.0,
        inoue_delay=0.0,
        channel_proxy=None,
        inoue_proxy="http://127.0.0.1:7897",
        allow_inoue_direct=False,
        download_workers=11,
        session_id="test-session",
        seed=1,
    )

    collector = mod.CoverageCollector(args)

    assert collector.download_workers == 11


def test_geonode_proxy_urls_use_all_configured_ports_without_printing_secret():
    mod = load_module()
    env = {
        "GEONODE_PROXY_USER": "user",
        "GEONODE_PROXY_PASS": "pass",
        "GEONODE_PROXY_HOST": "proxy.example:9999",
    }

    urls = mod.geonode_proxy_urls(env, [9000, 9001, 9010])

    assert urls == [
        "http://user:pass@proxy.example:9000",
        "http://user:pass@proxy.example:9001",
        "http://user:pass@proxy.example:9010",
    ]


def test_download_retries_retryable_response_on_next_proxy_endpoint(tmp_path):
    mod = load_module()
    args = types.SimpleNamespace(
        out=str(tmp_path),
        target=1,
        player_pool=1,
        records_limit=1,
        channel_delay=0.0,
        inoue_delay=0.0,
        channel_proxy=None,
        inoue_proxy="http://127.0.0.1:7897",
        allow_inoue_direct=False,
        download_workers=1,
        download_attempts=3,
        use_geonode=False,
        geonode_ports="9000-9002",
        session_id="test-session",
        seed=1,
    )
    collector = mod.CoverageCollector(args)

    class RotatingProxy:
        def __init__(self):
            self.calls = 0

        def get_with_label(self, *_args, **_kwargs):
            self.calls += 1
            if self.calls == 1:
                return 0, b"blocked", "geonode:9000"
            return 200, b'{"gamemode":"league"}', "geonode:9001"

    collector.inoue_proxy = RotatingProxy()

    body, row = collector.download_replay("abc123")

    assert body == b'{"gamemode":"league"}'
    assert row["via"] == "inoue:geonode:9001"
    assert row["attempts"] == 2


def test_use_geonode_keeps_channel_on_configured_proxy_by_default(tmp_path):
    mod = load_module()
    args = types.SimpleNamespace(
        out=str(tmp_path),
        target=1,
        player_pool=1,
        records_limit=1,
        channel_delay=0.0,
        inoue_delay=0.0,
        channel_proxy="http://127.0.0.1:7897",
        inoue_proxy="http://127.0.0.1:7897",
        allow_inoue_direct=False,
        download_workers=1,
        download_attempts=3,
        use_geonode=True,
        channel_use_geonode=False,
        geonode_ports="9000-9001",
        session_id="test-session",
        seed=1,
    )
    collector = mod.CoverageCollector(args)

    assert isinstance(collector.channel, mod.HttpClient)
    assert collector.channel.label == "proxy"
    assert isinstance(collector.inoue_direct, mod.HttpClient)
    assert collector.inoue_direct.label == "direct"
    assert isinstance(collector.inoue_proxy, mod.RotatingHttpClient)


def test_fetch_records_retries_channel_429_before_burning_player(tmp_path):
    mod = load_module()
    args = types.SimpleNamespace(
        out=str(tmp_path),
        target=1,
        player_pool=1,
        records_limit=1,
        channel_delay=0.0,
        inoue_delay=0.0,
        channel_proxy=None,
        inoue_proxy=None,
        allow_inoue_direct=False,
        download_workers=8,
        download_attempts=1,
        channel_workers=2,
        channel_attempts=2,
        channel_retry_delay=0.0,
        use_geonode=False,
        channel_use_geonode=False,
        geonode_ports="9000-9001",
        session_id="test-session",
        seed=1,
    )
    collector = mod.CoverageCollector(args)

    class FlakyChannel:
        def __init__(self):
            self.calls = 0

        def get(self, *_args, **_kwargs):
            self.calls += 1
            if self.calls == 1:
                return 429, b'{"success":false,"error":"Rate limit exceeded!"}'
            return 200, b'{"success":true,"data":{"entries":[{"replayid":"fresh","gamemode":"league"}]}}'

    collector.channel = FlakyChannel()

    records = collector.fetch_records("player-a")

    assert records == [{"replayid": "fresh", "gamemode": "league"}]
    assert collector.channel.calls == 2
    assert collector.channel_workers == 2


def test_prepare_marks_expired_replay_ids_as_dead(tmp_path):
    mod = load_module()
    meta = tmp_path / "_meta"
    meta.mkdir()
    (meta / "reattempt_queue.jsonl").write_text('{"replayid":"expired","status":404,"err":"replay expired\\n"}\n')
    args = types.SimpleNamespace(
        out=str(tmp_path),
        target=1,
        player_pool=1,
        records_limit=2,
        channel_delay=0.0,
        inoue_delay=0.0,
        channel_proxy=None,
        inoue_proxy=None,
        allow_inoue_direct=False,
        download_workers=1,
        download_attempts=1,
        channel_workers=1,
        channel_attempts=1,
        channel_retry_delay=0.0,
        use_geonode=False,
        channel_use_geonode=False,
        geonode_ports="9000-9001",
        session_id="test-session",
        seed=1,
    )
    collector = mod.CoverageCollector(args)

    collector.prepare()

    assert "expired" in collector.dead_replay_ids


def test_collector_uses_configured_max_replays_per_player(tmp_path):
    mod = load_module()
    args = types.SimpleNamespace(
        out=str(tmp_path),
        target=1,
        player_pool=1,
        records_limit=1,
        channel_delay=0.0,
        inoue_delay=0.0,
        channel_proxy=None,
        inoue_proxy=None,
        allow_inoue_direct=False,
        download_workers=1,
        download_attempts=1,
        channel_workers=1,
        channel_attempts=1,
        channel_retry_delay=0.0,
        max_replays_per_player=4,
        use_geonode=False,
        channel_use_geonode=False,
        geonode_ports="9000-9001",
        session_id="test-session",
        seed=1,
    )

    collector = mod.CoverageCollector(args)

    assert collector.max_replays_per_player == 4
