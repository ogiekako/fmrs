#!/usr/bin/env python3
"""エクスポート済み GBDT モデル JSON から、特徴量の構造的寄与度を算出する。

`export_gbdt.py` が出力する `{feature_names, baseline, trees}` 形式（Rust 側の
`smoke_features::GbdtModel` と同じスキーマ）を読み、各特徴が予測値をどれだけ
動かしたかを木構造だけから見積もる。学習データを必要としないため、コミット済み
モデルの寄与度をいつでも再現できる。

指標:
  gain   各内部ノードについて
           |左部分木の平均葉値 - 右部分木の平均葉値| * (部分木の葉数 / 木の葉数)
         を分岐特徴に加算し、全体で正規化したもの。
  split  分岐に使われた回数の割合。
  depthw 浅い分岐ほど重い、2^-depth の重み付き分岐回数の割合。

注意: エクスポート JSON にはノードごとのサンプル数が含まれないため、これは
sklearn の gain / permutation importance そのものではなく、その代理量である。
学習データが手元にあるなら `train_cone_model.py` の permutation_importance を
使うほうが正確。

使い方:
  python3 analysis/smoke_cone/gbdt_structural_importance.py models/beam_sota.json
  python3 analysis/smoke_cone/gbdt_structural_importance.py models/*.json --top 30
"""
from __future__ import annotations

import argparse
import json
from collections import defaultdict
from pathlib import Path

# 木のノード: (feature_idx, threshold, left, right, value, is_leaf)
FEAT, THR, LEFT, RIGHT, VALUE, IS_LEAF = range(6)


def analyze(path: Path):
    model = json.loads(path.read_text())
    names = model["feature_names"]
    gain = defaultdict(float)
    split = defaultdict(float)
    depthw = defaultdict(float)

    for tree in model["trees"]:
        n = len(tree)
        mean = [0.0] * n  # 部分木の葉値の平均
        leaves = [0] * n  # 部分木の葉数

        # 後行順に部分木の平均葉値を畳み上げる（再帰なし）。
        order, stack = [], [(0, False)]
        while stack:
            i, expanded = stack.pop()
            if tree[i][IS_LEAF]:
                mean[i] = tree[i][VALUE]
                leaves[i] = 1
            elif expanded:
                order.append(i)
            else:
                stack.append((i, True))
                stack.append((tree[i][LEFT], False))
                stack.append((tree[i][RIGHT], False))
        for i in order:
            l, r = tree[i][LEFT], tree[i][RIGHT]
            leaves[i] = leaves[l] + leaves[r]
            mean[i] = (mean[l] * leaves[l] + mean[r] * leaves[r]) / leaves[i]

        depth = {0: 0}
        stack = [0]
        while stack:
            i = stack.pop()
            if tree[i][IS_LEAF]:
                continue
            for c in (tree[i][LEFT], tree[i][RIGHT]):
                depth[c] = depth[i] + 1
                stack.append(c)

        total_leaves = leaves[0]
        for i, node in enumerate(tree):
            if node[IS_LEAF]:
                continue
            name = names[node[FEAT]]
            l, r = node[LEFT], node[RIGHT]
            gain[name] += abs(mean[l] - mean[r]) * (leaves[i] / total_leaves)
            split[name] += 1
            depthw[name] += 0.5 ** depth[i]

    return names, gain, split, depthw


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("models", nargs="+", type=Path, help="GBDT モデル JSON")
    parser.add_argument("--top", type=int, default=45, help="表示する上位件数")
    args = parser.parse_args()

    for path in args.models:
        names, gain, split, depthw = analyze(path)
        g_tot = sum(gain.values()) or 1.0
        s_tot = sum(split.values()) or 1.0
        d_tot = sum(depthw.values()) or 1.0
        used = sum(1 for n in names if split[n] > 0)
        print(f"=== {path}  ({len(names)} 特徴中 {used} 個が分岐に使用)")
        print(f"{'':4}{'feature':34}{'gain%':>8}{'split%':>8}{'depthw%':>9}{'cum%':>8}")
        cum = 0.0
        for rank, name in enumerate(sorted(names, key=lambda n: -gain[n])[: args.top], 1):
            cum += 100 * gain[name] / g_tot
            print(
                f"{rank:<4d}{name:34}{100 * gain[name] / g_tot:8.3f}"
                f"{100 * split[name] / s_tot:8.2f}{100 * depthw[name] / d_tot:9.2f}{cum:8.2f}"
            )
        unused = [n for n in names if split[n] == 0]
        print(f"  未使用（定数または完全共線）{len(unused)} 個: {', '.join(unused)}\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
