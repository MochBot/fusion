#!/usr/bin/env python3
from __future__ import annotations

import argparse
import collections
import concurrent.futures
import datetime as dt
import json
import os
import random
import threading
import time
import urllib.error
import urllib.parse
import urllib.request
import uuid
from pathlib import Path
from typing import Any

TARGET_RANKS = ("ss", "s+", "s", "s-", "a+", "a")
LOWER_BOUND_RANKS = {"a-", "b+", "b", "b-", "c+", "c", "c-", "d+", "d", "z"}
CHANNEL_BASE = "https://ch.tetr.io/api"
INOUE_REPLAY = "https://inoue.szy.lol/api/replay/{replayid}"
USER_AGENT = "mosaic-fusion-coaching-coverage/0.1 (rank coverage via Inoue relay)"
FUSION_ROOT = Path(__file__).resolve().parents[3]
DEFAULT_OUT = FUSION_ROOT / "data" / "replays-coverage-ssa"
DEFAULT_LOCAL_PROXY = "http://127.0.0.1:7897"
DEFAULT_GEONODE_PORTS = tuple(range(9000, 9011))

Json = dict[str, Any]


def utc_now_iso() -> str:
    return dt.datetime.now(dt.timezone.utc).isoformat()


def cursor_from_entry(entry: Json) -> str | None:
    cursor = entry.get("p")
    if isinstance(cursor, dict):
        return f"{cursor.get('pri')}:{cursor.get('sec')}:{cursor.get('ter')}"
    if cursor is None:
        return None
    return str(cursor)


def parse_ports(value: str) -> list[int]:
    ports: list[int] = []
    for chunk in value.split(","):
        part = chunk.strip()
        if not part:
            continue
        if "-" in part:
            start_raw, end_raw = part.split("-", 1)
            start = int(start_raw.strip())
            end = int(end_raw.strip())
            if end < start:
                raise ValueError(f"invalid descending port range: {part}")
            ports.extend(range(start, end + 1))
        else:
            ports.append(int(part))
    if not ports:
        raise ValueError("at least one proxy port is required")
    return ports


def geonode_proxy_urls(env: dict[str, str] | os._Environ[str], ports: list[int] | tuple[int, ...]) -> list[str]:
    user = (env.get("GEONODE_PROXY_USER") or "").strip()
    password = (env.get("GEONODE_PROXY_PASS") or "").strip()
    base = (env.get("GEONODE_PROXY_BASE") or "").strip()
    if not user or not password or not base:
        raise RuntimeError("GEONODE_PROXY_USER, GEONODE_PROXY_PASS, and GEONODE_PROXY_BASE must be set")
    if "://" in base:
        parsed = urllib.parse.urlparse(base)
        host = parsed.hostname or base
    else:
        host = base.split(":", 1)[0]
    quoted_user = urllib.parse.quote(user, safe="")
    quoted_password = urllib.parse.quote(password, safe="")
    return [f"http://{quoted_user}:{quoted_password}@{host}:{port}" for port in ports]


def player_from_league_entry(entry: Json) -> Json | None:
    league = entry.get("league")
    if not isinstance(league, dict):
        return None
    rank = str(league.get("rank") or "").lower()
    if rank not in TARGET_RANKS:
        return None
    player_id = entry.get("_id")
    if not player_id:
        return None
    return {
        "_id": player_id,
        "username": entry.get("username"),
        "country": entry.get("country"),
        "rank": rank,
        "tr": league.get("tr"),
        "bestrank": league.get("bestrank"),
        "glicko": league.get("glicko"),
        "rd": league.get("rd"),
        "p": entry.get("p"),
    }


def output_path_for(root: Path, rank: str, player_id: str, replayid: str) -> Path:
    return root / rank / player_id / f"{replayid}.ttrm"


def opponents_from_record(record: Json, player_id: str) -> list[str]:
    opponents: list[str] = []
    for other in record.get("otherusers") or []:
        if not isinstance(other, dict):
            continue
        opponent = other.get("id") or other.get("_id")
        if opponent and opponent != player_id:
            opponents.append(str(opponent))
    return opponents


def replay_index_row(
    player: Json,
    record: Json,
    relative_path: str,
    *,
    rounds: int,
    events: int,
) -> Json:
    player_id = str(player["_id"])
    return {
        "replayid": record.get("replayid"),
        "record_id": record.get("_id"),
        "player_id": player_id,
        "rank": player.get("rank"),
        "ts": record.get("ts"),
        "gamemode": record.get("gamemode"),
        "pb": record.get("pb", False),
        "opponents": opponents_from_record(record, player_id),
        "on_disk": True,
        "path": relative_path,
        "rounds": rounds,
        "events": events,
        "country": player.get("country"),
        "username": player.get("username"),
    }


def validate_ttrm_payload(payload: Json) -> dict[str, int]:
    if payload.get("gamemode") != "league":
        raise ValueError(f"gamemode is {payload.get('gamemode')!r}")
    replay = payload.get("replay")
    if not isinstance(replay, dict):
        raise ValueError("missing replay object")
    rounds = replay.get("rounds")
    if not isinstance(rounds, list) or not rounds:
        raise ValueError("missing replay.rounds")
    event_count = 0
    for round_data in rounds:
        if not isinstance(round_data, list) or len(round_data) < 2:
            raise ValueError("round entry is not a two-player list")
        for side in round_data[:2]:
            if not isinstance(side, dict):
                raise ValueError("round side is not an object")
            side_replay = side.get("replay")
            if not isinstance(side_replay, dict):
                raise ValueError("round side has no replay object")
            events = side_replay.get("events")
            if not isinstance(events, list) or not events:
                raise ValueError("round side has no events")
            event_count += len(events)
    return {"rounds": len(rounds), "events": event_count}


class HttpClient:
    def __init__(self, proxy: str | None, label: str | None = None):
        handlers = []
        if proxy:
            handlers.append(urllib.request.ProxyHandler({"http": proxy, "https": proxy}))
        self.opener = urllib.request.build_opener(*handlers)
        self.label = label or ("proxy" if proxy else "direct")

    def get(self, url: str, *, timeout: float, headers: dict[str, str] | None = None) -> tuple[int, bytes]:
        merged = {"User-Agent": USER_AGENT}
        if headers:
            merged.update(headers)
        req = urllib.request.Request(url, headers=merged)
        try:
            with self.opener.open(req, timeout=timeout) as response:
                return int(response.status), response.read()
        except urllib.error.HTTPError as exc:
            return int(exc.code), exc.read()
        except (urllib.error.URLError, TimeoutError, OSError) as exc:
            return 0, str(exc).encode("utf-8", "replace")

    def get_with_label(self, url: str, *, timeout: float, headers: dict[str, str] | None = None) -> tuple[int, bytes, str]:
        status, body = self.get(url, timeout=timeout, headers=headers)
        return status, body, self.label


class RotatingHttpClient:
    def __init__(self, proxies: list[str], label_prefix: str):
        if not proxies:
            raise ValueError("at least one proxy URL is required")
        self.clients: list[HttpClient] = []
        for proxy in proxies:
            parsed = urllib.parse.urlparse(proxy)
            label = f"{label_prefix}:{parsed.port}" if parsed.port else label_prefix
            self.clients.append(HttpClient(proxy, label))
        self._lock = threading.Lock()
        self._index = 0

    def _next_client(self) -> HttpClient:
        with self._lock:
            client = self.clients[self._index % len(self.clients)]
            self._index += 1
            return client

    def get(self, url: str, *, timeout: float, headers: dict[str, str] | None = None) -> tuple[int, bytes]:
        status, body, _label = self.get_with_label(url, timeout=timeout, headers=headers)
        return status, body

    def get_with_label(self, url: str, *, timeout: float, headers: dict[str, str] | None = None) -> tuple[int, bytes, str]:
        client = self._next_client()
        return client.get_with_label(url, timeout=timeout, headers=headers)


class CoverageCollector:
    def __init__(self, args: argparse.Namespace):
        self.root = Path(args.out)
        self.meta = self.root / "_meta"
        self.logs = self.root / "_logs"
        self.target = args.target
        self.player_pool = args.player_pool
        self.records_limit = args.records_limit
        self.channel_delay = args.channel_delay
        self.inoue_delay = args.inoue_delay
        self.download_workers = max(1, int(getattr(args, "download_workers", 1)))
        self.download_attempts = max(1, int(getattr(args, "download_attempts", 4)))
        self.channel_workers = max(1, int(getattr(args, "channel_workers", 12)))
        self.channel_attempts = max(1, int(getattr(args, "channel_attempts", 5)))
        self.channel_retry_delay = max(0.0, float(getattr(args, "channel_retry_delay", 0.15)))
        self.max_replays_per_player = max(1, int(getattr(args, "max_replays_per_player", 2)))
        self.channel_gate = threading.BoundedSemaphore(self.channel_workers)
        use_geonode = bool(getattr(args, "use_geonode", False))
        channel_use_geonode = bool(getattr(args, "channel_use_geonode", False))
        if use_geonode:
            ports = parse_ports(getattr(args, "geonode_ports", "9000-9010"))
            proxies = geonode_proxy_urls(os.environ, ports)
            self.channel = RotatingHttpClient(proxies, "geonode") if channel_use_geonode else HttpClient(args.channel_proxy)
            self.inoue_proxy = RotatingHttpClient(proxies, "geonode")
        else:
            self.channel = HttpClient(args.channel_proxy)
            self.inoue_proxy = HttpClient(args.inoue_proxy) if args.inoue_proxy else None
        self.inoue_direct = HttpClient(None) if args.allow_inoue_direct or (not self.inoue_proxy and not use_geonode) else None
        self.session_id = args.session_id or f"mosaic-coverage-ssa-{uuid.uuid4()}"
        self.random = random.Random(args.seed)
        self.players_by_rank: dict[str, list[Json]] = {rank: [] for rank in TARGET_RANKS}
        self.saved_rows: dict[str, list[Json]] = {rank: [] for rank in TARGET_RANKS}
        self.saved_players: dict[str, Json] = {}
        self.download_rows: list[Json] = []
        self.fail_rows: list[Json] = []
        self.seen_replay_ids: set[str] = set()
        self.dead_replay_ids: set[str] = set()

    def log(self, message: str) -> None:
        self.logs.mkdir(parents=True, exist_ok=True)
        line = f"[{time.strftime('%Y-%m-%d %H:%M:%S')}] {message}"
        print(line, flush=True)
        with (self.logs / "collector.log").open("a") as handle:
            handle.write(line + "\n")

    def prepare(self) -> None:
        self.meta.mkdir(parents=True, exist_ok=True)
        self.logs.mkdir(parents=True, exist_ok=True)
        for rank in TARGET_RANKS:
            (self.root / rank).mkdir(parents=True, exist_ok=True)
        index_path = self.meta / "replay_index.jsonl"
        if index_path.exists():
            for line in index_path.read_text().splitlines():
                if not line.strip():
                    continue
                row = json.loads(line)
                rank = row.get("rank")
                replayid = row.get("replayid")
                if row.get("on_disk") and rank in self.saved_rows and replayid:
                    self.saved_rows[rank].append(row)
                    self.seen_replay_ids.add(replayid)
                    player_id = row.get("player_id")
                    if player_id and player_id not in self.saved_players:
                        self.saved_players[str(player_id)] = {
                            "_id": str(player_id),
                            "username": row.get("username"),
                            "country": row.get("country"),
                            "rank": rank,
                            "tr": None,
                            "bestrank": None,
                            "glicko": None,
                            "rd": None,
                            "p": None,
                        }
        self.load_jsonl_rows(self.meta / "downloads.jsonl", self.download_rows)
        self.load_jsonl_rows(self.meta / "reattempt_queue.jsonl", self.fail_rows)
        for row in self.fail_rows:
            if self.is_permanent_replay_failure(row):
                replayid = row.get("replayid")
                if replayid:
                    self.dead_replay_ids.add(str(replayid))
        self.reconcile_existing_files()

    def load_jsonl_rows(self, path: Path, rows: list[Json]) -> None:
        if not path.exists():
            return
        for line in path.read_text().splitlines():
            if not line.strip():
                continue
            rows.append(json.loads(line))

    def is_permanent_replay_failure(self, row: Json) -> bool:
        status = row.get("status")
        err = str(row.get("err") or "").lower()
        return status == 404 or status == "404" or "replay expired" in err

    def player_from_payload(self, rank: str, player_id: str, payload: Json) -> Json:
        player = {
            "_id": player_id,
            "username": None,
            "country": None,
            "rank": rank,
            "tr": None,
            "bestrank": None,
            "glicko": None,
            "rd": None,
            "p": None,
        }
        users = payload.get("users")
        if isinstance(users, list):
            for user in users:
                if not isinstance(user, dict):
                    continue
                user_id = user.get("_id") or user.get("id")
                if user_id == player_id:
                    player["username"] = user.get("username")
                    player["country"] = user.get("country")
                    break
        return player

    def record_from_payload(self, player_id: str, replayid: str, payload: Json) -> Json:
        otherusers: list[Json] = []
        users = payload.get("users")
        if isinstance(users, list):
            for user in users:
                if not isinstance(user, dict):
                    continue
                user_id = user.get("_id") or user.get("id")
                if user_id and user_id != player_id:
                    otherusers.append({"id": user_id})
        return {
            "_id": None,
            "replayid": replayid,
            "ts": payload.get("ts"),
            "gamemode": payload.get("gamemode"),
            "pb": False,
            "otherusers": otherusers,
        }

    def reconcile_existing_files(self) -> None:
        reconciled = 0
        for rank in TARGET_RANKS:
            for path in sorted((self.root / rank).glob("*/*.ttrm")):
                replayid = path.stem
                if replayid in self.seen_replay_ids:
                    continue
                player_id = path.parent.name
                try:
                    payload = json.loads(path.read_text())
                    summary = validate_ttrm_payload(payload)
                except Exception as exc:
                    self.fail_rows.append({"rank": rank, "player_id": player_id, "replayid": replayid, "err": str(exc), "ts": utc_now_iso()})
                    continue
                player = self.player_from_payload(rank, player_id, payload)
                record = self.record_from_payload(player_id, replayid, payload)
                row = replay_index_row(
                    player,
                    record,
                    str(path.relative_to(self.root)),
                    rounds=summary["rounds"],
                    events=summary["events"],
                )
                self.saved_rows[rank].append(row)
                self.saved_players[player_id] = player
                self.seen_replay_ids.add(replayid)
                reconciled += 1
        if reconciled:
            self.log(f"reconciled on-disk replay files missing from meta count={reconciled}")

    def channel_json(self, path: str, params: dict[str, Any]) -> Json:
        query = urllib.parse.urlencode(params)
        url = f"{CHANNEL_BASE}{path}?{query}"
        last_status = 0
        last_body = b""
        for attempt in range(self.channel_attempts):
            with self.channel_gate:
                status, body = self.channel.get(
                    url,
                    timeout=60.0,
                    headers={"X-Session-ID": self.session_id},
                )
            time.sleep(self.channel_delay)
            last_status = status
            last_body = body
            if status == 200:
                payload = json.loads(body)
                if not payload.get("success"):
                    error_text = str(payload.get("error") or "").lower()
                    if "rate limit" in error_text and attempt + 1 < self.channel_attempts:
                        if self.channel_retry_delay:
                            time.sleep(self.channel_retry_delay * (attempt + 1))
                        continue
                    raise RuntimeError(f"Channel request failed: {payload!r}")
                data = payload.get("data")
                if not isinstance(data, dict):
                    raise RuntimeError("Channel response has no data object")
                return data
            if status not in {0, 429, 500, 502, 503, 504}:
                break
            if attempt + 1 < self.channel_attempts and self.channel_retry_delay:
                time.sleep(self.channel_retry_delay * (attempt + 1))
        raise RuntimeError(f"Channel HTTP {last_status}: {last_body[:160]!r}")

    def discover_players(self) -> None:
        if all(len(self.players_by_rank[rank]) >= self.player_pool for rank in TARGET_RANKS):
            return
        cursor: str | None = None
        page = 0
        seen: set[str] = set()
        while True:
            params: dict[str, Any] = {"limit": 100}
            if cursor:
                params["after"] = cursor
            data = self.channel_json("/users/by/league", params)
            entries = data.get("entries") or []
            if not entries:
                break
            page += 1
            last_rank = None
            for entry in entries:
                if not isinstance(entry, dict):
                    continue
                last_rank = (entry.get("league") or {}).get("rank") or last_rank
                player = player_from_league_entry(entry)
                if not player:
                    continue
                rank = player["rank"]
                player_id = player["_id"]
                if player_id in seen or len(self.players_by_rank[rank]) >= self.player_pool:
                    continue
                seen.add(player_id)
                self.players_by_rank[rank].append(player)
            counts = {rank: len(self.players_by_rank[rank]) for rank in TARGET_RANKS}
            self.log(f"leaderboard page={page} last_rank={last_rank} pools={counts}")
            if all(counts[rank] >= self.player_pool for rank in TARGET_RANKS):
                return
            if all(counts[rank] >= self.target for rank in TARGET_RANKS) and last_rank in LOWER_BOUND_RANKS:
                return
            cursor = cursor_from_entry(entries[-1])
            if not cursor:
                return

    def fetch_records(self, player_id: str) -> list[Json]:
        data = self.channel_json(
            f"/users/{urllib.parse.quote(player_id)}/records/league/recent",
            {"limit": self.records_limit},
        )
        records = data.get("entries") or []
        return [record for record in records if isinstance(record, dict)]

    def download_replay(self, replayid: str) -> tuple[bytes | None, Json]:
        url = INOUE_REPLAY.format(replayid=urllib.parse.quote(replayid))
        attempts: list[tuple[str, int, bytes]] = []
        if self.inoue_proxy is not None:
            for _attempt in range(self.download_attempts):
                if hasattr(self.inoue_proxy, "get_with_label"):
                    status, body, label = self.inoue_proxy.get_with_label(
                        url, timeout=45.0, headers={"Accept": "application/octet-stream"}
                    )
                else:
                    status, body = self.inoue_proxy.get(
                        url, timeout=45.0, headers={"Accept": "application/octet-stream"}
                    )
                    label = "proxy"
                attempts.append((label, status, body))
                if status == 200:
                    if self.inoue_delay:
                        time.sleep(self.inoue_delay)
                    return body, {
                        "replayid": replayid,
                        "status": status,
                        "bytes": len(body),
                        "err": None,
                        "via": f"inoue:{label}",
                        "attempts": len(attempts),
                        "ts": utc_now_iso(),
                    }
                if status not in {0, 429, 500, 502, 503, 504}:
                    break
                if self.inoue_delay:
                    time.sleep(self.inoue_delay)
        if self.inoue_direct is not None:
            status, body, label = self.inoue_direct.get_with_label(
                url, timeout=45.0, headers={"Accept": "application/octet-stream"}
            )
            attempts.append((label, status, body))
            if status == 200:
                return body, {
                    "replayid": replayid,
                    "status": status,
                    "bytes": len(body),
                    "err": None,
                    "via": f"inoue:{label}",
                    "attempts": len(attempts),
                    "ts": utc_now_iso(),
                }
        if not attempts:
            return None, {
                "replayid": replayid,
                "status": 0,
                "bytes": 0,
                "err": "no inoue client configured",
                "via": "inoue:none",
                "attempts": 0,
                "ts": utc_now_iso(),
            }
        label, status, body = attempts[-1]
        return None, {
            "replayid": replayid,
            "status": status,
            "bytes": 0,
            "err": body[:240].decode("utf-8", "replace"),
            "via": f"inoue:{label}",
            "attempts": len(attempts),
            "ts": utc_now_iso(),
        }

    def save_replay(self, player: Json, record: Json, body: bytes, summary: dict[str, int]) -> None:
        rank = str(player["rank"])
        replayid = str(record["replayid"])
        out = output_path_for(self.root, rank, str(player["_id"]), replayid)
        out.parent.mkdir(parents=True, exist_ok=True)
        tmp = out.with_suffix(out.suffix + ".tmp")
        tmp.write_bytes(body)
        tmp.replace(out)
        row = replay_index_row(
            player,
            record,
            str(out.relative_to(self.root)),
            rounds=summary["rounds"],
            events=summary["events"],
        )
        self.saved_rows[rank].append(row)
        self.saved_players[str(player["_id"])] = player
        self.seen_replay_ids.add(replayid)
        self.log(f"saved rank={rank} count={len(self.saved_rows[rank])} replayid={replayid}")
        self.write_meta()

    def collect_player_candidate(
        self, rank: str, player: Json
    ) -> tuple[tuple[Json, Json, bytes, dict[str, int]] | None, list[Json], list[Json]]:
        download_rows: list[Json] = []
        fail_rows: list[Json] = []
        player_id = str(player["_id"])
        try:
            records = self.fetch_records(player_id)
        except Exception as exc:
            fail_rows.append({"rank": rank, "player_id": player_id, "err": str(exc), "ts": utc_now_iso()})
            return None, download_rows, fail_rows
        for record in records:
            replayid = record.get("replayid")
            if not replayid or replayid in self.seen_replay_ids or str(replayid) in self.dead_replay_ids:
                continue
            if record.get("gamemode") not in (None, "league"):
                continue
            body, download_row = self.download_replay(str(replayid))
            download_rows.append(download_row)
            if body is None:
                fail_row = {"rank": rank, "player_id": player_id, **download_row}
                fail_rows.append(fail_row)
                if self.is_permanent_replay_failure(fail_row):
                    self.dead_replay_ids.add(str(replayid))
                continue
            try:
                payload = json.loads(body.decode("utf-8"))
                summary = validate_ttrm_payload(payload)
            except Exception as exc:
                fail_rows.append({"rank": rank, "player_id": player_id, "replayid": replayid, "err": str(exc), "ts": utc_now_iso()})
                continue
            return (player, record, body, summary), download_rows, fail_rows
        return None, download_rows, fail_rows

    def collect_rank(self, rank: str) -> None:
        players = list(self.players_by_rank[rank])
        self.random.shuffle(players)
        per_player = collections.Counter(row["player_id"] for row in self.saved_rows[rank])
        for pass_limit in range(1, self.max_replays_per_player + 1):
            pending = [player for player in players if per_player[str(player["_id"])] < pass_limit]
            batch_size = max(self.download_workers * 4, self.target - len(self.saved_rows[rank]))
            for start in range(0, len(pending), batch_size):
                if len(self.saved_rows[rank]) >= self.target:
                    return
                batch = pending[start:start + batch_size]
                with concurrent.futures.ThreadPoolExecutor(max_workers=self.download_workers) as executor:
                    futures = [executor.submit(self.collect_player_candidate, rank, player) for player in batch]
                    for future in concurrent.futures.as_completed(futures):
                        try:
                            result, download_rows, fail_rows = future.result()
                        except Exception as exc:
                            self.fail_rows.append({"rank": rank, "err": str(exc), "ts": utc_now_iso()})
                            continue
                        self.download_rows.extend(download_rows)
                        self.fail_rows.extend(fail_rows)
                        if result is None:
                            continue
                        player, record, body, summary = result
                        replayid = str(record["replayid"])
                        if replayid in self.seen_replay_ids:
                            continue
                        self.save_replay(player, record, body, summary)
                        per_player[str(player["_id"])] += 1
                        if len(self.saved_rows[rank]) >= self.target:
                            for pending_future in futures:
                                pending_future.cancel()
                            return
                self.write_meta()
        raise RuntimeError(f"rank {rank} collected {len(self.saved_rows[rank])}, target {self.target}")

    def state(self) -> Json:
        replay_counts = {rank: len(self.saved_rows[rank]) for rank in TARGET_RANKS}
        distinct_players = {rank: len({row["player_id"] for row in self.saved_rows[rank]}) for rank in TARGET_RANKS}
        country_histogram = {
            rank: dict(collections.Counter(row.get("country") or "??" for row in self.saved_rows[rank]))
            for rank in TARGET_RANKS
        }
        date_histogram = {
            rank: dict(collections.Counter(str(row.get("ts") or "")[:10] for row in self.saved_rows[rank]))
            for rank in TARGET_RANKS
        }
        return {
            "updated_at": utc_now_iso(),
            "source": "TETRA CHANNEL metadata + Inoue replay relay",
            "replay_body_endpoint": INOUE_REPLAY,
            "target_per_rank": self.target,
            "rank_order": list(TARGET_RANKS),
            "on_disk_rank_counts": replay_counts,
            "distinct_players_by_rank": distinct_players,
            "country_histogram": country_histogram,
            "date_histogram": date_histogram,
            "candidate_players_by_rank": {rank: len(self.players_by_rank[rank]) for rank in TARGET_RANKS},
            "downloads_logged": len(self.download_rows),
            "failures_logged": len(self.fail_rows),
            "session_id": self.session_id,
        }

    def write_meta(self) -> None:
        self.meta.mkdir(parents=True, exist_ok=True)
        with (self.meta / "players.jsonl").open("w") as handle:
            for player in sorted(self.saved_players.values(), key=lambda item: (item["rank"], item["_id"])):
                handle.write(json.dumps(player, sort_keys=True) + "\n")
        with (self.meta / "replay_index.jsonl").open("w") as handle:
            rows = [row for rank in TARGET_RANKS for row in self.saved_rows[rank]]
            for row in sorted(rows, key=lambda item: (item["rank"], item["player_id"], item["replayid"])):
                handle.write(json.dumps(row, sort_keys=True) + "\n")
        with (self.meta / "downloads.jsonl").open("w") as handle:
            for row in self.download_rows:
                handle.write(json.dumps(row, sort_keys=True) + "\n")
        with (self.meta / "reattempt_queue.jsonl").open("w") as handle:
            for row in self.fail_rows:
                handle.write(json.dumps(row, sort_keys=True) + "\n")
        ranks = {rank: sorted({row["player_id"] for row in self.saved_rows[rank]}) for rank in TARGET_RANKS}
        (self.meta / "ranks.json").write_text(json.dumps(ranks, indent=2, sort_keys=True) + "\n")
        (self.meta / "state.json").write_text(json.dumps(self.state(), indent=2, sort_keys=True) + "\n")

    def run(self) -> None:
        self.prepare()
        self.discover_players()
        self.write_meta()
        for rank in TARGET_RANKS:
            if len(self.saved_rows[rank]) >= self.target:
                continue
            self.log(f"rank_start rank={rank} target={self.target}")
            self.collect_rank(rank)
            self.write_meta()
        self.write_meta()
        self.log(f"complete counts={self.state()['on_disk_rank_counts']}")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Collect SS-to-A TETR.IO league replay coverage via Inoue relay")
    parser.add_argument("--out", default=str(DEFAULT_OUT))
    parser.add_argument("--target", type=int, default=100)
    parser.add_argument("--player-pool", type=int, default=260)
    parser.add_argument("--records-limit", type=int, default=20)
    parser.add_argument("--channel-proxy", default=DEFAULT_LOCAL_PROXY)
    parser.add_argument("--inoue-proxy", default=DEFAULT_LOCAL_PROXY)
    parser.add_argument("--allow-inoue-direct", action="store_true")
    parser.add_argument("--use-geonode", action="store_true")
    parser.add_argument("--channel-use-geonode", action="store_true")
    parser.add_argument("--geonode-ports", default="9000-9010")
    parser.add_argument("--channel-delay", type=float, default=0.1)
    parser.add_argument("--inoue-delay", type=float, default=0.0)
    parser.add_argument("--download-workers", type=int, default=16)
    parser.add_argument("--download-attempts", type=int, default=4)
    parser.add_argument("--channel-workers", type=int, default=12)
    parser.add_argument("--channel-attempts", type=int, default=5)
    parser.add_argument("--channel-retry-delay", type=float, default=0.15)
    parser.add_argument("--max-replays-per-player", type=int, default=2)
    parser.add_argument("--session-id", default=None)
    parser.add_argument("--seed", type=int, default=20260608)
    return parser.parse_args()


def main() -> None:
    CoverageCollector(parse_args()).run()


if __name__ == "__main__":
    main()
