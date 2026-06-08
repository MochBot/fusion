#!/usr/bin/env python3
# /// script
# requires-python = ">=3.11"
# dependencies = ["httpx>=0.27"]
# ///
"""Incremental X/X+ league replay pull: newest replays within the last N days,
keeping only NEW and UNIQUE ones (deduped against the existing corpus).

Unlike collect_x_xplus_replays.py (which is a discover-once pipeline that skips
already-indexed players on re-run), this re-polls the KNOWN X/X+ cohort
(_meta/players.jsonl) for fresh records, filters to a recent time window, drops
anything already downloaded, fetches the new .ttrm files into the same
{rank}/{player_id}/ layout, and appends to the same _meta ledgers so the corpus
stays consistent.

Run:  uv run training/scripts/collect_x_xplus_incremental.py [DAYS]
      (DAYS default 3.0)  Requires GEONODE_PROXY_* env (same as the daemon).
"""
from __future__ import annotations

import asyncio
import json
import sys
import time
from datetime import datetime, timedelta, timezone
from pathlib import Path

SCRIPTS_DIR = Path(__file__).resolve().parent
sys.path.insert(0, str(SCRIPTS_DIR))

from collect_x_xplus_replays import (  # noqa: E402
    DOWNLOAD_LOG_PATH,
    META_DIR,
    PLAYERS_PATH,
    RANKS_PATH,
    REPLAY_INDEX_PATH,
    ROTATING_PORTS,
    download_one,
    fetch_player_records_all_pages,
    make_client,
    output_path_for,
)

DAYS = float(sys.argv[1]) if len(sys.argv) > 1 else 3.0
RECORDS_CONCURRENCY = 8
DOWNLOAD_CONCURRENCY = 16
STAMP = time.strftime("%Y%m%dT%H%M%SZ", time.gmtime())
MANIFEST_PATH = META_DIR / f"incremental_{STAMP}.json"


def log(msg: str) -> None:
    print(f"[{time.strftime('%Y-%m-%d %H:%M:%S')}] {msg}", flush=True)


def parse_ts(s: str) -> datetime | None:
    try:
        return datetime.fromisoformat(s.replace("Z", "+00:00"))
    except Exception:
        return None


def load_players() -> list[dict]:
    players = []
    with open(PLAYERS_PATH) as f:
        for line in f:
            line = line.strip()
            if line:
                players.append(json.loads(line))
    return players


def load_ranks() -> dict[str, dict]:
    if RANKS_PATH.exists():
        return json.loads(RANKS_PATH.read_text())
    return {}


def load_already_downloaded() -> set[str]:
    already: set[str] = set()
    if DOWNLOAD_LOG_PATH.exists():
        for line in DOWNLOAD_LOG_PATH.read_text().splitlines():
            line = line.strip()
            if not line:
                continue
            try:
                entry = json.loads(line)
                if entry.get("status") == 200 or entry.get("on_disk"):
                    already.add(entry["replayid"])
            except Exception:
                continue
    return already


async def main() -> None:
    cutoff = datetime.now(timezone.utc) - timedelta(days=DAYS)
    log("=" * 70)
    log(f"Incremental X/X+ pull: window = last {DAYS} days (cutoff {cutoff.isoformat()})")
    players = load_players()
    ranks = load_ranks()
    already = load_already_downloaded()
    log(f"cohort: {len(players)} known X/X+ players | dedup set: {len(already):,} already-downloaded replays")

    # ---- Phase 1: re-poll each known player for recent records ----
    clients = {p: make_client(p) for p in ROTATING_PORTS}
    sem = asyncio.Semaphore(RECORDS_CONCURRENCY)
    new_refs: list[dict] = []
    seen_new: set[str] = set()
    in_window_total = 0
    completed = 0

    async def poll(idx: int, player: dict) -> tuple[str, str, list[dict]]:
        async with sem:
            port = ROTATING_PORTS[idx % len(ROTATING_PORTS)]
            recs = await fetch_player_records_all_pages(clients[port], player["_id"])
            rank = (ranks.get(player["_id"]) or {}).get("rank") or player.get("rank") or "x"
            return player["_id"], rank, recs

    try:
        tasks = [poll(i, p) for i, p in enumerate(players)]
        for fut in asyncio.as_completed(tasks):
            pid, rank, recs = await fut
            completed += 1
            for r in recs:
                rid = r.get("replayid")
                ts = r.get("ts")
                if not rid or not ts:
                    continue
                dt = parse_ts(ts)
                if dt is None or dt < cutoff:
                    continue
                in_window_total += 1
                if rid in already or rid in seen_new:
                    continue
                seen_new.add(rid)
                new_refs.append({
                    "replayid": rid,
                    "record_id": r.get("_id"),
                    "player_id": pid,
                    "ts": ts,
                    "gamemode": r.get("gamemode"),
                    "pb": r.get("pb"),
                    "opponents": [u.get("id") for u in (r.get("otherusers") or [])],
                    "rank": rank,
                })
            if completed % 25 == 0 or completed == len(players):
                log(f"  polled {completed}/{len(players)} players | in-window refs={in_window_total} | new unique={len(new_refs)}")
    finally:
        for c in clients.values():
            await c.aclose()

    log(f"discovery done: {in_window_total} in-window records, {len(new_refs)} NEW unique (not already in corpus)")
    if not new_refs:
        log("nothing new to download. Done.")
        MANIFEST_PATH.write_text(json.dumps({"window_days": DAYS, "cutoff": cutoff.isoformat(), "new": 0, "downloaded": 0}, indent=2))
        return

    # Append discovered refs to the shared replay index (keep corpus meta consistent)
    with open(REPLAY_INDEX_PATH, "a") as idx_h:
        for ref in new_refs:
            idx_h.write(json.dumps({k: ref[k] for k in ("replayid", "record_id", "player_id", "ts", "gamemode", "pb", "opponents")}) + "\n")

    # ---- Phase 2: download the new .ttrm files via inoue (port-failover) ----
    log(f"downloading {len(new_refs)} new replays (concurrency {DOWNLOAD_CONCURRENCY})")
    dl_clients = {p: make_client(p) for p in ROTATING_PORTS}
    dsem = asyncio.Semaphore(DOWNLOAD_CONCURRENCY)
    ok = 0
    fail = 0
    bytes_in = 0
    downloaded_ids: list[str] = []

    async def grab(ref: dict):
        async with dsem:
            out = output_path_for(ref["replayid"], ref["rank"], ref["player_id"])
            return ref, await download_one(asyncio.Semaphore(1), dl_clients, ref["replayid"], out)

    try:
        log_h = open(DOWNLOAD_LOG_PATH, "a")
        tasks = [grab(ref) for ref in new_refs]
        done = 0
        for fut in asyncio.as_completed(tasks):
            ref, outcome = await fut
            done += 1
            log_h.write(json.dumps({
                "replayid": outcome.replayid,
                "status": outcome.status,
                "bytes": outcome.bytes_in,
                "err": outcome.err,
                "port": outcome.port,
                "attempts": outcome.attempts,
                "ms": round(outcome.elapsed_ms, 1),
                "ts": time.strftime("%Y-%m-%dT%H:%M:%S"),
                "incremental": STAMP,
            }) + "\n")
            if outcome.status == 200:
                ok += 1
                bytes_in += outcome.bytes_in
                downloaded_ids.append(outcome.replayid)
            else:
                fail += 1
            if done % 20 == 0 or done == len(new_refs):
                log_h.flush()
                log(f"  downloaded {done}/{len(new_refs)} | ok={ok} fail={fail} | {bytes_in/1_048_576:.1f} MiB")
    finally:
        log_h.close()
        for c in dl_clients.values():
            await c.aclose()

    MANIFEST_PATH.write_text(json.dumps({
        "window_days": DAYS,
        "cutoff": cutoff.isoformat(),
        "players_polled": len(players),
        "in_window_records": in_window_total,
        "new_unique": len(new_refs),
        "downloaded_ok": ok,
        "download_failed": fail,
        "bytes_in_mib": round(bytes_in / 1_048_576, 1),
        "downloaded_ids": downloaded_ids,
    }, indent=2))
    log("=" * 70)
    log(f"DONE: {ok} new replays downloaded ({fail} failed), {bytes_in/1_048_576:.1f} MiB. Manifest: {MANIFEST_PATH.name}")


if __name__ == "__main__":
    asyncio.run(main())
