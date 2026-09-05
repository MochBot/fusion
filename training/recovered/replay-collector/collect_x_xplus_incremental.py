#!/usr/bin/env python3
# /// script
# requires-python = ">=3.11"
# dependencies = ["httpx>=0.27"]
# ///
"""Pull newest league replays for a selected current-rank cohort."""
from __future__ import annotations

import argparse
import asyncio
import importlib
import json
import sys
import time
from datetime import UTC, datetime, timedelta
from pathlib import Path
from typing import Any

SCRIPTS_DIR = Path(__file__).resolve().parent
sys.path.insert(0, str(SCRIPTS_DIR))

collector = importlib.import_module("collect_x_xplus_replays")
DOWNLOAD_LOG_PATH = collector.DOWNLOAD_LOG_PATH
META_DIR = collector.META_DIR
PLAYERS_PATH = collector.PLAYERS_PATH
RANKS_PATH = collector.RANKS_PATH
REPLAY_INDEX_PATH = collector.REPLAY_INDEX_PATH
ROOT = collector.ROOT
ROTATING_PORTS = collector.ROTATING_PORTS
TETRA_LB = collector.TETRA_LB
TETRA_RECORDS = collector.TETRA_RECORDS
download_one = collector.download_one
fetch_json = collector.fetch_json
is_valid_replay_body = collector.is_valid_replay_body
make_direct_client = collector.make_direct_client
make_geonode_client = collector.make_geonode_client
output_path_for = collector.output_path_for

Json = dict[str, Any]
DEFAULT_DAYS = 3.0
RECORDS_CONCURRENCY = 8
DOWNLOAD_CONCURRENCY = 16
STAMP = time.strftime("%Y%m%dT%H%M%SZ", time.gmtime())
MANIFEST_PATH = META_DIR / f"incremental_{STAMP}.json"


def log(msg: str) -> None:
    print(f"[{time.strftime('%Y-%m-%d %H:%M:%S')}] {msg}", flush=True)


def parse_ranks(value: str) -> tuple[str, ...]:
    ranks = tuple(rank.strip().lower() for rank in value.split(",") if rank.strip())
    if not ranks or any(rank != "x+" for rank in ranks):
        raise argparse.ArgumentTypeError("this collector accepts only --ranks x+")
    return ranks


def parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Collect newest current-rank X+ league replays")
    parser.add_argument("--ranks", type=parse_ranks, default=("x+",))
    parser.add_argument("--days", type=float, default=DEFAULT_DAYS)
    parser.add_argument("--since", type=parse_since, default=None)
    parser.add_argument("--record-pages", type=int, default=10)
    parser.add_argument("--use-geonode", action="store_true")
    parser.add_argument("--geonode-stop-file", type=Path, default=META_DIR / "STOP_GEONODE")
    args = parser.parse_args(argv)
    if args.days <= 0:
        parser.error("--days must be positive")
    if args.record_pages < 1:
        parser.error("--record-pages must be at least 1")
    return args


def parse_ts(value: str) -> datetime | None:
    try:
        return datetime.fromisoformat(value.replace("Z", "+00:00"))
    except (TypeError, ValueError):
        return None


def parse_since(value: str) -> datetime:
    parsed = parse_ts(value)
    if parsed is None or parsed.tzinfo is None:
        raise argparse.ArgumentTypeError("--since must be an ISO-8601 timestamp with timezone")
    return parsed.astimezone(UTC)


def load_players(path: Path = PLAYERS_PATH) -> list[Json]:
    return [] if not path.exists() else [json.loads(line) for line in path.read_text().splitlines() if line.strip()]


def load_ranks(path: Path = RANKS_PATH) -> dict[str, Json]:
    if path.exists():
        return json.loads(path.read_text())
    return {}


def load_already_downloaded(root: Path = ROOT, log_path: Path = DOWNLOAD_LOG_PATH) -> set[str]:
    already: set[str] = set()
    if log_path.exists():
        for line in log_path.read_text().splitlines():
            if not line.strip():
                continue
            try:
                entry = json.loads(line)
            except json.JSONDecodeError:
                continue
            if entry.get("status") == 200 or entry.get("on_disk"):
                replayid = entry.get("replayid")
                if replayid:
                    already.add(str(replayid))
    already.update(path.stem for path in root.glob("**/*.ttrm"))
    return already


def select_players(players: list[Json], rank_map: dict[str, Json], ranks: tuple[str, ...]) -> list[Json]:
    selected: list[Json] = []
    for player in players:
        player_id = str(player["_id"])
        current_rank = str((rank_map.get(player_id) or {}).get("rank") or player.get("rank") or "").lower()
        if current_rank in ranks:
            selected.append(player)
    return selected


def select_live_xplus_players(candidates: list[Json], live_entries: list[Json]) -> list[Json]:
    by_id = {str(player["_id"]): player for player in candidates}
    selected: list[Json] = []
    for entry in live_entries:
        player_id = str(entry.get("_id") or "")
        league = entry.get("league") or {}
        if player_id and str(league.get("rank") or "").lower() == "x+":
            live_player = {key: value for key, value in entry.items() if key != "league"}
            selected.append({**by_id.get(player_id, {}), **live_player, "rank": "x+"})
    return selected


async def fetch_live_leaderboard(client) -> list[Json]:
    entries: list[Json] = []
    cursor: str | None = None
    while True:
        url = f"{TETRA_LB}?limit=100"
        if cursor:
            url += f"&after={cursor}"
        data = await fetch_json(client, url)
        if not data or not data.get("success"):
            raise RuntimeError("live leaderboard fetch failed")
        page = data.get("data", {}).get("entries") or []
        if not page:
            return entries
        entries.extend(page)
        last = page[-1].get("p")
        if not last:
            return entries
        cursor = f"{last['pri']}:{last['sec']}:{last['ter']}"


async def fetch_records_since(client, player_id: str, cutoff: datetime, max_pages: int) -> list[Json]:
    records: list[Json] = []
    cursor: str | None = None
    for page_number in range(1, max_pages + 1):
        url = TETRA_RECORDS.format(user=player_id) + "?limit=100"
        if cursor:
            url += f"&after={cursor}"
        data = await fetch_json(client, url)
        if not data or not data.get("success"):
            raise RuntimeError(f"records page {page_number} failed for {player_id}")
        entries = data.get("data", {}).get("entries") or []
        if not entries:
            return records
        records.extend(entry for entry in entries if not entry.get("stub") and entry.get("replayid"))
        timestamps = [parse_ts(entry.get("ts")) for entry in entries]
        if any(timestamp is not None and timestamp <= cutoff for timestamp in timestamps):
            return records
        last = entries[-1].get("p")
        if not last:
            return records
        cursor = f"{last['pri']}:{last['sec']}:{last['ter']}"
    raise RuntimeError(f"records page cap {max_pages} before cutoff for {player_id}")


def new_replay_refs(
    polled: tuple[str, str, list[Json]],
    cutoff: datetime,
    excluded: set[str],
) -> list[Json]:
    player_id, rank, records = polled
    if rank != "x+":
        return []
    refs: list[Json] = []
    for record in records:
        replayid = str(record.get("replayid") or "")
        timestamp = record.get("ts")
        parsed = parse_ts(timestamp) if isinstance(timestamp, str) else None
        if not replayid or parsed is None or parsed < cutoff or replayid in excluded:
            continue
        excluded.add(replayid)
        refs.append(
            {
                "replayid": replayid,
                "record_id": record.get("_id"),
                "player_id": player_id,
                "ts": timestamp,
                "gamemode": record.get("gamemode"),
                "pb": record.get("pb"),
                "opponents": [user.get("id") for user in (record.get("otherusers") or [])],
                "rank": rank,
            }
        )
    return refs


async def main(args: argparse.Namespace) -> None:
    since = getattr(args, "since", None)
    cutoff = since if since is not None else datetime.now(UTC) - timedelta(days=args.days)
    candidates = load_players(PLAYERS_PATH)
    already = load_already_downloaded(ROOT, DOWNLOAD_LOG_PATH)

    metadata_client = make_direct_client(session_id=f"incremental-{STAMP}")
    records_gate = asyncio.Semaphore(RECORDS_CONCURRENCY)

    async def poll(player: Json) -> tuple[str, str, list[Json]]:
        async with records_gate:
            records = await fetch_records_since(
                metadata_client, str(player["_id"]), cutoff, args.record_pages
            )
            return str(player["_id"]), "x+", records

    refs: list[Json] = []
    try:
        players = select_live_xplus_players(candidates, await fetch_live_leaderboard(metadata_client))
        log(f"Incremental X+ pull: players={len(players)} days={args.days} known={len(already)}")
        for future in asyncio.as_completed([poll(player) for player in players]):
            refs.extend(new_replay_refs(await future, cutoff, already))
    finally:
        await metadata_client.aclose()

    if not refs:
        MANIFEST_PATH.write_text(json.dumps({"window_days": args.days, "new": 0, "downloaded": 0}, indent=2))
        return

    direct_client = make_direct_client()
    geonode_clients = (
        {port: make_geonode_client(port) for port in ROTATING_PORTS}
        if args.use_geonode
        else None
    )
    download_gate = asyncio.Semaphore(DOWNLOAD_CONCURRENCY)

    async def grab(ref: Json):
        out = output_path_for(str(ref["replayid"]), "x+", str(ref["player_id"]))
        outcome = await download_one(
            download_gate,
            direct_client,
            geonode_clients,
            str(ref["replayid"]),
            out,
            args.geonode_stop_file,
        )
        if outcome.status == 200 and (
            not out.exists() or not is_valid_replay_body(out.read_bytes())
        ):
            out.unlink(missing_ok=True)
            outcome.status = None
            outcome.err = "invalid_body"
        return ref, outcome

    outcomes = []
    try:
        for future in asyncio.as_completed([grab(ref) for ref in refs]):
            outcomes.append(await future)
    finally:
        await direct_client.aclose()
        if geonode_clients is not None:
            for client in geonode_clients.values():
                await client.aclose()

    with REPLAY_INDEX_PATH.open("a") as index_handle, DOWNLOAD_LOG_PATH.open("a") as log_handle:
        for ref, outcome in outcomes:
            log_handle.write(json.dumps({
                "replayid": outcome.replayid,
                "status": outcome.status,
                "bytes": outcome.bytes_in,
                "err": outcome.err,
                "port": outcome.port,
                "via": outcome.via,
                "attempts": outcome.attempts,
                "incremental": STAMP,
            }) + "\n")
            if outcome.status == 200:
                index_handle.write(json.dumps(ref) + "\n")

    downloaded = [outcome.replayid for _ref, outcome in outcomes if outcome.status == 200]
    MANIFEST_PATH.write_text(json.dumps({
        "window_days": args.days,
        "cutoff": cutoff.isoformat(),
        "players_polled": len(players),
        "new_unique": len(refs),
        "downloaded_ok": len(downloaded),
        "downloaded_ids": downloaded,
    }, indent=2))


if __name__ == "__main__":
    asyncio.run(main(parse_args()))
