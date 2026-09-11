#!/usr/bin/env python3
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Pure selection policy for the X/X+/U/SS replay wave.

Rank/recency/dedup rules with no network access, so focused tests can pin
them without fakes: X/X+ take all API-available refs, U/SS take the
globally newest top_n unique replay IDs by record timestamp, every replay
ID is fetched at most once, and all candidate refs are preserved for rank
overlap.
"""

from __future__ import annotations

from dataclasses import dataclass
from datetime import datetime
from pathlib import Path
from typing import Any, NewType

PlayerId = NewType("PlayerId", str)
ReplayId = NewType("ReplayId", str)
Json = dict[str, Any]


def parse_ts_epoch(value: Any) -> float | None:
    """Parse an ISO-8601 record timestamp to epoch seconds, or None."""
    if not isinstance(value, str):
        return None
    try:
        parsed = datetime.fromisoformat(value.replace("Z", "+00:00"))
    except ValueError:
        return None
    if parsed.tzinfo is None:
        return None
    return parsed.timestamp()


@dataclass(frozen=True, slots=True)
class WaveRef:
    """One candidate replay reference from a player's record list."""

    replayid: ReplayId
    record_id: str | None
    player_id: PlayerId
    rank: str
    ts: str | None
    ts_epoch: float | None
    opponents: tuple[str, ...]


def ref_from_record(player_id: PlayerId, rank: str, record: Json) -> WaveRef | None:
    """Project a raw league record entry onto a WaveRef, or None when unusable."""
    replayid = record.get("replayid")
    if not replayid or record.get("stub"):
        return None
    ts = record.get("ts")
    return WaveRef(
        replayid=ReplayId(str(replayid)),
        record_id=record.get("_id"),
        player_id=player_id,
        rank=rank,
        ts=ts if isinstance(ts, str) else None,
        ts_epoch=parse_ts_epoch(ts),
        opponents=tuple(str(u.get("id")) for u in (record.get("otherusers") or []) if isinstance(u, dict) and u.get("id")),
    )


def select_global_newest(refs: list[WaveRef], top_n: int) -> list[WaveRef]:
    """Dedup refs by replay ID keeping the newest timestamp, return newest top_n."""
    newest: dict[ReplayId, WaveRef] = {}
    for ref in refs:
        if ref.ts_epoch is None:
            continue
        prior = newest.get(ref.replayid)
        if prior is None or (ref.ts_epoch, ref.player_id) > (prior.ts_epoch or 0.0, prior.player_id):
            newest[ref.replayid] = ref
    return sorted(newest.values(), key=lambda r: (r.ts_epoch or 0.0, r.replayid), reverse=True)[:top_n]


def dedup_all(refs: list[WaveRef]) -> list[WaveRef]:
    """Dedup refs by replay ID keeping the newest timestamp (X/X+ take-all path)."""
    newest: dict[ReplayId, WaveRef] = {}
    for ref in refs:
        prior = newest.get(ref.replayid)
        if prior is None or (ref.ts_epoch or 0.0, ref.player_id) > (prior.ts_epoch or 0.0, prior.player_id):
            newest[ref.replayid] = ref
    return sorted(newest.values(), key=lambda r: (r.ts_epoch or 0.0, r.replayid), reverse=True)


def qualifies_for_next_page(oldest_fetched_epoch: float | None, exhausted: bool, threshold: float) -> bool:
    """True while a player's deeper record pages can still beat the global threshold."""
    if exhausted:
        return False
    if oldest_fetched_epoch is None:
        return True
    return oldest_fetched_epoch > threshold


def inventory_existing_bodies(roots: list[Path]) -> set[str]:
    """Inventory local replay bodies by file stem across existing corpora."""
    stems: set[str] = set()
    for root in roots:
        if not root.exists():
            continue
        stems.update(path.stem for path in root.glob("**/*.ttrm"))
    return stems


def split_existing(selected: list[WaveRef], existing: set[str]) -> tuple[list[ReplayId], list[ReplayId]]:
    """Split selected IDs into (already-on-disk, still-missing) replay ID lists."""
    on_disk = [ref.replayid for ref in selected if ref.replayid in existing]
    missing = [ref.replayid for ref in selected if ref.replayid not in existing]
    return on_disk, missing
