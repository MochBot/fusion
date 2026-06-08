#!/usr/bin/env bash
# B3 cross-validation: native label_opp_cand per-candidate exact attack must match
# the TS oracle rank-dump-k7-attack.ts on the same .ctx (sorted per-group multiset).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
FE="$ROOT/fusion-engine"
MF="$ROOT/mosaic-fusion-testing"
PY="$ROOT/valuenet/.venv/bin/python"

SRC="${SRC:-$HOME/.cache/mosaic-k7-eval/scale/xplus30.ctx}"
WORK="${WORK:-$HOME/.cache/mosaic-k7-eval/b3}"
N="${N:-40}"
BEAM="${BEAM:-300}"
K=7

mkdir -p "$WORK"
SMALL="$WORK/in.ctx"
head -c "$((N * 1750))" "$SRC" > "$SMALL"
echo "input: $SMALL ($(( $(stat -f%z "$SMALL") / 1750 )) records), beam=$BEAM"

echo "=== native label_opp_cand ==="
OUT_DIR="$WORK/native" K=$K BEAM=$BEAM MAX_POSITIONS=100000 "$FE/target/release/label_opp_cand" "$SMALL"

echo "=== ts rank-dump-k7-attack.ts ==="
( cd "$MF" && bun run scripts/rank-dump-k7-attack.ts "$SMALL" --out "$WORK/ts" --max-positions 100000 --k $K --beam $BEAM )

echo "=== compare ==="
"$PY" - "$WORK/native" "$WORK/ts" <<'PYEOF'
import json, sys
import numpy as np

nat_dir, ts_dir = sys.argv[1], sys.argv[2]

nm = json.load(open(f"{nat_dir}/groups.json"))
rec, ex_off = nm["record_bytes"], nm["exact_off"]
raw = np.fromfile(f"{nat_dir}/cand.bin", dtype=np.uint8).reshape(-1, rec)
nat_exact = np.ascontiguousarray(raw[:, ex_off:ex_off + 4]).view("<f4").reshape(-1)
ng = nm["groups"]

tm = json.load(open(f"{ts_dir}/groups.json"))
rw, ec = tm["row_width"], tm["exact_attack_col"]
ts = np.fromfile(f"{ts_dir}/cand.bin", dtype="<f4").reshape(-1, rw)
ts_exact = ts[:, ec]
tg = tm["groups"]

if len(ng) != len(tg):
    print(f"GROUP COUNT MISMATCH native={len(ng)} ts={len(tg)}")
    sys.exit(1)

maxdiff = 0.0
bad = 0
for a, b in zip(ng, tg):
    va = np.sort(nat_exact[a["start"]:a["start"] + a["size"]])
    vb = np.sort(ts_exact[b["start"]:b["start"] + b["size"]])
    if len(va) != len(vb):
        bad += 1
        continue
    d = float(np.abs(va - vb).max()) if len(va) else 0.0
    maxdiff = max(maxdiff, d)
    if d > 1e-3:
        bad += 1

print(f"groups={len(ng)} totalCands native={nat_exact.size} ts={ts_exact.size} maxdiff={maxdiff} badGroups={bad}")
sys.exit(0 if bad == 0 and maxdiff <= 1e-3 else 1)
PYEOF
echo "B3 PASS"
