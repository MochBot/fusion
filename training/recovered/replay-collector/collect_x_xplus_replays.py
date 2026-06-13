#!/usr/bin/env python3
# /// script
# requires-python = ">=3.11"
# dependencies = ["httpx>=0.27"]
# ///
"""
Collect every available recent league replay for X and X+ ranked players.

Pipeline:
  Phase A: paginate TETR.IO league leaderboard, dump all X/X+ players
  Phase B: per player, paginate league records, dump all replay IDs
  Phase C: fetch /summaries/league for each player, attach current rank/TR
  Phase D: download every unique .ttrm via inoue.szy.lol with port-failover
           and adaptive backoff on inoue 500s

All output goes to fusion-engine/data/replays-x-xplus/.
State is fully resumable - safe to kill and restart.
"""

from __future__ import annotations

import asyncio
import json
import os
import random
import signal
import sys
import time
from collections import Counter, deque
from dataclasses import dataclass, field
from pathlib import Path

import httpx

UA = "mosaic-fusion-coaching-collector/0.1 (research; xran/xplus league replays for coaching model training)"

ROOT = Path("/Users/li8591/projects/mosaic-fusion-engine-coaching/fusion-engine/data/replays-x-xplus")
META_DIR = ROOT / "_meta"
LOG_DIR = ROOT / "_logs"
PLAYERS_PATH = META_DIR / "players.jsonl"
RANKS_PATH = META_DIR / "ranks.json"
REPLAY_INDEX_PATH = META_DIR / "replay_index.jsonl"
DOWNLOAD_LOG_PATH = META_DIR / "downloads.jsonl"
STATE_PATH = META_DIR / "state.json"
LIVE_LOG_PATH = LOG_DIR / "collector.log"

PROXY_USER = os.environ["GEONODE_PROXY_USER"].strip()
PROXY_PASS = os.environ["GEONODE_PROXY_PASS"].strip()
PROXY_HOST = os.environ["GEONODE_PROXY_BASE"].strip().split(":")[0]
ROTATING_PORTS = list(range(9000, 9011))

TETRA_LB = "https://ch.tetr.io/api/users/by/league"
TETRA_RECORDS = "https://ch.tetr.io/api/users/{user}/records/league/recent"
TETRA_SUMMARY = "https://ch.tetr.io/api/users/{user}/summaries/league"
INOUE_REPLAY = "https://inoue.szy.lol/api/replay/{replayid}"

LB_SESSION = "mosaic-collector-leaderboard-2026-05-26"
PER_PAGE_SLEEP_S = 1.0
DOWNLOAD_CONCURRENCY_MAX = 32
DOWNLOAD_CONCURRENCY_BACKOFF = 18
DOWNLOAD_TIMEOUT_S = 25.0
MAX_DOWNLOAD_ATTEMPTS = 3
ERROR_WINDOW_SIZE = 100
ERROR_500_RATE_TRIGGER = 0.02
COOLDOWN_AFTER_500_BURST_S = 60.0
RECOVERY_HEALTHY_REQUESTS = 200
META_FETCH_CONCURRENCY = 10
RECORDS_FETCH_CONCURRENCY = 8


def log(msg: str) -> None:
    ts = time.strftime("%Y-%m-%d %H:%M:%S")
    line = f"[{ts}] {msg}"
    print(line, flush=True)
    try:
        with open(LIVE_LOG_PATH, "a") as f:
            f.write(line + "\n")
    except Exception:
        pass


def proxy_url(port: int, country: str | None = None) -> str:
    user = PROXY_USER
    if country:
        user = f"{PROXY_USER}-country-{country.lower()}"
    return f"http://{user}:{PROXY_PASS}@{PROXY_HOST}:{port}"


def make_client(port: int, session_id: str | None = None) -> httpx.AsyncClient:
    headers = {"User-Agent": UA, "Accept": "application/json,application/octet-stream"}
    if session_id:
        headers["X-Session-ID"] = session_id
    return httpx.AsyncClient(
        proxy=proxy_url(port),
        headers=headers,
        limits=httpx.Limits(max_keepalive_connections=80, max_connections=160),
        timeout=httpx.Timeout(DOWNLOAD_TIMEOUT_S, connect=10.0),
        follow_redirects=True,
    )


def save_state(state: dict) -> None:
    state["updated_at"] = time.strftime("%Y-%m-%dT%H:%M:%S")
    tmp = STATE_PATH.with_suffix(".json.tmp")
    tmp.write_text(json.dumps(state, indent=2))
    tmp.replace(STATE_PATH)


def load_state() -> dict:
    if STATE_PATH.exists():
        return json.loads(STATE_PATH.read_text())
    return {
        "started_at": time.strftime("%Y-%m-%dT%H:%M:%S"),
        "phase": "init",
    }


async def fetch_json(client: httpx.AsyncClient, url: str, max_retries: int = 3) -> dict | None:
    for attempt in range(max_retries + 1):
        try:
            r = await client.get(url, timeout=20.0)
            if r.status_code == 200:
                try:
                    return r.json()
                except Exception:
                    return None
            if r.status_code in (429, 502, 503, 504) and attempt < max_retries:
                await asyncio.sleep(2.0 * (2**attempt))
                continue
            return None
        except (httpx.TimeoutException, httpx.ConnectError, httpx.RemoteProtocolError):
            if attempt < max_retries:
                await asyncio.sleep(2.0 * (2**attempt))
                continue
        except Exception:
            return None
    return None


async def phase_a_discover_players(state: dict) -> list[dict]:
    if PLAYERS_PATH.exists() and state.get("phase_a_complete"):
        players = [json.loads(line) for line in PLAYERS_PATH.read_text().splitlines() if line.strip()]
        log(f"Phase A: resumed from cache, {len(players)} players")
        return players

    log("Phase A: paginate league leaderboard for X / X+ players")
    players: list[dict] = []
    seen: set[str] = set()
    cursor: str | None = None
    page = 0
    rank_counts: Counter = Counter()

    async with make_client(ROTATING_PORTS[0], session_id=LB_SESSION) as c:
        while True:
            url = f"{TETRA_LB}?limit=100"
            if cursor:
                url += f"&after={cursor}"
            data = await fetch_json(c, url)
            if not data or not data.get("success"):
                log(f"  page {page+1}: fetch failed, aborting Phase A")
                break
            entries = data["data"]["entries"]
            if not entries:
                log(f"  page {page+1}: empty")
                break
            page += 1
            page_added = 0
            page_ranks: Counter = Counter()
            for e in entries:
                uid = e["_id"]
                if uid in seen:
                    continue
                seen.add(uid)
                rank = e.get("league", {}).get("rank", "?")
                if rank not in ("x", "x+"):
                    continue
                players.append({
                    "_id": uid,
                    "username": e.get("username"),
                    "country": e.get("country"),
                    "rank": rank,
                    "tr": e.get("league", {}).get("tr"),
                    "bestrank": e.get("league", {}).get("bestrank"),
                    "glicko": e.get("league", {}).get("glicko"),
                    "rd": e.get("league", {}).get("rd"),
                    "p": e.get("p"),
                })
                page_added += 1
                rank_counts[rank] += 1
                page_ranks[rank] += 1
            last = entries[-1]["p"]
            cursor = f"{last['pri']}:{last['sec']}:{last['ter']}"
            last_tr = entries[-1]["league"]["tr"]
            log(f"  page {page}: +{page_added} X/X+ added, last_tr={last_tr:.1f}, total X/X+ so far {len(players)} {dict(page_ranks)}")
            if page_added == 0:
                log("  no new X/X+ on this page, end of X-tier range")
                break
            await asyncio.sleep(PER_PAGE_SLEEP_S)

    PLAYERS_PATH.parent.mkdir(parents=True, exist_ok=True)
    with open(PLAYERS_PATH, "w") as f:
        for p in players:
            f.write(json.dumps(p) + "\n")
    log(f"Phase A complete: {len(players)} players ({dict(rank_counts)})")
    state["phase_a_complete"] = True
    state["total_players"] = len(players)
    state["rank_counts"] = dict(rank_counts)
    save_state(state)
    return players


async def fetch_player_records_all_pages(
    client: httpx.AsyncClient, user_id: str, max_pages: int = 50
) -> list[dict]:
    records: list[dict] = []
    cursor: str | None = None
    for _ in range(max_pages):
        url = TETRA_RECORDS.format(user=user_id) + "?limit=100"
        if cursor:
            url += f"&after={cursor}"
        data = await fetch_json(client, url)
        if not data or not data.get("success"):
            break
        entries = data.get("data", {}).get("entries") or []
        if not entries:
            break
        non_stub = [e for e in entries if not e.get("stub") and e.get("replayid")]
        records.extend(non_stub)
        last = entries[-1].get("p")
        if not last:
            break
        cursor = f"{last['pri']}:{last['sec']}:{last['ter']}"
        if len(entries) < 100:
            break
        await asyncio.sleep(0.3)
    return records


async def phase_b_discover_replays(state: dict, players: list[dict]) -> dict[str, list[dict]]:
    already_have: dict[str, list[dict]] = {}
    if REPLAY_INDEX_PATH.exists():
        for line in REPLAY_INDEX_PATH.read_text().splitlines():
            if not line.strip():
                continue
            rec = json.loads(line)
            pid = rec["player_id"]
            already_have.setdefault(pid, []).append(rec)
        log(f"Phase B: resumed, {len(already_have)} players already indexed, {sum(len(v) for v in already_have.values())} replay refs known")

    needed = [p for p in players if p["_id"] not in already_have]
    if not needed:
        log("Phase B: all players already indexed")
        state["phase_b_complete"] = True
        save_state(state)
        return already_have

    log(f"Phase B: fetching paginated records for {len(needed)} new players")
    clients = {p: make_client(p) for p in ROTATING_PORTS}
    sem = asyncio.Semaphore(RECORDS_FETCH_CONCURRENCY)
    completed = 0
    progress_lock = asyncio.Lock()

    async def one(idx: int, player: dict) -> tuple[str, list[dict]]:
        async with sem:
            port = ROTATING_PORTS[idx % len(ROTATING_PORTS)]
            recs = await fetch_player_records_all_pages(clients[port], player["_id"])
            return player["_id"], recs

    out_handle = open(REPLAY_INDEX_PATH, "a")
    try:
        tasks = [one(i, p) for i, p in enumerate(needed)]
        for fut in asyncio.as_completed(tasks):
            pid, recs = await fut
            simple = []
            for r in recs:
                others = [u.get("id") for u in (r.get("otherusers") or [])]
                simple.append({
                    "replayid": r["replayid"],
                    "record_id": r.get("_id"),
                    "player_id": pid,
                    "ts": r.get("ts"),
                    "gamemode": r.get("gamemode"),
                    "pb": r.get("pb"),
                    "opponents": others,
                })
            for entry in simple:
                out_handle.write(json.dumps(entry) + "\n")
            out_handle.flush()
            already_have[pid] = simple
            async with progress_lock:
                completed += 1
                if completed % 25 == 0 or completed == len(needed):
                    log(f"  Phase B: {completed}/{len(needed)} players indexed, "
                        f"~{sum(len(v) for v in already_have.values())} replay refs total")
    finally:
        out_handle.close()
        for c in clients.values():
            await c.aclose()

    state["phase_b_complete"] = True
    state["total_replay_refs"] = sum(len(v) for v in already_have.values())
    save_state(state)
    return already_have


async def phase_c_attach_ranks(state: dict, players: list[dict]) -> dict[str, dict]:
    existing: dict[str, dict] = {}
    if RANKS_PATH.exists():
        existing = json.loads(RANKS_PATH.read_text())
        log(f"Phase C: resumed, {len(existing)} ranks already attached")

    needed = [p for p in players if p["_id"] not in existing]
    if not needed:
        log("Phase C: all ranks attached")
        state["phase_c_complete"] = True
        save_state(state)
        return existing

    log(f"Phase C: fetching /summaries/league for {len(needed)} new players")
    clients = {p: make_client(p) for p in ROTATING_PORTS}
    sem = asyncio.Semaphore(META_FETCH_CONCURRENCY)

    async def one(idx: int, p: dict) -> tuple[str, dict | None]:
        async with sem:
            port = ROTATING_PORTS[idx % len(ROTATING_PORTS)]
            data = await fetch_json(clients[port], TETRA_SUMMARY.format(user=p["_id"]))
            if data and data.get("success"):
                d = data.get("data", {})
                return p["_id"], {
                    "rank": d.get("rank"),
                    "tr": d.get("tr"),
                    "bestrank": d.get("bestrank"),
                    "standing": d.get("standing"),
                    "standing_local": d.get("standing_local"),
                    "glicko": d.get("glicko"),
                    "rd": d.get("rd"),
                    "apm": d.get("apm"),
                    "pps": d.get("pps"),
                    "vs": d.get("vs"),
                    "gamesplayed": d.get("gamesplayed"),
                    "decaying": d.get("decaying"),
                    "fetched_at": time.strftime("%Y-%m-%dT%H:%M:%S"),
                }
            return p["_id"], None

    try:
        completed = 0
        for fut in asyncio.as_completed([one(i, p) for i, p in enumerate(needed)]):
            pid, meta = await fut
            if meta:
                existing[pid] = meta
            completed += 1
            if completed % 50 == 0 or completed == len(needed):
                RANKS_PATH.write_text(json.dumps(existing, indent=2))
                log(f"  Phase C: {completed}/{len(needed)} attached")
    finally:
        for c in clients.values():
            await c.aclose()
        RANKS_PATH.write_text(json.dumps(existing, indent=2))

    state["phase_c_complete"] = True
    state["ranks_attached"] = len(existing)
    save_state(state)
    return existing


@dataclass
class DLOutcome:
    replayid: str
    status: int | None
    bytes_in: int
    err: str | None
    port: int
    attempts: int
    elapsed_ms: float


@dataclass
class CollectorStats:
    started_at: float = field(default_factory=time.time)
    attempted: int = 0
    success: int = 0
    failed: int = 0
    skipped: int = 0
    bytes_in: int = 0
    err_counter: Counter = field(default_factory=Counter)
    recent_outcomes: deque = field(default_factory=lambda: deque(maxlen=ERROR_WINDOW_SIZE))
    current_concurrency: int = DOWNLOAD_CONCURRENCY_MAX
    cooldowns_triggered: int = 0
    last_log: float = field(default_factory=time.time)

    def record(self, outcome: DLOutcome) -> None:
        self.attempted += 1
        self.bytes_in += outcome.bytes_in
        self.recent_outcomes.append(outcome.status or 0)
        if outcome.status == 200:
            self.success += 1
        else:
            self.failed += 1
            self.err_counter[outcome.err or f"status_{outcome.status}"] += 1

    def error_500_rate(self) -> float:
        if not self.recent_outcomes:
            return 0.0
        n_500 = sum(1 for s in self.recent_outcomes if s == 500)
        return n_500 / len(self.recent_outcomes)

    def healthy_streak(self) -> int:
        streak = 0
        for s in reversed(self.recent_outcomes):
            if s == 200:
                streak += 1
            else:
                break
        return streak

    def summary(self, total_target: int) -> str:
        elapsed = time.time() - self.started_at
        rate = self.success / elapsed if elapsed > 0 else 0
        remaining = max(0, total_target - self.attempted)
        eta_s = remaining / rate if rate > 0 else float("inf")
        eta_str = "?" if eta_s == float("inf") else (f"{eta_s/60:.0f}m" if eta_s < 7200 else f"{eta_s/3600:.1f}h")
        return (f"attempted={self.attempted}/{total_target} "
                f"success={self.success} fail={self.failed} skipped={self.skipped} "
                f"bw={self.bytes_in/1_048_576/elapsed if elapsed>0 else 0:.2f}MiB/s "
                f"rate={rate:.2f}/s conc={self.current_concurrency} "
                f"err500_rate={self.error_500_rate()*100:.1f}% eta={eta_str}")


def output_path_for(replayid: str, rank: str, player_id: str) -> Path:
    rank_dir = ROOT / rank
    player_dir = rank_dir / player_id
    player_dir.mkdir(parents=True, exist_ok=True)
    return player_dir / f"{replayid}.ttrm"


async def download_one(
    sem: asyncio.Semaphore,
    clients: dict[int, httpx.AsyncClient],
    replayid: str,
    out_path: Path,
) -> DLOutcome:
    async with sem:
        ports = list(clients.keys())
        random.shuffle(ports)
        t0 = time.perf_counter()
        last_err: str | None = None
        last_status: int | None = None
        attempts = 0
        for attempt in range(MAX_DOWNLOAD_ATTEMPTS):
            port = ports[attempt % len(ports)]
            client = clients[port]
            url = INOUE_REPLAY.format(replayid=replayid)
            attempts += 1
            try:
                async with client.stream("GET", url, timeout=DOWNLOAD_TIMEOUT_S) as r:
                    last_status = r.status_code
                    if r.status_code != 200:
                        body = await r.aread()
                        last_err = f"http_{r.status_code}"
                        if r.status_code in (429, 500, 502, 503, 504):
                            await asyncio.sleep(2.0 * (attempt + 1))
                            continue
                        return DLOutcome(replayid, r.status_code, len(body), last_err, port, attempts, (time.perf_counter()-t0)*1000)
                    buf = bytearray()
                    async for chunk in r.aiter_bytes():
                        buf.extend(chunk)
                    if not buf or buf[:1] != b"{":
                        last_err = "invalid_body"
                        await asyncio.sleep(0.5)
                        continue
                    tmp = out_path.with_suffix(out_path.suffix + ".tmp")
                    tmp.write_bytes(buf)
                    tmp.replace(out_path)
                    return DLOutcome(replayid, 200, len(buf), None, port, attempts, (time.perf_counter()-t0)*1000)
            except asyncio.TimeoutError:
                last_err = "timeout"
                await asyncio.sleep(1.0)
            except (httpx.TimeoutException,) as e:
                last_err = f"httpx_{type(e).__name__}"
                await asyncio.sleep(1.0)
            except (httpx.ConnectError, httpx.RemoteProtocolError) as e:
                last_err = type(e).__name__
                await asyncio.sleep(1.0)
            except Exception as e:
                last_err = f"{type(e).__name__}"
                break
        return DLOutcome(replayid, last_status, 0, last_err or "exhausted", port, attempts, (time.perf_counter()-t0)*1000)


async def phase_d_download(
    state: dict, replay_index: dict[str, list[dict]], rank_map: dict[str, dict]
) -> None:
    log("Phase D: build unique download queue")

    dedup: dict[str, tuple[str, str]] = {}
    for pid, refs in replay_index.items():
        rank_info = rank_map.get(pid) or {}
        rank = rank_info.get("rank") or "x"
        for ref in refs:
            rid = ref["replayid"]
            if rid in dedup:
                continue
            dedup[rid] = (rank, pid)
    total_unique = len(dedup)
    log(f"  unique replay IDs to download: {total_unique:,}")

    already: set[str] = set()
    if DOWNLOAD_LOG_PATH.exists():
        for line in DOWNLOAD_LOG_PATH.read_text().splitlines():
            if not line.strip():
                continue
            try:
                entry = json.loads(line)
                if entry.get("status") == 200 or entry.get("on_disk"):
                    already.add(entry["replayid"])
            except Exception:
                continue
    for rid, (rank, pid) in dedup.items():
        if rid in already:
            continue
        out = output_path_for(rid, rank, pid)
        if out.exists() and out.stat().st_size > 10000:
            already.add(rid)
    log(f"  already on disk / previously succeeded: {len(already):,}")

    queue = [(rid, dedup[rid][0], dedup[rid][1]) for rid in dedup if rid not in already]
    log(f"  queue length: {len(queue):,}")
    if not queue:
        log("Phase D: nothing to download")
        state["phase_d_complete"] = True
        save_state(state)
        return

    random.shuffle(queue)

    clients = {p: make_client(p) for p in ROTATING_PORTS}
    stats = CollectorStats()
    log_handle = open(DOWNLOAD_LOG_PATH, "a")

    state["phase"] = "downloading"
    state["queue_initial_size"] = len(queue)
    save_state(state)

    async def worker_pool() -> None:
        sem = asyncio.Semaphore(stats.current_concurrency)
        active = sem

        async def gated(rid: str, rank: str, pid: str) -> DLOutcome:
            nonlocal active
            if active is not sem:
                pass
            async with sem:
                out = output_path_for(rid, rank, pid)
                return await download_one(asyncio.Semaphore(1), clients, rid, out)

        tasks = []
        for rid, rank, pid in queue:
            tasks.append(asyncio.create_task(gated(rid, rank, pid)))

        for fut in asyncio.as_completed(tasks):
            outcome = await fut
            stats.record(outcome)

            log_handle.write(json.dumps({
                "replayid": outcome.replayid,
                "status": outcome.status,
                "bytes": outcome.bytes_in,
                "err": outcome.err,
                "port": outcome.port,
                "attempts": outcome.attempts,
                "ms": round(outcome.elapsed_ms, 1),
                "ts": time.strftime("%Y-%m-%dT%H:%M:%S"),
            }) + "\n")
            if stats.attempted % 20 == 0:
                log_handle.flush()

            now = time.time()
            if now - stats.last_log > 10:
                log(f"  Phase D: {stats.summary(len(queue))}")
                stats.last_log = now
                state["phase_d_progress"] = {
                    "attempted": stats.attempted,
                    "success": stats.success,
                    "failed": stats.failed,
                    "bytes_in_mib": round(stats.bytes_in / 1_048_576, 1),
                    "current_concurrency": stats.current_concurrency,
                    "err_counter": dict(stats.err_counter.most_common(10)),
                }
                save_state(state)

            if stats.error_500_rate() > ERROR_500_RATE_TRIGGER and stats.current_concurrency > DOWNLOAD_CONCURRENCY_BACKOFF:
                log(f"  ! 500-rate {stats.error_500_rate()*100:.1f}% - cooldown 60s + drop concurrency to {DOWNLOAD_CONCURRENCY_BACKOFF}")
                stats.cooldowns_triggered += 1
                await asyncio.sleep(COOLDOWN_AFTER_500_BURST_S)
                stats.current_concurrency = DOWNLOAD_CONCURRENCY_BACKOFF
            elif stats.healthy_streak() >= RECOVERY_HEALTHY_REQUESTS and stats.current_concurrency < DOWNLOAD_CONCURRENCY_MAX:
                log(f"  + healthy streak {stats.healthy_streak()} - raise concurrency to {DOWNLOAD_CONCURRENCY_MAX}")
                stats.current_concurrency = DOWNLOAD_CONCURRENCY_MAX

    try:
        await worker_pool()
    finally:
        log_handle.close()
        for c in clients.values():
            await c.aclose()

    state["phase_d_complete"] = True
    state["phase_d_final"] = {
        "attempted": stats.attempted,
        "success": stats.success,
        "failed": stats.failed,
        "bytes_in_mib": round(stats.bytes_in / 1_048_576, 1),
        "err_counter": dict(stats.err_counter.most_common(20)),
        "cooldowns_triggered": stats.cooldowns_triggered,
        "total_seconds": round(time.time() - stats.started_at, 1),
    }
    save_state(state)
    log("Phase D complete: " + stats.summary(len(queue)))


def handle_sigterm(signum, frame) -> None:
    log(f"Received signal {signum}, will exit after current asyncio iteration")
    sys.exit(0)


async def main() -> None:
    ROOT.mkdir(parents=True, exist_ok=True)
    META_DIR.mkdir(parents=True, exist_ok=True)
    LOG_DIR.mkdir(parents=True, exist_ok=True)

    signal.signal(signal.SIGTERM, handle_sigterm)
    signal.signal(signal.SIGINT, handle_sigterm)

    state = load_state()
    log("=" * 70)
    log(f"Mosaic X/X+ league replay collector starting")
    log(f"Output root: {ROOT}")
    log(f"Resuming from state: phase_a={state.get('phase_a_complete', False)} "
        f"phase_b={state.get('phase_b_complete', False)} "
        f"phase_c={state.get('phase_c_complete', False)} "
        f"phase_d={state.get('phase_d_complete', False)}")
    log("=" * 70)

    players = await phase_a_discover_players(state)
    if not players:
        log("FATAL: no players discovered; abort")
        return

    replay_index = await phase_b_discover_replays(state, players)
    rank_map = await phase_c_attach_ranks(state, players)

    await phase_d_download(state, replay_index, rank_map)

    state["phase"] = "complete"
    state["completed_at"] = time.strftime("%Y-%m-%dT%H:%M:%S")
    save_state(state)
    log("ALL PHASES COMPLETE")


if __name__ == "__main__":
    asyncio.run(main())
