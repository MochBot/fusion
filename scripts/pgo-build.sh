#!/usr/bin/env bash
# Profile-Guided Optimization build for fusion-engine
#
# Two-step process:
#   1. Build with instrumentation, run D5 benchmark to collect profile data
#   2. Merge profiles, rebuild with PGO applied
#
# Usage:
#   ./scripts/pgo-build.sh              # full PGO build + D5 benchmark
#   ./scripts/pgo-build.sh --bench-only # skip profiling, just benchmark (requires prior PGO build)
#   ./scripts/pgo-build.sh --test       # run full test suite after PGO build
#
# Results on WSL2 (AMD Ryzen 7 5800X):
#   Before PGO: ~99ms D5
#   After PGO:  ~66ms D5 (-33%)

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
PGO_DIR="/tmp/fusion-pgo-data"

# find llvm-profdata from the rustc toolchain
LLVM_PROFDATA="$(rustc --print sysroot)/lib/rustlib/$(rustc -vV | grep host | cut -d' ' -f2)/bin/llvm-profdata"

if [[ ! -x "$LLVM_PROFDATA" ]]; then
    echo "error: llvm-profdata not found at $LLVM_PROFDATA"
    echo "install the llvm-tools component: rustup component add llvm-tools"
    exit 1
fi

cd "$PROJECT_ROOT"

bench_d5() {
    echo "=== D5 Benchmark ==="
    CLOUD_EXEC_SKIP=1 cargo test -p fusion-engine --release -- test_benchmark_d5 --ignored --nocapture 2>&1 \
        | grep -E "D5:|running|passed"
}

if [[ "${1:-}" == "--bench-only" ]]; then
    bench_d5
    exit 0
fi

echo "=== Step 1: Instrumented build + profile collection ==="
rm -rf "$PGO_DIR"
mkdir -p "$PGO_DIR"

CLOUD_EXEC_SKIP=1 RUSTFLAGS="-Cprofile-generate=$PGO_DIR" \
    cargo test -p fusion-engine --release -- test_benchmark_d5 --ignored --nocapture 2>&1 \
    | grep -E "D5:|running|passed"

PROFRAW_COUNT=$(find "$PGO_DIR" -name "*.profraw" | wc -l)
echo "collected $PROFRAW_COUNT profile files"

if [[ "$PROFRAW_COUNT" -eq 0 ]]; then
    echo "error: no .profraw files generated"
    exit 1
fi

echo ""
echo "=== Step 2: Merge profiles ==="
"$LLVM_PROFDATA" merge -o "$PGO_DIR/merged.profdata" "$PGO_DIR"/*.profraw
echo "merged profile: $(wc -c < "$PGO_DIR/merged.profdata") bytes"

echo ""
echo "=== Step 3: PGO-optimized build + benchmark ==="
CLOUD_EXEC_SKIP=1 RUSTFLAGS="-Cprofile-use=$PGO_DIR/merged.profdata" \
    cargo test -p fusion-engine --release -- test_benchmark_d5 --ignored --nocapture 2>&1 \
    | grep -E "D5:|running|passed"

if [[ "${1:-}" == "--test" ]]; then
    echo ""
    echo "=== Full test suite ==="
    CLOUD_EXEC_SKIP=1 RUSTFLAGS="-Cprofile-use=$PGO_DIR/merged.profdata" \
        cargo test --workspace --release 2>&1 | tail -20
fi

echo ""
echo "done. PGO profile at $PGO_DIR/merged.profdata"
echo "to re-benchmark without re-profiling: $0 --bench-only"
