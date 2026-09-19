#!/bin/sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
destination=${1:-"$root/fixtures/openers"}
base=https://pub-c6a5fdf5991c4b37b12fedd724b38095.r2.dev

verify() {
    if command -v sha256sum >/dev/null 2>&1; then
        printf '%s  %s\n' "$1" "$2" | sha256sum -c - >/dev/null
    else
        printf '%s  %s\n' "$1" "$2" | shasum -a 256 -c - >/dev/null
    fi
}

download() {
    digest=$1
    path="$destination/$2"
    url=$3
    if [ -f "$path" ] && verify "$digest" "$path"; then
        return
    fi
    mkdir -p "$(dirname -- "$path")"
    temporary="$path.$$.tmp"
    trap 'rm -f "$temporary"' EXIT HUP INT TERM
    curl --fail --location --silent --show-error --retry 2 --max-time 120 \
        --proto '=https' --proto-redir '=https' "$url" --output "$temporary"
    verify "$digest" "$temporary"
    mv "$temporary" "$path"
    trap - EXIT HUP INT TERM
    printf 'Hydrated %s\n' "$2"
}

# Runs in a subshell so its staging variables and cleanup trap cannot disturb
# the caller's own staged files or trap.
fetch() (
    digest=$1
    url=$2
    target=$3
    mkdir -p "$(dirname -- "$target")"
    temporary="$target.$$.tmp"
    trap 'rm -f "$temporary"' EXIT HUP INT TERM
    curl --fail --location --silent --show-error --retry 2 --max-time 300 \
        --proto '=https' --proto-redir '=https' "$url" --output "$temporary"
    verify "$digest" "$temporary"
    mv "$temporary" "$target"
    trap - EXIT HUP INT TERM
)

fixture() {
    download "$1" "$2" "$base/test-inputs/fusion/$1/data.json"
}

# The retired dealt-input/tail-queue request fields are gone from the engine's
# strict round decoder, so the pinned input is migrated by dropping exactly
# those two keys. The transform is pinned at both ends: the published input
# digest and the expected output digest.
migration() {
    output_digest=$1
    output="$destination/$2"
    input_digest=$3
    input_url=$4
    if [ -f "$output" ] && verify "$output_digest" "$output"; then
        return
    fi
    mkdir -p "$(dirname -- "$output")"
    temporary="$output.$$.tmp"
    input="$output.$$.input"
    trap 'rm -f "$temporary" "$input"' EXIT HUP INT TERM
    fetch "$input_digest" "$input_url" "$input"
    python3 - "$input" "$temporary" <<'PY'
import json
import sys

with open(sys.argv[1], "rb") as handle:
    rounds = json.load(handle)
for entry in rounds:
    request = entry["input"]
    request.pop("dealtInputs", None)
    request.pop("tailQueue", None)
with open(sys.argv[2], "w", encoding="utf-8") as handle:
    json.dump(rounds, handle, separators=(",", ":"), ensure_ascii=False)
PY
    verify "$output_digest" "$temporary"
    mv "$temporary" "$output"
    rm -f "$input"
    trap - EXIT HUP INT TERM
    printf 'Migrated %s\n' "$2"
}

fixture 1132c53b79dbf7716151ceeb9a690c78a9cbb74cc559489c17ebc74170d5ae70 catalog-mini.json
fixture 656cfa58ebb9dc0ca57985a7a098fa3d75f89d4a48ab84426cb107ed4db4cfab parity-rounds.json
fixture 9a30feea7a3074ac16117c82a92b1d525691df1bfcf56b8c1fc039e9fcb8f490 parity-segments.json
migration c932d2833085eefddad6a3656a456d0da60bf864f8834b86c81bf3ef9dc5a1d9 perf/replay-round-inputs.json \
    d378719aadb878f04d5d3d0dbd7421fe1ca5a28e9f115b7a15423ab6bde25a71 \
    "$base/test-inputs/fusion/d378719aadb878f04d5d3d0dbd7421fe1ca5a28e9f115b7a15423ab6bde25a71/data.json"
# Current public opener release, pinned by its dataset manifest and by the
# asset digest the release manifest declares. The catalog digest is the same
# dataset revision Mosaic's openerDatasetPin names.
download 5abc5168ee0f7d54d4ced9378aa244abfa7c1cda82d4ede08fa9fcff9d7d8fe0 catalog-full.json \
    "$base/releases/0b353d5748c939bf1f367f3fc3f4bf5d93ba6cba04e4a2bea36e93683349ed2f/openers-v2.json"
printf 'Opener test inputs verified\n'
