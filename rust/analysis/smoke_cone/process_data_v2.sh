#!/usr/bin/env bash
# Turn the v2 data-gen artifacts into a training CSV.
#   edges.bin --edge_value_dp--> values_v2.bin (exact DP labels)
#   per-step best + frontier samples + DP values --analysis--> dataset_v2.csv
#   dataset_v2.csv --cone-features--> train_v2.csv (features + label/group)
set -euo pipefail
cd "$(dirname "$0")/../.."   # rust/ root
DATA="$(pwd)/analysis/smoke_cone/data_v2"

# 1) frontier sizes "<step> <size>" from mem-trace
grep 'advance next_step' "$DATA/run.mem" \
  | sed -E 's/.*next_step=([0-9]+).*frontier_in=([0-9]+).*/\1 \2/' \
  > "$DATA/frontier.txt"
echo "frontier.txt lines: $(wc -l < "$DATA/frontier.txt")"

# 2) exact value-DP labels from recorded edges
python3 analysis/smoke_cone/edge_value_dp.py \
  --edges "$DATA/edges.bin" --out "$DATA/values_v2.bin"

# 3) labeled dataset (best_piece_reachable = DP value, fallback trace)
FMRS_CONE_DATA="$DATA" FMRS_CONE_FRONTIER="$DATA/frontier.txt" \
FMRS_DATASET_OUT="$DATA/dataset_v2.csv" FMRS_EDGE_VALUE_FILE="$DATA/values_v2.bin" \
FMRS_TRACE_DEEP_FROM=21 FMRS_TRACE_CAP=4000 \
  cargo test -p fmrs_core --test smoke_cone_analysis -- --nocapture smoke_cone_analysis 2>&1 | tail -25
echo "dataset_v2.csv rows: $(($(wc -l < "$DATA/dataset_v2.csv") - 1))"

# 4) extract features -> train CSV (label = best_piece_reachable)
./target/release/fmrs single-king-smoke cone-features \
  --dataset "$DATA/dataset_v2.csv" -o "$DATA/train_v2.csv" \
  --label best_piece_reachable
echo "train_v2.csv rows: $(($(wc -l < "$DATA/train_v2.csv") - 1))"
