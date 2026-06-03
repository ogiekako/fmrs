#!/usr/bin/env python3
"""Train a HistGradientBoosting regressor on the cone training CSV and export it
as a portable JSON the Rust beam can evaluate (BeamScorer::Gbdt).

Target = `label` (best_piece_reachable): the beam scores predicted reachable
value directly, so it ranks high-piece survivors first (across cells) and uses
the learned within-cell promise as the tiebreak — without the linear model's
ceiling (per-cell Spearman ~0.20 vs GBDT ~0.32).

The trees are fit on ALL feature columns (in `feature_names()` order, no column
dropping) so each node's `feature_idx` indexes directly into the Rust feature
vector. Output JSON: {feature_names, baseline, trees:[[ [feat,thr,left,right,
value,is_leaf], ... ], ...]}. A node with is_leaf=1 returns `value`; otherwise
go `left` if x[feat] <= thr else `right`. Prediction = baseline + Σ leaf values.
"""
import argparse
import json
import sys

import numpy as np
import pandas as pd
from sklearn.ensemble import HistGradientBoostingRegressor

META = ["label", "group", "live_deeper"]


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--csv", required=True)
    ap.add_argument("--out", required=True)
    ap.add_argument("--max-iter", type=int, default=300)
    ap.add_argument("--lr", type=float, default=0.05)
    ap.add_argument("--leaves", type=int, default=31)
    args = ap.parse_args()

    df = pd.read_csv(args.csv)
    feat_cols = [c for c in df.columns if c not in META]
    X = df[feat_cols].to_numpy(np.float64)
    y = df["label"].to_numpy(np.float64)
    print(f"rows={len(df)} features={len(feat_cols)}", file=sys.stderr)

    m = HistGradientBoostingRegressor(
        max_iter=args.max_iter, learning_rate=args.lr,
        max_leaf_nodes=args.leaves, l2_regularization=1.0,
    ).fit(X, y)

    baseline = float(np.ravel(m._baseline_prediction)[0])
    trees = []
    for it in m._predictors:
        nodes = it[0].nodes
        tree = []
        for n in nodes:
            tree.append([
                int(n["feature_idx"]),
                float(n["num_threshold"]),
                int(n["left"]),
                int(n["right"]),
                float(n["value"]),
                int(n["is_leaf"]),
            ])
        trees.append(tree)

    # Verify our traversal matches sklearn on a sample.
    def traverse(tree, x):
        i = 0
        while True:
            feat, thr, left, right, value, is_leaf = tree[i]
            if is_leaf:
                return value
            i = left if x[feat] <= thr else right

    idx = np.random.RandomState(0).choice(len(X), 2000, replace=False)
    mine = np.array([baseline + sum(traverse(t, X[i]) for t in trees) for i in idx])
    skl = m.predict(X[idx])
    maxerr = float(np.max(np.abs(mine - skl)))
    print(f"max |mine - sklearn.predict| = {maxerr:.6g}", file=sys.stderr)
    assert maxerr < 1e-4, "tree traversal mismatch"

    model = {"feature_names": feat_cols, "baseline": baseline, "trees": trees}
    with open(args.out, "w") as f:
        json.dump(model, f)
    n_nodes = sum(len(t) for t in trees)
    print(f"wrote {args.out}: {len(trees)} trees, {n_nodes} nodes", file=sys.stderr)


if __name__ == "__main__":
    main()
