#!/usr/bin/env bash
# Generate fresh training data (v2) for the beam scorer, in the 44-regime that
# produces the 40-piece smoke. Records:
#   - FMRS_EDGE_FILE         : parent->child edges (for exact value-DP labels)
#   - FMRS_PERSTEP_BEST_DIR  : per-step max-piece positions (discriminative rows)
#   - FMRS_FRONTIER_SAMPLE_DIR: uniform frontier samples (broad negatives)
# Beam guided by current SOTA (--beam-sota), moderate widths (proven 39-config),
# memory self-capped to avoid OOM on the 125GB box (45GB already in use).
set -euo pipefail
cd "$(dirname "$0")/../.."   # rust/ root

DATA="$(pwd)/analysis/smoke_cone/data_v2"
mkdir -p "$DATA"
rm -f "$DATA"/best_step_*.txt "$DATA"/frontier_sample_*.txt "$DATA"/edges.bin

FMRS_EDGE_FILE="$DATA/edges.bin" \
FMRS_PERSTEP_BEST_DIR="$DATA" \
FMRS_FRONTIER_SAMPLE_DIR="$DATA" \
FMRS_FRONTIER_SAMPLE_N="${SAMPLE_N:-20000}" \
  ./target/release/fmrs single-king-smoke ideal-backward \
  --parallel "${PARALLEL:-14}" --allow-white-pieces --slack 100 --double-king \
  --seed-sfen '8G/6K1p/9/3+P2P+p1/4PP3/7kP/7P1/9/9 w P2r2b3g4s4n4l9p 1' \
  --canonicalize-attacker-goldish --min-pawn-pct 44 \
  --rook-bishop-allow-start 31 --rook-bishop-allow-step 2 --goldish-priority \
  --lance-knight-allow-start 8 --lance-knight-allow-step 2 \
  --beam-width 200000 --beam-sota --beam-width-at 75:1500000 --beam-width-max 3000000 \
  --memory-budget-pct "${BUDGET_PCT:-25}" --max-memo-entries 1500000000 \
  --seed-result-log /dev/null --max-step 115 --mem-trace --fresh \
  > "$DATA/run.out" 2> "$DATA/run.mem"

echo "=== DATA-GEN DONE ==="
echo "edges.bin bytes: $(stat -c %s "$DATA/edges.bin" 2>/dev/null || echo NA)"
ls "$DATA"/best_step_*.txt 2>/dev/null | wc -l | sed 's/^/best_step files: /'
grep -oE 'global_best_pieces=[0-9]+ steps=[0-9]+' "$DATA/run.out" | tail -1
