#!/bin/sh
set -eu
cd "$(dirname "$0")"
if [ ! -f ipsj.cls ]; then
  echo "ipsj.cls がありません。README.md の公式配布先から UTF8 版を取得してください。" >&2
  exit 1
fi
xelatex -interaction=nonstopmode -halt-on-error gpw2026_cooperative_mate.tex
xelatex -interaction=nonstopmode -halt-on-error gpw2026_cooperative_mate.tex
