#!/usr/bin/env bash
# PGO build for direct-cobra-copy perft engine
#
# Three-step:
#   1. Instrumented build, run D5 perft to collect profile data
#   2. Merge .profraw files
#   3. Rebuild with profile data applied
#
# Usage:
#   ./scripts/pgo-build.sh          # full PGO cycle + D5 benchmark
#   ./scripts/pgo-build.sh --d7     # full PGO cycle + D7 benchmark
#
# Requires: rustup component add llvm-tools

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
PGO_DIR="/tmp/cobra-pgo-data"

LLVM_PROFDATA="$(rustc --print sysroot)/lib/rustlib/$(rustc -vV | grep host | cut -d' ' -f2)/bin/llvm-profdata"

if [[ ! -x "$LLVM_PROFDATA" ]]; then
    echo "error: llvm-profdata not found at $LLVM_PROFDATA"
    echo "install: rustup component add llvm-tools"
    exit 1
fi

cd "$PROJECT_ROOT"

echo "step 1: instrumented build + D5 profile collection"
rm -rf "$PGO_DIR"
mkdir -p "$PGO_DIR"

# build instrumented binary
RUSTFLAGS="-Cprofile-generate=$PGO_DIR" \
    cargo build --release --bin perft_cli 2>&1

# run D5 as training workload (exercises generate() heavily)
echo "running D5 training workload..."
RUSTFLAGS="-Cprofile-generate=$PGO_DIR" \
    cargo run --release --bin perft_cli -- 5 2>&1

PROFRAW_COUNT=$(find "$PGO_DIR" -name "*.profraw" | wc -l)
echo "collected $PROFRAW_COUNT profile files"

if [[ "$PROFRAW_COUNT" -eq 0 ]]; then
    echo "error: no .profraw files generated"
    exit 1
fi

echo ""
echo "step 2: merge profiles"
"$LLVM_PROFDATA" merge -o "$PGO_DIR/merged.profdata" "$PGO_DIR"/*.profraw
echo "merged profile: $(wc -c < "$PGO_DIR/merged.profdata") bytes"

echo ""
echo "step 3: PGO-optimized build"
RUSTFLAGS="-Cprofile-use=$PGO_DIR/merged.profdata -Cllvm-args=-pgo-warn-missing-function" \
    cargo build --release --bin bench_perft --bin perft_cli 2>&1

echo ""
echo "benchmark"
if [[ "${1:-}" == "--d7" ]]; then
    echo "running D7 benchmark with PGO..."
    cargo run --release --bin bench_perft 2>&1
else
    echo "running D5 benchmark with PGO..."
    # just run D5 via the test suite for quick comparison
    cargo test --release -- test_d5_accuracy --nocapture 2>&1
fi

echo ""
echo "done. profile at $PGO_DIR/merged.profdata"
