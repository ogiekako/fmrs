#!/usr/bin/env bash
# A/B test: old embedded SOTA vs new v2 GBDT model
# Run 5 random double-king terminal seeds, compare max pieces reached.
set -euo pipefail
cd "$(dirname "$0")/../.."

SEEDS=(
  "1k7/9/1SK6/9/9/9/9/9/9 b 2r2b4g3s4n4l18p 1"
  "2k6/9/2SK5/9/9/9/9/9/9 b 2r2b4g3s4n4l18p 1"
  "1k7/9/KP7/9/9/9/9/9/9 b 2r2b4g4s4n4l17p 1"
  "8k/9/6KS1/9/9/9/9/9/9 b 2r2b4g3s4n4l18p 1"
  "3k5/9/3PK4/9/9/9/9/9/9 b 2r2b4g4s4n4l17p 1"
)

COMMON_ARGS=(
  --parallel 8 --allow-white-pieces --slack 100 --double-king
  --canonicalize-attacker-goldish --min-pawn-pct 44
  --rook-bishop-allow-start 31 --rook-bishop-allow-step 2 --goldish-priority
  --lance-knight-allow-start 8 --lance-knight-allow-step 2
  --beam-width 200000 --beam-width-at 75:1000000 --beam-width-max 1000000
  --memory-budget-pct 15 --seed-result-log /dev/null --max-step 100 --fresh
)

mkdir -p /tmp/ab_test
PIDS=()

for i in "${!SEEDS[@]}"; do
  SFEN="${SEEDS[$i]}"
  echo "seed $i: $SFEN"

  ./target/release/fmrs single-king-smoke ideal-backward \
    "${COMMON_ARGS[@]}" --seed-sfen "$SFEN" \
    --beam-sota \
    > /dev/null 2> /tmp/ab_test/old_${i}.log &
  PIDS+=($!)

  ./target/release/fmrs single-king-smoke ideal-backward \
    "${COMMON_ARGS[@]}" --seed-sfen "$SFEN" \
    --beam-sota --beam-model models/cone_dp_gbdt_v2.json \
    > /dev/null 2> /tmp/ab_test/new_${i}.log &
  PIDS+=($!)
done

echo "Started ${#PIDS[@]} runs. Waiting..."
wait "${PIDS[@]}"
echo "All done."

echo ""
echo "=== A/B Results ==="
printf "%-5s %-6s %-6s %-6s %-6s\n" "seed" "old_p" "new_p" "old_s" "new_s"
for i in "${!SEEDS[@]}"; do
  OLD_BEST=$(grep -oE 'global_best_pieces=[0-9]+' /tmp/ab_test/old_${i}.log | tail -1 | grep -oE '[0-9]+' || echo 0)
  NEW_BEST=$(grep -oE 'global_best_pieces=[0-9]+' /tmp/ab_test/new_${i}.log | tail -1 | grep -oE '[0-9]+' || echo 0)
  OLD_STEP=$(grep -oE 'global_best_pieces=[0-9]+ steps=[0-9]+' /tmp/ab_test/old_${i}.log | tail -1 | grep -oE 'steps=[0-9]+' | grep -oE '[0-9]+' || echo 0)
  NEW_STEP=$(grep -oE 'global_best_pieces=[0-9]+ steps=[0-9]+' /tmp/ab_test/new_${i}.log | tail -1 | grep -oE 'steps=[0-9]+' | grep -oE '[0-9]+' || echo 0)
  printf "%-5s %-6s %-6s %-6s %-6s\n" "$i" "$OLD_BEST" "$NEW_BEST" "$OLD_STEP" "$NEW_STEP"
done
