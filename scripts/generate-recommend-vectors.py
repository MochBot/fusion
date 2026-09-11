#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Emit the shared recommend-v1 contract vectors as canonical JSON.

Every vector below is declared here as an explicit builder fed literal values,
so the shared contract inputs stay reviewable in source-only checkouts where the
generated JSON is not tracked. The generator never imports or runs the engine,
the recommendation runtime, or a search, and never reads the generated artifact:
expected values are literals from the wire contract, not results of a run.

Usage:

  python3 scripts/generate-recommend-vectors.py "$TMP/recommend-v1.json"
  diff "$TMP/recommend-v1.json" ../tests/fixtures/recommend-v1.json
"""

from __future__ import annotations

import argparse
import json
from pathlib import Path
from typing import Final

EMPTY_ROWS: Final = (0,) * 40
TOPPED_OUT_ROWS: Final = (1023,) * 40
EMPTY_GMASK: Final = (0,) * 40
SEEDLESS: Final = -1
PARTIAL_ROWS: Final = (1, 1) + EMPTY_ROWS[2:]
HEADER: Final = {
    "contract": "recommend-v1",
    "notes": "Shared replay recommendation fixtures. Unknown facts remain absent or null.",
}

JsonValue = str | int | float | bool | None | list["JsonValue"] | dict[str, "JsonValue"]
JsonObject = dict[str, JsonValue]

# Absent facts are dropped so a missing fact never reads as a zero-filled stand-in.
# These source field names are the deliberate exceptions the contract states as an explicit null.
STATED_NULLS: Final = frozenset({"hold", "artifact", "final_gmask"})
# Availability facts the contract states as an explicit null rather than omitting.
STATED_NULL: Final = "\0stated-null"
# Availability facts the contract omits entirely; the marker is never emitted.
ABSENT: Final = "\0absent"


def _obj(fields: JsonObject) -> JsonObject:
    """Render a nested wire object with browser field names, keeping only stated nulls."""
    kept = _drop_absent(fields)
    return {_camel(f): _wire_value(item) for f, item in kept.items()}


def _drop_absent(fields: JsonObject) -> JsonObject:
    """Remove fields whose fact is unavailable, unless the contract states them as null."""
    return {f: item for f, item in fields.items()
            if item is not ABSENT and (item is not None or f in STATED_NULLS)}


def _wire_null(item: JsonValue) -> JsonValue:
    """Render the stated-null marker as JSON null; every other value passes through."""
    return None if item is STATED_NULL else item


def _wire_value(item: JsonValue) -> JsonValue:
    """Render one nested value, so paths, scores, and injected input blocks convert too."""
    if _is_stated(item):
        return _wire_null(item)
    if isinstance(item, dict):
        return _obj(item)
    if isinstance(item, list):
        return [_wire_value(entry) for entry in item]
    return item


def _is_stated(item: JsonValue) -> bool:
    """True for either availability marker, which is never real contract data."""
    return item is STATED_NULL or item is ABSENT


def _camel(field: str) -> str:
    """Browser spelling of one contract field: snake_case converts, wire names pass through."""
    if "_" not in field:
        return field
    head, *rest = field.split("_")
    return head + "".join(part.capitalize() for part in rest)


def _position(pieces: str, rows: tuple[int, ...] = EMPTY_ROWS, hold: int | None = None) -> JsonObject:
    """A full 40-row situation with the whole empty queue horizon."""
    return {"board_rows": rows, "gmask": EMPTY_GMASK, "pieces": tuple(int(p) for p in pieces.split(",")),
            "hold": hold, "b2b": 0, "combo": 0, "pending": 0, "mult": 1, "beam_width": 20}


def _place(piece: int, rotation: int, x: int, y: int, *, hold: bool = False) -> JsonObject:
    """One path placement; spin stays north for every shared vector."""
    return {"piece": piece, "rotation": rotation, "x": x, "y": y, "spin": 0, "hold_used": hold}


def _root(piece: int, rotation: int, x: int, y: int, score: float) -> JsonObject:
    """One scored root placement reported by the native search."""
    return {"piece": piece, "rotation": rotation, "x": x, "y": y, "spin": 0, "score": score}


def _step(raw_attack: int, surge_delta: int, lines: int, combo_after: int) -> JsonObject:
    """Per-step mechanics for this contract's fixed chain: no b2b seed, no surge release."""
    return {"rawAttack": raw_attack, "surgeDelta": surge_delta, "lines": lines, "b2bAfter": 0,
            "comboAfter": combo_after, "b2bBefore": 0, "comboBefore": 0, "isSurgeRelease": False}


def _assumptions(seed_b2b: int, *, hold: bool = False, extend: bool = False) -> JsonObject:
    """Executed-assumption block reported beside a ready candidate."""
    return {"holdSupported": hold, "queueExtension7bag": extend, "seedB2b": seed_b2b,
            "seedCombo": 0, "seedPending": 0, "garbageMult": 1}


def _request(situation: JsonObject, *, out: JsonObject | None = None, replace: bool = False) -> JsonObject:
    """One native-search request: `out` adds input facts, or becomes the whole input envelope."""
    envelope = dict(out or {}) if replace else {
        "kind": "fullPosition", "holdSupported": True, "queueExtension7bag": False, **(out or {})}
    return {"version": 1, "source": "native-search", "config": "full-search",
            "input": envelope, "situation": situation}

def _candidate(
    source: str,
    path: list[JsonObject],
    completion: str,
    requested: int,
    mechanics: list[JsonObject],
    final_rows: tuple[int, ...] | None,
    scores: JsonObject,
    assumptions: JsonObject,
    shaped: int | None = None,
    raw: int | None = None,
    surge: int | None = None,
    stated_nulls: bool = False,
) -> JsonObject:
    """Ready candidate; unavailable rows, totals, and gmask stay absent unless the contract states them."""
    return {"source": source, "config": "full-search" if source == "native-search" else "exact",
            "path": path, "completion": completion, "requested_plies": requested,
            "completed_plies": len(path), "mechanics": mechanics, "shaped_value": shaped,
            "raw_total": raw, "surge_total": surge, "final_rows": final_rows,
            "artifact": STATED_NULL if stated_nulls else ABSENT,
            "final_gmask": STATED_NULL if stated_nulls else ABSENT,
            "scores": scores, "assumptions": assumptions}


def _named(name: str, *, outcome: JsonObject | None = None, request: JsonObject | None = None,
           answer: JsonObject | None = None) -> JsonObject:
    """One named vector, converted to wire spelling once; the payload stays source spelling here."""
    if request is not None:
        return _entry({"name": name, "expected": answer, "request": request})
    return _entry({"name": name, "outcome": outcome})


def _ready(candidate: JsonObject) -> JsonObject:
    """A ready outcome wrapping one candidate."""
    return {"status": "ready", "candidate": candidate}


def _unavailable(reason: str) -> JsonObject:
    """An unavailable outcome with its distinct reason."""
    return {"status": "unavailable", "reason": reason}


def _entry(fields: JsonObject) -> JsonObject:
    """One vector converted exactly once, so nested payloads never convert twice."""
    return {_camel(f): item for f, item in _obj(fields).items()}


def _requests() -> list[JsonObject]:
    """The executable and rejected shared request vectors, in contract order."""
    opening = _position("2,0,1")
    return [
        _named("native opening with empty chain counters", answer={"status": "ready"},
               request=_request({**opening, "b2b": SEEDLESS, "combo": SEEDLESS})),
        _named("native rank conditioning is unsupported",
               answer={"status": "unavailable", "reason": "unsupported-conditioning"},
               request={**_request(_position("2,0,1")), "conditioning": {"rank": "s"}}),
        _named("native source rejects fixed sequence semantics",
               answer={"status": "unavailable", "reason": "unsupported-input"},
               request=_request(_position("2,0,1"), replace=True,
                                out={"kind": "fixedSequence", "holdSupported": False,
                                     "extendBeyondSupplied": False})),
        _named("unknown request fields are rejected",
               answer={"status": "unavailable", "reason": "unsupported-input"},
               request=_request(_position("2,0,1"), out={"surprise": True})),
        _named("native hold action plays the held piece with real mechanics",
               answer={"status": "ready"}, request=_request(_position("2,0,1,3", hold=6))),
        _named("native queue extension reports enabled assumptions over the supplied horizon",
               answer={"status": "ready"},
               request=_request(_position("0,1,2,3,4"), out={"queueExtension7bag": True})),
        _named("native topped-out board has no legal continuation",
               answer={"status": "unavailable", "reason": "no-legal-continuation"},
               request=_request(_position("2,0,1", rows=TOPPED_OUT_ROWS))),
    ]


def _cases() -> list[JsonObject]:
    """The contract outcome vectors, in contract order."""
    s2 = _assumptions(0)
    native = _assumptions(1, hold=True, extend=True)
    return [
        _named("s2-fixed complete line", outcome=_ready(_candidate(
            "s2-fixed", [_place(1, 0, 4, 0), _place(2, 1, 5, 1)], "complete", 2,
            [_step(4, 0, 2, 0), _step(2, 3, 1, 1)], EMPTY_ROWS, {"selection_score": 9}, s2,
            shaped=9, raw=6, surge=3))),
        _named("s2-fixed partial line with unavailable gmask", outcome=_ready(_candidate(
            "s2-fixed", [_place(0, 0, 3, 1)], "partial", 5, [_step(1, 0, 1, 0)], PARTIAL_ROWS,
            {"selection_score": 1, "native_composite": STATED_NULL, "policy_logit": STATED_NULL},
            s2, shaped=1, raw=1, surge=0, stated_nulls=True))),
        _named("native-search best variation with unavailable per-step mechanics", outcome=_ready(
            _candidate("native-search", [_place(2, 0, 4, 0, hold=True), _place(1, 0, 0, 0)],
                       "partial", 6, [{}, {}], None,
                       {"native_composite": 12.5, "root_scores": [_root(2, 0, 4, 0, 12.5),
                                                                  _root(5, 1, 7, 2, 9.25)]}, native))),
        _named("cached source miss stays distinguishable", outcome=_unavailable("cache-miss")),
        _named("rank conditioning rejected instead of dropped",
               outcome=_unavailable("unsupported-conditioning")),
        _named("absent policy model reported honestly", outcome=_unavailable("model-absent")),
        _named("execution failure stays distinct from unavailable",
               outcome={"status": "failed", "error": "invalid model artifact"}),
    ]


def build_document() -> JsonObject:
    """Return the complete recommend-v1 contract document."""
    return {**HEADER, "requests": _requests(), "cases": _cases()}


def main() -> None:
    parser = argparse.ArgumentParser(description="Generate the shared recommend-v1 contract vectors.")
    parser.add_argument("output", type=Path, help="path of the JSON file to write")
    args = parser.parse_args()
    args.output.write_text(json.dumps(build_document(), indent=2) + "\n", encoding="utf-8")


if __name__ == "__main__":
    main()
