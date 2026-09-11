#!/bin/sh
set -eu
root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
sh "$root/scripts/hydrate-opener-tests.sh"
mkdir -p "$root/tests/fixtures"
python3 "$root/scripts/generate-recommend-vectors.py" "$root/tests/fixtures/recommend-v1.json"
cd "$root"
export CARGO_BUILD_JOBS=${CARGO_BUILD_JOBS:-4}
export RUST_TEST_THREADS=${RUST_TEST_THREADS:-4}
exec cargo test --lib "$@"
