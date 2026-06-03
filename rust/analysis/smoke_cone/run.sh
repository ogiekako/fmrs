#!/usr/bin/env bash
# Reproduce the smoke best-cone analysis.
#
# 1. Runs single-king-smoke ideal-backward to --max-step (default 37), dumping
#    the max-piece "best" positions at each step (FMRS_PERSTEP_BEST_DIR) and the
#    per-step frontier sizes (mem-trace).
# 2. Runs the analysis test (fmrs_core/tests/smoke_cone_analysis.rs) which
#    reconstructs each best position's unique solution, traces it toward mate,
#    and reports cone-vs-frontier and the "live fraction" per step.
#
# Memory: the frontier grows ~1.7x per 2 steps; --max-step 37 peaks ~60GB.
# Lower MAX_STEP on smaller machines.
set -euo pipefail
cd "$(dirname "$0")/../.."   # repo rust/ root

MAX_STEP="${MAX_STEP:-37}"
PARALLEL="${PARALLEL:-48}"
DATA="$(pwd)/analysis/smoke_cone/data"
SEED='8k/6K+P1/9/9/9/9/9/9/9 w 2r2b4g4s4n4l17p 1'

mkdir -p "$DATA"
rm -f "$DATA"/best_step_*.txt
cargo build --release

FMRS_PERSTEP_BEST_DIR="$DATA" ./target/release/fmrs single-king-smoke ideal-backward \
  --parallel "$PARALLEL" --allow-white-pieces --slack 100 --double-king \
  --seed-sfen "$SEED" --canonicalize-attacker-goldish --min-pawn-pct 60 \
  --rook-bishop-allow-start 31 --rook-bishop-allow-step 2 --goldish-priority \
  --lance-knight-allow-start 8 --lance-knight-allow-step 3 --max-file 7 \
  --seed-result-log /dev/null --max-step "$MAX_STEP" --mem-trace \
  > "$DATA/run.out" 2> "$DATA/run.mem"

# Extract frontier sizes: "<step> <frontier_size>".
grep 'advance next_step' "$DATA/run.mem" \
  | sed -E 's/.*next_step=([0-9]+).*frontier_in=([0-9]+).*/\1 \2/' \
  > "$DATA/frontier.txt"

FMRS_CONE_DATA="$DATA" FMRS_CONE_FRONTIER="$DATA/frontier.txt" \
  cargo test -p fmrs_core --test smoke_cone_analysis -- --nocapture
