#!/bin/sh
# 豆腐図式（盤上は玉・歩・と金のみ）煙詰の最大初形枚数を、ビームなしの完全な逆算で求める。
# usage: sh run.sh <sk|dk|dkm> [hand]
#   hand を付けると初形の攻方持駒を許し、盤上＋攻方持駒の枚数を最大化する
#   （bin/fmrs_hand = 720d99c のビルド。出力ディレクトリは <name>_hand）
#   sk  = 単玉
#   dk  = 双玉
#   dkm = 双玉・都詰（受方玉を5五で詰める。4枚の詰上がりも許す）
# bin/fmrs は 23b5b73 のビルド。8時間で打ち切る。
set -eu
cd "$(dirname "$0")"
name=$1
bin=bin/fmrs
dir=$name
handflag=""
if [ "${2:-}" = hand ]; then
  bin=bin/fmrs_hand
  dir=${name}_hand
  handflag="--allow-black-hand"
fi
case "$name" in
  sk) extra="" ;;
  dk) extra="--double-king" ;;
  dkm) extra="--double-king --miyako --mate-square 55" ;;
  *) echo "unknown: $name" >&2; exit 2 ;;
esac
mkdir -p "$dir"
date -Is > "$dir/started_at"
status=0
# shellcheck disable=SC2086
timeout 8h $bin single-king-smoke ideal-backward \
  --parallel 14 --max-memo-entries 10000000 \
  --only-pawn --allow-white-pieces --slack 100 $extra $handflag \
  --random-seed 0 --no-progress-ticker \
  --seed-result-log "$dir/seeds.jsonl" > "$dir/run.log" 2>&1 || status=$?
echo "exit=$status $(date -Is)" > "$dir/finished_at"
exit $status
