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

fixture() {
    download "$1" "$2" "$base/test-inputs/fusion/$1/data.json"
}

fixture 1132c53b79dbf7716151ceeb9a690c78a9cbb74cc559489c17ebc74170d5ae70 catalog-mini.json
fixture 656cfa58ebb9dc0ca57985a7a098fa3d75f89d4a48ab84426cb107ed4db4cfab parity-rounds.json
fixture 9a30feea7a3074ac16117c82a92b1d525691df1bfcf56b8c1fc039e9fcb8f490 parity-segments.json
fixture d378719aadb878f04d5d3d0dbd7421fe1ca5a28e9f115b7a15423ab6bde25a71 perf/replay-round-inputs.json
download 05f8d86cf6309daa65039a31b591396459b21246d06d2e81327b49539f566f10 catalog-full.json \
    "$base/releases/6966cc245ca5fc33bd63fd365164a356f4ac3ba08041364aceb4e730721abc95/openers-v2.json"
printf 'Opener test inputs verified\n'
