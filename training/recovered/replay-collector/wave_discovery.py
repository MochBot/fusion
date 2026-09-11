#!/usr/bin/env python3
# /// script
# requires-python = ">=3.11"
# dependencies = ["httpx>=0.27"]
# ///
"""Live Tetra Channel discovery for the X/X+/U/SS replay wave.

Fresh leaderboard pagination plus per-player league-record paging over the
base collector's direct client. No caches are read; every report carries an
explicit completeness flag instead of claiming complete on truncation.
"""

from __future__ import annotations

import asyncio
import importlib
import sys
import time
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any

SCRIPTS_DIR = Path(__file__).resolve().parent
sys.path.insert(0, str(SCRIPTS_DIR))

collector = importlib.import_module("collect_x_xplus_replays")
wave_selection = importlib.import_module("wave_selection")
fetch_json = collector.fetch_json
TETRA_LB = collector.TETRA_LB
TETRA_RECORDS = collector.TETRA_RECORDS
PlayerId = wave_selection.PlayerId
WaveRef = wave_selection.WaveRef
dedup_all = wave_selection.dedup_all
qualifies_for_next_page = wave_selection.qualifies_for_next_page
ref_from_record = wave_selection.ref_from_record
select_global_newest = wave_selection.select_global_newest
Json = dict[str, Any]

RECORDS_CONCURRENCY = 8
RECORDS_PAGE_SLEEP_S = 0.3
RECORDS_PAGE_TIMEOUT_S = 45.0


def log(msg: str) -> None:
    """Print a timestamped discovery line to stdout."""
    print(f"[{time.strftime('%Y-%m-%d %H:%M:%S')}] {msg}", flush=True)


async def fetch_leaderboard_players(client: Any, ranks: tuple[str, ...], page_cap: int) -> tuple[dict[str, list[Json]], Json]:
    """Paginate the live leaderboard fresh; stop past the target rank band."""
    players: dict[str, list[Json]] = {rank: [] for rank in ranks}
    seen: set[str] = set()
    cursor: str | None = None
    pages = 0
    players_seen = 0
    report: Json = {"pages": 0, "players_seen": 0, "stop_reason": "cap", "complete": False}
    while pages < page_cap:
        url = f"{TETRA_LB}?limit=100"
        if cursor:
            url += f"&after={cursor}"
        data = await fetch_json(client, url)
        if not data or not data.get("success"):
            report.update({"pages": pages, "players_seen": players_seen, "stop_reason": "fetch_failed", "complete": False})
            return players, report
        entries = data["data"]["entries"]
        if not entries:
            report.update({"pages": pages, "players_seen": players_seen, "stop_reason": "exhausted", "complete": True})
            return players, report
        pages += 1
        players_seen += len(entries)
        page_added = 0
        for entry in entries:
            uid = str(entry["_id"])
            if uid in seen:
                continue
            seen.add(uid)
            rank = str(entry.get("league", {}).get("rank", "")).lower()
            if rank in players:
                players[rank].append(entry)
                page_added += 1
        last_rank = str(entries[-1].get("league", {}).get("rank", "")).lower()
        log(f"  leaderboard page {pages}: +{page_added} wave-rank players, last_rank={last_rank}")
        if page_added == 0 and last_rank not in ranks:
            report.update({"pages": pages, "players_seen": players_seen, "stop_reason": f"past_band_at_{last_rank}", "complete": True})
            return players, report
        last = entries[-1]["p"]
        cursor = f"{last['pri']}:{last['sec']}:{last['ter']}"
        await asyncio.sleep(collector.PER_PAGE_SLEEP_S)
    report.update({"pages": pages, "players_seen": players_seen, "stop_reason": "page_cap", "complete": False})
    return players, report


async def fetch_record_page(client: Any, player_id: str, cursor: str | None) -> list[Json]:
    """Fetch one league-record page, raising fail-closed on transport/API failure."""
    url = TETRA_RECORDS.format(user=player_id) + "?limit=100"
    if cursor:
        url += f"&after={cursor}"
    data = await fetch_json(client, url)
    if not data or not data.get("success"):
        raise RuntimeError(f"records page failed for {player_id}")
    return data.get("data", {}).get("entries") or []


@dataclass(frozen=True, slots=True)
class PageTransport:
    """Concurrency-limited transport for record paging (client travels with its gate)."""

    client: Any
    gate: asyncio.Semaphore


@dataclass(frozen=True, slots=True)
class PageRequest:
    """One record-page request: whose records, at which rank, after which cursor."""

    pid: str
    rank: str
    cursor: str | None


@dataclass(slots=True)
class PlayerProgress:
    """Mutable per-player paging accumulator (builder owned by the discovery loop)."""

    refs: list[WaveRef] = field(default_factory=list)
    cursor: str | None = None
    exhausted: bool = False
    pages: int = 0
    oldest: float | None = None


async def page_refs(transport: PageTransport, request: PageRequest) -> tuple[list[WaveRef], int, str | None, bool]:
    """Fetch one record page and project its entries; exhausted when short or cursorless."""
    async with transport.gate:
        page = await asyncio.wait_for(
            fetch_record_page(transport.client, request.pid, request.cursor),
            timeout=RECORDS_PAGE_TIMEOUT_S,
        )
        await asyncio.sleep(RECORDS_PAGE_SLEEP_S)
    refs = [ref for record in page if (ref := ref_from_record(PlayerId(request.pid), request.rank, record))]
    if len(page) < 100:
        return refs, len(page), None, True
    last = page[-1].get("p")
    next_cursor = f"{last['pri']}:{last['sec']}:{last['ter']}" if last else None
    return refs, len(page), next_cursor, next_cursor is None


def track_page(progress: PlayerProgress, refs: list[WaveRef], cursor: str | None, exhausted: bool) -> None:
    """Fold one fetched page into per-player discovery state."""
    epochs = [ref.ts_epoch for ref in refs if ref.ts_epoch is not None]
    progress.refs.extend(refs)
    progress.cursor = cursor
    progress.exhausted = exhausted
    progress.pages += 1
    if epochs:
        oldest = min(epochs)
        progress.oldest = oldest if progress.oldest is None else min(progress.oldest, oldest)


async def discover_rank_refs(
    client: Any,
    rank: str,
    entries: list[Json],
    top_n: int | None,
    max_pages: int,
) -> tuple[list[WaveRef], Json]:
    """Fetch per-player records; take-all for X/X+, global-newest top_n with early-stop otherwise."""
    report: Json = {"players": len(entries), "records_scanned": 0, "pages": 0,
                    "truncated_players": [], "failed_players": [], "complete": True}
    transport = PageTransport(client, asyncio.Semaphore(RECORDS_CONCURRENCY))
    per_player: dict[str, PlayerProgress] = {}

    def current_threshold() -> float:
        if top_n is None:
            return float("-inf")
        epochs = sorted((r.ts_epoch for progress in per_player.values() for r in progress.refs if r.ts_epoch is not None), reverse=True)
        return epochs[top_n - 1] if len(epochs) >= top_n else float("-inf")

    if top_n is not None:
        ordered: list[WaveRef] = []
        seen_ids: set[str] = set()
        for entry in entries:
            pid = str(entry["_id"])
            progress = PlayerProgress()
            per_player[pid] = progress
            while not progress.exhausted and progress.pages < max_pages and len(seen_ids) < top_n:
                request = PageRequest(pid, rank, progress.cursor)
                try:
                    refs, raw, cursor, exhausted = await page_refs(transport, request)
                except (TimeoutError, RuntimeError) as error:
                    report["failed_players"].append(f"{pid}: {error}")
                    progress.exhausted = True
                    report["complete"] = False
                    break
                track_page(progress, refs, cursor, exhausted)
                report["records_scanned"] += raw
                report["pages"] += 1
                for ref in refs:
                    if str(ref.replayid) not in seen_ids:
                        seen_ids.add(str(ref.replayid))
                        ordered.append(ref)
            if not progress.exhausted and progress.pages >= max_pages:
                report["truncated_players"].append(pid)
                report["complete"] = False
            log(
                f"  {rank}: player={pid} pages={progress.pages} "
                f"selected={len(seen_ids)}/{top_n} failures={len(report['failed_players'])}"
            )
            if len(seen_ids) >= top_n:
                break
        return ordered[:top_n], report

    pending: list[PageRequest] = [PageRequest(str(e["_id"]), rank, None) for e in entries]
    for request in pending:
        per_player[request.pid] = PlayerProgress()
    while pending:
        results = await asyncio.gather(
            *(page_refs(transport, request) for request in pending), return_exceptions=True)
        next_pending: list[PageRequest] = []
        for request, result in zip(pending, results):
            pid = request.pid
            if isinstance(result, BaseException):
                report["failed_players"].append(f"{pid}: {result}")
                per_player[pid].exhausted = True
                continue
            refs, raw, cursor, exhausted = result
            progress = per_player[pid]
            track_page(progress, refs, cursor, exhausted)
            report["records_scanned"] += raw
            report["pages"] += 1
            if progress.pages >= max_pages and not exhausted:
                report["truncated_players"].append(pid)
                progress.exhausted = True
                continue
            if qualifies_for_next_page(progress.oldest, progress.exhausted, current_threshold()):
                next_pending.append(PageRequest(pid, rank, progress.cursor))
        log(
            f"  {rank}: records batch={len(pending)} pages={report['pages']} "
            f"pending_next={len(next_pending)} failures={len(report['failed_players'])}"
        )
        pending = next_pending
    if report["truncated_players"] or report["failed_players"]:
        report["complete"] = False
    all_refs = [ref for progress in per_player.values() for ref in progress.refs]
    selected = select_global_newest(all_refs, top_n) if top_n is not None else dedup_all(all_refs)
    return selected, report
