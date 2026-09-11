#!/usr/bin/env python3
# /// script
# requires-python = ">=3.11"
# dependencies = ["httpx>=0.27"]
# ///
"""Fresh replay-wave metadata discovery for current-rank X, X+, U, and SS.

Discovery-only by default: paginates the live leaderboard and per-player
league records over direct HTTPS, then writes a reviewable manifest with
per-rank counts, existing-body coverage, and a byte estimate. No replay
bodies are fetched unless --download is passed explicitly.

Wave scope (per delegation): X and X+ take all API-available replays;
U takes the first 2,000 unique replay IDs in leaderboard order (not a
global-newest sort: per-player record pages are consumed sequentially and
the run stops once the target count is reached); SS takes the first 2,000
in leaderboard order as the default.

Resume (--resume --manifest <path>): reuses the supplied manifest's
immutable records.jsonl selection with no re-scan and no new scope, skips
IDs with a prior status-200 ledger entry or a validated body on disk, and
fetches the remainder with bounded concurrency and a flushed ledger.
The manifest must live in <out>/_meta; pass the matching --out for the
manifest (or omit --manifest to resume the newest manifest under <out>).

How to run (from fusion-engine/training, dry-run default fetches no bodies):
  PYTHONPATH="$PWD" uv run python recovered/replay-collector/collect_wave_xxplus_u_ss.py --help
  PYTHONPATH="$PWD" uv run python recovered/replay-collector/collect_wave_xxplus_u_ss.py
"""

from __future__ import annotations

import argparse
import asyncio
import importlib
import json
import shutil
import sys
import time
from datetime import UTC, datetime
from pathlib import Path
from typing import Any

import httpx

SCRIPTS_DIR = Path(__file__).resolve().parent
sys.path.insert(0, str(SCRIPTS_DIR))

collector = importlib.import_module("collect_x_xplus_replays")
wave_selection = importlib.import_module("wave_selection")
make_direct_client = collector.make_direct_client
make_geonode_client = collector.make_geonode_client
download_one = collector.download_one
is_valid_replay_body = collector.is_valid_replay_body
ROTATING_PORTS = collector.ROTATING_PORTS
WaveRef = wave_selection.WaveRef
inventory_existing_bodies = wave_selection.inventory_existing_bodies
split_existing = wave_selection.split_existing
Json = dict[str, Any]

WAVE_RANKS = ("x", "x+", "u", "ss")
DEFAULT_U_TOP_N = 2000
DEFAULT_SS_TOP_N = 2000
HISTORICAL_MEAN_BODY_BYTES = 1_565_429
STAMP = time.strftime("%Y%m%dT%H%M%SZ", time.gmtime())
FUSION_ROOT = Path(__file__).resolve().parents[3]
DEFAULT_WAVE_DIR = Path(__file__).resolve().parents[3] / "data" / "replays-wave-02"
DEFAULT_EXISTING_ROOTS = (
    FUSION_ROOT / "data" / "replays-x-xplus",
    FUSION_ROOT / "data" / "replays-coverage-ssa",
)
AIDATA_PROVENANCE_PATH = "reused_provenance.jsonl"


def log(msg: str) -> None:
    """Print a timestamped driver line to stdout."""
    print(f"[{time.strftime('%Y-%m-%d %H:%M:%S')}] {msg}", flush=True)


def parse_ranks(value: str) -> tuple[str, ...]:
    """Parse a comma-separated rank subset of the wave ranks."""
    ranks = tuple(rank.strip().lower() for rank in value.split(",") if rank.strip())
    if not ranks or any(rank not in WAVE_RANKS for rank in ranks):
        raise argparse.ArgumentTypeError(f"--ranks must be a subset of {','.join(WAVE_RANKS)}")
    return ranks


def parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    """Build the wave driver CLI; discovery-only unless --download is given."""
    parser = argparse.ArgumentParser(description="Fresh X/X+/U/SS replay-wave metadata discovery (dry-run default)")
    parser.add_argument("--ranks", type=parse_ranks, default=WAVE_RANKS)
    parser.add_argument("--u-top-n", type=int, default=DEFAULT_U_TOP_N)
    parser.add_argument("--ss-top-n", type=int, default=DEFAULT_SS_TOP_N)
    parser.add_argument("--record-pages", type=int, default=50)
    parser.add_argument("--leaderboard-pages", type=int, default=2000)
    parser.add_argument("--out", type=Path, default=DEFAULT_WAVE_DIR)
    parser.add_argument("--existing-root", type=Path, action="append", default=[],
                        help="extra corpus root to reuse bodies from (repeatable); "
                             "when given, replaces the project-local defaults")
    parser.add_argument("--download", action="store_true", help="explicitly fetch missing replay bodies")
    parser.add_argument("--use-geonode", action="store_true", help="use rotating GeoNode fallback for replay bodies")
    parser.add_argument("--geonode-stop-file", type=Path, default=None)
    parser.add_argument("--resume", action="store_true",
                        help="resume from a supplied manifest: reuse records.jsonl as-is, no discovery")
    parser.add_argument("--manifest", type=Path, default=None,
                        help="manifest file for --resume (default: newest manifest_*.json under <out>/_meta)")
    parser.add_argument("--download-concurrency", type=int, default=8,
                        help="bounded concurrent body fetches for download/resume (1-32)")
    parser.add_argument("--session-id", default=f"wave-discovery-{STAMP}")
    args = parser.parse_args(argv)
    if args.u_top_n < 1 or args.ss_top_n < 1:
        parser.error("--u-top-n and --ss-top-n must be positive")
    if args.record_pages < 1 or args.leaderboard_pages < 1:
        parser.error("--record-pages and --leaderboard-pages must be positive")
    if args.download_concurrency < 1 or args.download_concurrency > 32:
        parser.error("--download-concurrency must be 1-32")
    if args.resume and args.manifest is not None and not args.manifest.exists():
        parser.error("--manifest must point at an existing manifest file")
    return args




wave_discovery = importlib.import_module("wave_discovery")
fetch_leaderboard_players = wave_discovery.fetch_leaderboard_players
discover_rank_refs = wave_discovery.discover_rank_refs
fetch_record_page = wave_discovery.fetch_record_page
def write_wave_meta(wave_dir: Path, leaderboard: list[Json], candidates: list[WaveRef], manifest: Json) -> Path:
    """Persist leaderboard, candidate refs, and manifest without touching existing corpora."""
    meta = wave_dir / "_meta"
    meta.mkdir(parents=True, exist_ok=True)
    with (meta / "leaderboard.jsonl").open("w") as handle:
        for entry in leaderboard:
            handle.write(json.dumps(entry) + "\n")
    with (meta / "records.jsonl").open("w") as handle:
        for ref in candidates:
            handle.write(json.dumps({"replayid": ref.replayid, "record_id": ref.record_id,
                                     "player_id": ref.player_id, "rank": ref.rank, "ts": ref.ts}) + "\n")
    manifest_path = meta / f"manifest_{STAMP}.json"
    manifest_path.write_text(json.dumps(manifest, indent=2))
    return manifest_path


async def run_discovery(args: argparse.Namespace) -> tuple[Json, Path]:
    """Run fresh leaderboard + records discovery and write the reviewable manifest."""
    wave_dir: Path = args.out
    existing_roots = resolve_existing_roots(wave_dir, list(args.existing_root))
    top_n_by_rank = {"u": args.u_top_n, "ss": args.ss_top_n}
    client = make_direct_client(session_id=args.session_id)
    try:
        players_by_rank, lb_report = await fetch_leaderboard_players(client, args.ranks, args.leaderboard_pages)
        manifest_ranks: Json = {}
        all_candidates: list[WaveRef] = []
        all_selected: list[WaveRef] = []
        existing = inventory_existing_bodies(existing_roots)
        pagination_complete = bool(lb_report["complete"])
        for rank in args.ranks:
            entries = players_by_rank[rank]
            top_n = top_n_by_rank.get(rank)
            selected, rep = await discover_rank_refs(client, rank, entries, top_n, args.record_pages)
            all_candidates.extend(selected)
            all_selected.extend(selected)
            on_disk, missing = split_existing(selected, existing)
            manifest_ranks[rank] = {**rep, "candidates": len(selected),
                                    "selected_unique": len({r.replayid for r in selected}),
                                    "existing": len(on_disk), "missing": len(missing)}
            pagination_complete = pagination_complete and bool(rep["complete"])
            log(f"rank {rank}: players={len(entries)} selected={len(selected)} existing={len(on_disk)} missing={len(missing)}")
    finally:
        await client.aclose()
    unique_selected = {ref.replayid for ref in all_selected}
    missing_unique = sorted(rid for rid in unique_selected if rid not in existing)
    manifest: Json = {
        "wave": "xxplus-u-ss-01", "created_at": datetime.now(UTC).isoformat(),
        "command": sys.argv, "ranks_requested": list(args.ranks),
        "leaderboard": lb_report, "ranks": manifest_ranks,
        "unique_selected": len(unique_selected), "missing_unique": len(missing_unique),
        "byte_estimate": len(missing_unique) * HISTORICAL_MEAN_BODY_BYTES,
        "pagination_complete": pagination_complete,
        "download_launched": False,
        "launch_command": " ".join([*sys.argv, "--download"]),
    }
    leaderboard_entries = [e for rank in args.ranks for e in players_by_rank[rank]]
    manifest_path = write_wave_meta(wave_dir, leaderboard_entries, all_candidates, manifest)
    return manifest, manifest_path


RESUME_BODY_MIN_BYTES = 10_000
RESUME_PROGRESS_EVERY = 20
RESUME_PROGRESS_INTERVAL_S = 30.0
PROXY_PROBE_TIMEOUT_S = 20.0


def resolve_resume_manifest(wave_dir: Path, explicit: Path | None) -> Path:
    """Resolve the immutable manifest for --resume without any network scan."""
    if explicit is not None:
        return explicit
    candidates = sorted((wave_dir / "_meta").glob("manifest_*.json"))
    if not candidates:
        raise FileNotFoundError(f"no manifest_*.json under {wave_dir / '_meta'}; pass --manifest explicitly")
    return candidates[-1]


def check_resume_manifest_scope(wave_dir: Path, manifest_path: Path) -> None:
    """Fail closed unless the selected manifest lives in <out>/_meta.

    Resume reuses the manifest's sibling records.jsonl selection, so a
    manifest from another wave directory must not combine with this run's
    --out records, ledgers, or bodies. Compare resolved paths so
    dot-dot/spelling aliases of the same directory still match.
    """
    expected = (wave_dir / "_meta").resolve()
    actual = manifest_path.resolve().parent
    if actual != expected:
        raise ValueError(
            f"resume manifest {manifest_path} is outside {expected}; "
            f"pass the matching --out for that manifest (or omit --manifest to "
            f"resume the newest manifest under {expected})"
        )


def load_resume_wanted(wave_dir: Path) -> list[str]:
    """Load the immutable records.jsonl selection in file order, deduped."""
    records_path = wave_dir / "_meta" / "records.jsonl"
    wanted: list[str] = []
    for line in records_path.read_text().splitlines():
        if line.strip():
            wanted.append(str(json.loads(line)["replayid"]))
    return list(dict.fromkeys(wanted))


def ledger_succeeded_ids(wave_dir: Path) -> set[str]:
    """Replay IDs with a prior status-200 ledger entry (preserved across resumes)."""
    succeeded: set[str] = set()
    log_path = wave_dir / "_meta" / "downloads.jsonl"
    if not log_path.exists():
        return succeeded
    for line in log_path.read_text().splitlines():
        if not line.strip():
            continue
        try:
            entry = json.loads(line)
        except ValueError:
            continue
        if entry.get("status") == 200 and entry.get("replayid"):
            succeeded.add(str(entry["replayid"]))
    return succeeded


def validated_body_ids(wave_dir: Path, candidates: list[str]) -> set[str]:
    """Candidate body IDs with a present, plausibly-sized, schema-valid body.

    Cheap size gate first; only survivors pay for a JSON validation parse.
    Corrupt survivors are removed so the caller re-fetches them.
    """
    bodies_dir = wave_dir / "bodies"
    valid: set[str] = set()
    for rid in candidates:
        out = bodies_dir / f"{rid}.ttrm"
        try:
            if not out.exists() or out.stat().st_size < RESUME_BODY_MIN_BYTES:
                continue
            if is_valid_replay_body(out.read_bytes()):
                valid.add(rid)
            else:
                out.unlink(missing_ok=True)
        except OSError:
            continue
    return valid


def reuse_cross_corpus_bodies(
    wave_dir: Path, candidates: list[str], existing_roots: list[Path]
) -> tuple[set[str], list[Json]]:
    """Copy validated cross-corpus bodies for wanted IDs into the wave bodies dir.

    Filename-stem inventory only (no corpus-wide content parse); only IDs in
    ``candidates`` pay for a size gate plus schema validation, and only those
    are copied. objects-noid roots are skipped: their stems are content hashes,
    not replay IDs, so stem matching cannot apply. Returns reused IDs plus
    provenance records mapping each replay ID to its source body path.
    """
    bodies_dir = wave_dir / "bodies"
    bodies_dir.mkdir(parents=True, exist_ok=True)
    destination_root = bodies_dir.resolve()
    stem_to_source: dict[str, Path] = {}
    for root in existing_roots:
        if not root.exists() or root.resolve() == destination_root:
            continue
        if root.name == "objects-noid":
            continue
        try:
            for path in root.glob("**/*.ttrm"):
                if path.resolve().is_relative_to(destination_root):
                    continue
                if path.stem not in stem_to_source:
                    stem_to_source[path.stem] = path
        except OSError:
            continue
    reused: set[str] = set()
    provenance: list[Json] = []
    for rid in dict.fromkeys(candidates):
        source = stem_to_source.get(rid)
        if source is None:
            continue
        try:
            if source.stat().st_size < RESUME_BODY_MIN_BYTES:
                continue
            if not is_valid_replay_body(source.read_bytes()):
                continue
            shutil.copy2(source, bodies_dir / f"{rid}.ttrm")
        except OSError:
            continue
        reused.add(rid)
        provenance.append({"replayid": rid, "source": str(source)})
    if provenance:
        meta = wave_dir / "_meta"
        meta.mkdir(parents=True, exist_ok=True)
        with (meta / AIDATA_PROVENANCE_PATH).open("a") as handle:
            for record in provenance:
                handle.write(json.dumps(record) + "\n")
    return reused, provenance


def log_aidata_corpora_names(wave_dir: Path) -> None:
    """Log sibling corpora directory names only; never scan their contents."""
    try:
        names = sorted(path.name for path in wave_dir.parent.iterdir())
    except OSError:
        names = []
    log(f"aidata corpora (names only, no content scan): {names}")


def classify_proxy_error(error: BaseException) -> str:
    """Sanitized proxy failure class: exception type only, never URL/credential text."""
    return type(error).__name__


async def diagnose_proxy_once(session_id: str, probe_replayid: str) -> tuple[bool, str]:
    """Probe the configured proxy with one bounded replay-body request.

    Returns (working, classification). The classification is a sanitized
    token such as "ok-direct", "ProxyError", "timeout", "ConnectError", or
    "proxy_config_missing" — never a raw message, URL, or credential.
    """
    try:
        probe_client = make_geonode_client(ROTATING_PORTS[0], session_id=session_id)
    except RuntimeError:
        return False, "proxy_config_missing"
    url = collector.INOUE_REPLAY.format(replayid=probe_replayid)
    try:
        async with probe_client.stream("GET", url, timeout=PROXY_PROBE_TIMEOUT_S) as response:
            chunk_count = 0
            async for _chunk in response.aiter_bytes():
                chunk_count += 1
                break
            if response.status_code == 200:
                return True, "ok"
            return False, f"http_{response.status_code}"
    except httpx.ProxyError as error:
        return False, classify_proxy_error(error)
    except httpx.TimeoutException as error:
        return False, classify_proxy_error(error)
    except (httpx.ConnectError, httpx.RemoteProtocolError) as error:
        return False, classify_proxy_error(error)
    except Exception as error:
        return False, classify_proxy_error(error)
    finally:
        await probe_client.aclose()


async def download_queue_concurrent(
    args: argparse.Namespace,
    queue: list[str],
    geonode_clients: dict[int, Any] | None,
    *,
    resume_manifest_path: Path | None = None,
) -> Json:
    """Fetch one body per queued ID with bounded concurrency and a flushed ledger."""
    wave_dir: Path = args.out
    meta = wave_dir / "_meta"
    direct = make_direct_client(session_id=args.session_id)
    stop_file = args.geonode_stop_file or meta / "STOP_GEONODE"
    gate = asyncio.Semaphore(getattr(args, "download_concurrency", 8))
    log_path = meta / "downloads.jsonl"
    attempted = 0
    succeeded = 0
    failed = 0
    bytes_in = 0
    bytes_raw = 0
    bytes_decoded = 0
    via_counter: dict[str, int] = {}
    started = time.time()
    last_progress = started
    (wave_dir / "bodies").mkdir(parents=True, exist_ok=True)

    async def fetch_one(rid: str) -> Any:
        out = wave_dir / "bodies" / f"{rid}.ttrm"
        outcome = await download_one(gate, direct, geonode_clients, rid, out, stop_file)
        if outcome.status == 200:
            try:
                if not is_valid_replay_body(out.read_bytes()):
                    out.unlink(missing_ok=True)
                    outcome.status = None
                    outcome.err = "invalid_body"
                    outcome.bytes_in = 0
                    outcome.bytes_decoded = 0
            except OSError:
                outcome.status = None
                outcome.err = "body_read_failed"
                outcome.bytes_in = 0
                outcome.bytes_decoded = 0
        return outcome

    try:
        tasks = [asyncio.create_task(fetch_one(rid)) for rid in queue]
        with log_path.open("a") as handle:
            for fut in asyncio.as_completed(tasks):
                outcome = await fut
                attempted += 1
                bytes_in += outcome.bytes_in
                bytes_raw += outcome.bytes_raw
                bytes_decoded += outcome.bytes_decoded
                via_counter[outcome.via] = via_counter.get(outcome.via, 0) + 1
                if outcome.status == 200:
                    succeeded += 1
                else:
                    failed += 1
                handle.write(json.dumps({"replayid": outcome.replayid, "status": outcome.status,
                                         "bytes": outcome.bytes_in,
                                         "bytes_raw": outcome.bytes_raw,
                                         "bytes_decoded": outcome.bytes_decoded,
                                         "content_encoding": outcome.content_encoding,
                                         "err": outcome.err,
                                         "via": outcome.via, "port": outcome.port,
                                         "attempts": outcome.attempts}) + "\n")
                handle.flush()
                now = time.time()
                if attempted % RESUME_PROGRESS_EVERY == 0 or now - last_progress >= RESUME_PROGRESS_INTERVAL_S:
                    elapsed = now - started
                    rate = succeeded / elapsed if elapsed > 0 else 0.0
                    log(f"download progress: attempted={attempted}/{len(queue)} "
                        f"success={succeeded} fail={failed} "
                        f"raw_mib={bytes_raw / 1_048_576:.1f} "
                        f"decoded_mib={bytes_decoded / 1_048_576:.1f} rate={rate:.2f}/s")
                    last_progress = now
    finally:
        await direct.aclose()
        if geonode_clients is not None:
            for client in geonode_clients.values():
                await client.aclose()
    elapsed = time.time() - started
    summary: Json = {
        "attempted": attempted, "success": succeeded, "failed": failed,
        "bytes_in": bytes_in, "bytes_raw": bytes_raw, "bytes_decoded": bytes_decoded,
        "seconds": round(elapsed, 1),
        "concurrency": getattr(args, "download_concurrency", 8),
        "via": dict(via_counter),
        "resume_manifest": str(resume_manifest_path) if resume_manifest_path else None,
    }
    log(f"download done: attempted={attempted} success={succeeded} fail={failed} "
        f"raw_mib={bytes_raw / 1_048_576:.1f} decoded_mib={bytes_decoded / 1_048_576:.1f} "
        f"seconds={elapsed:.0f}s")
    return summary


def resolve_existing_roots(wave_dir: Path, explicit: list[Path]) -> list[Path]:
    """Resolve corpus-reuse roots: explicit --existing-root wins, else project-local defaults."""
    if explicit:
        return [wave_dir, *explicit]
    return [wave_dir, *DEFAULT_EXISTING_ROOTS]


async def run_download(args: argparse.Namespace, manifest: Json) -> None:
    """Fetch missing bodies for manifest-listed IDs, reusing the base downloader."""
    wave_dir: Path = args.out
    meta = wave_dir / "_meta"
    wanted: list[str] = []
    for line in (meta / "records.jsonl").read_text().splitlines():
        if line.strip():
            wanted.append(str(json.loads(line)["replayid"]))
    existing = inventory_existing_bodies(resolve_existing_roots(wave_dir, list(args.existing_root)))
    queue = [rid for rid in dict.fromkeys(wanted) if rid not in existing]
    log(f"download queue: {len(queue)} missing bodies")
    geonode_clients = (
        {port: make_geonode_client(port, session_id=args.session_id) for port in ROTATING_PORTS}
        if args.use_geonode
        else None
    )
    try:
        await download_queue_concurrent(args, queue, geonode_clients)
    finally:
        pass


async def run_resume(args: argparse.Namespace) -> tuple[Json, Path]:
    """Resume body downloads from the supplied manifest; discovery never runs."""
    wave_dir: Path = args.out
    manifest_path = resolve_resume_manifest(wave_dir, args.manifest)
    check_resume_manifest_scope(wave_dir, manifest_path)
    manifest: Json = json.loads(manifest_path.read_text())
    log_aidata_corpora_names(wave_dir)
    wanted = load_resume_wanted(wave_dir)
    preserved = ledger_succeeded_ids(wave_dir)
    with_body = validated_body_ids(wave_dir, wanted)
    ledger_missing = sorted(preserved - with_body)
    # Local valid bodies are already covered by validated_body_ids; keep the
    # wave dir out of cross-corpus reuse roots so a partial local stub cannot
    # shadow (first stem match wins) a valid body in an explicit root.
    existing_roots = [root for root in resolve_existing_roots(wave_dir, list(args.existing_root))
                      if root.resolve() != wave_dir.resolve()]
    missing = [rid for rid in wanted if rid not in with_body]
    reused, _provenance = reuse_cross_corpus_bodies(wave_dir, missing, existing_roots)
    skipped = with_body | reused
    queue = [rid for rid in wanted if rid not in skipped]
    log(f"resume manifest: {manifest_path} unique_wanted={len(wanted)} "
        f"preserved_ledger={len(preserved)} valid_bodies={len(with_body - preserved)} "
        f"ledger_missing_body={len(ledger_missing)} reused_cross_corpus={len(reused)} "
        f"queue={len(queue)}")
    geonode_clients: dict[int, Any] | None = None
    if args.use_geonode and queue:
        working, classification = await diagnose_proxy_once(args.session_id, queue[0])
        if working:
            geonode_clients = (
                {port: make_geonode_client(port, session_id=args.session_id) for port in ROTATING_PORTS}
            )
            log("proxy probe: ok, proxy fallback enabled")
        else:
            log(f"proxy probe: {classification}, continuing direct-only "
                f"(blocked: proxy unavailable, no repeated proxy attempts)")
    elif args.use_geonode:
        log("proxy probe: skipped, empty queue")
    summary = await download_queue_concurrent(
        args, queue, geonode_clients, resume_manifest_path=manifest_path)
    summary["reused_cross_corpus"] = len(reused)
    summary["reused_ids"] = sorted(reused)
    summary["ledger_missing_body"] = ledger_missing
    summary["ledger_missing_body_count"] = len(ledger_missing)
    manifest["resume_summary"] = summary
    return manifest, manifest_path


async def amain(args: argparse.Namespace) -> tuple[Json, Path]:
    """Discover metadata, write the manifest, and download only when asked."""
    if getattr(args, "resume", False):
        return await run_resume(args)
    manifest, manifest_path = await run_discovery(args)
    if args.download:
        manifest["download_launched"] = True
        manifest_path.write_text(json.dumps(manifest, indent=2))
        await run_download(args, manifest)
    else:
        log(f"dry-run: manifest at {manifest_path}, no bodies fetched "
            f"(missing={manifest['missing_unique']}, estimate={manifest['byte_estimate']} bytes)")
    return manifest, manifest_path


def main(argv: list[str] | None = None) -> tuple[Json, Path]:
    """CLI entry: parse args and run the wave driver."""
    return asyncio.run(amain(parse_args(argv)))


if __name__ == "__main__":
    main()
