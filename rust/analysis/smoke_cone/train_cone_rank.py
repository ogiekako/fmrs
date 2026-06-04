#!/usr/bin/env python3
"""Learning-to-rank beam scorer (LambdaRank) — direction 1.

The regression model wastes capacity fitting the piece-count floor and
saturates at the data's max reachable value. The beam only needs a good
*ordering* of the frontier per step, so we optimize that directly: LightGBM
LambdaRank with one query per `step`, relevance = best_piece_reachable. NDCG
focuses on getting the TOP positions right — exactly what the beam's top-K keeps.

Evaluated leakage-safe (GroupKFold on max_best_depth, so a whole solution path
stays in one fold) with the beam-relevant metrics: per-step and per-(step,piece)
Spearman. Exports the trees to the Rust GbdtModel JSON (BeamScorer::Gbdt),
verified against lightgbm's raw prediction.

Usage:
  python3 analysis/smoke_cone/train_cone_rank.py --csv train44.csv \
      --out models/cone_44_rank.json
"""
import argparse
import json
import sys

import lightgbm as lgb
import numpy as np
import pandas as pd
from scipy.stats import spearmanr
from sklearn.model_selection import GroupKFold

META = ["label", "group", "live_deeper"]


def make_groups(df):
    g = df["group"].to_numpy().astype(np.int64).copy()
    dead = g == 0
    g[dead] = np.arange(1_000_000, 1_000_000 + dead.sum())
    return g


def grouped_spearman(keys, y_true, y_pred):
    rhos, w = [], []
    d = keys.copy()
    d["t"], d["p"] = y_true, y_pred
    for _, grp in d.groupby(list(keys.columns)):
        if len(grp) < 10 or grp["t"].nunique() < 2:
            continue
        r = spearmanr(grp["p"], grp["t"]).correlation
        if r == r:
            rhos.append(r)
            w.append(len(grp))
    return float(np.average(rhos, weights=w)) if rhos else float("nan")


def lgb_query_sizes(step_arr):
    """LightGBM group sizes: rows must be sorted by query (step) already."""
    _, counts = np.unique(step_arr, return_counts=True)
    return counts


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--csv", required=True)
    ap.add_argument("--out", required=True)
    ap.add_argument("--folds", type=int, default=5)
    ap.add_argument("--leaves", type=int, default=31)
    ap.add_argument("--trees", type=int, default=400)
    ap.add_argument("--group", choices=["step", "cell"], default="step",
                    help="lambdarank query: step (full per-step ranking) or "
                         "cell (within step+piece-count; optimizes the "
                         "discriminative within-cell order)")
    args = ap.parse_args()

    df = pd.read_csv(args.csv)
    # LightGBM lambdarank caps a query (here = one step) at 10000 rows. Subsample
    # each step so train folds stay under the limit.
    keep = []
    for _, g in df.groupby("step"):
        keep.append(g.sample(n=min(len(g), 9000), random_state=0).index.to_numpy())
    df = df.loc[np.concatenate(keep)].reset_index(drop=True)
    feat_cols = [c for c in df.columns if c not in META]
    X = df[feat_cols].to_numpy(np.float64)
    rel = df["label"].to_numpy(np.int32)  # relevance = best_piece_reachable
    step = df["step"].to_numpy()
    # Query id for lambdarank grouping.
    if args.group == "cell":
        qid = step.astype(np.int64) * 1000 + df["board_total"].to_numpy().astype(np.int64)
    else:
        qid = step.astype(np.int64)
    groups = make_groups(df)
    keys_step = df[["step"]]
    keys_cell = df[["step", "board_total"]]
    max_rel = int(rel.max())
    label_gain = list(range(max_rel + 1))  # linear gains (avoid 2^rel overflow)
    print(f"rows={len(df)} features={len(feat_cols)} max_rel={max_rel}", file=sys.stderr)

    params = dict(
        objective="lambdarank", n_estimators=args.trees, learning_rate=0.05,
        num_leaves=args.leaves, min_child_samples=50, subsample=0.8,
        colsample_bytree=0.8, label_gain=label_gain, verbose=-1,
    )

    gkf = GroupKFold(n_splits=args.folds)
    step_sp, cell_sp = [], []
    for tr, te in gkf.split(X, rel, groups):
        # LightGBM ranking needs rows grouped by query; sort train by step.
        order = np.argsort(qid[tr], kind="stable")
        tri = tr[order]
        m = lgb.LGBMRanker(**params)
        m.fit(X[tri], rel[tri], group=lgb_query_sizes(qid[tri]))
        pr = m.predict(X[te])
        step_sp.append(grouped_spearman(keys_step.iloc[te], rel[te], pr))
        cell_sp.append(grouped_spearman(keys_cell.iloc[te], rel[te], pr))

    def stat(a):
        return f"{np.mean(a):.3f} ± {np.std(a):.3f}"

    print("\n=== LambdaRank CV (GroupKFold) ===")
    print(f"per-step Spearman {stat(step_sp)}   per-cell Spearman {stat(cell_sp)}")
    print("(compare regression GBDT: per-cell ~0.30-0.32)")

    # Final fit on all data (sorted by step) + importance.
    order = np.argsort(qid, kind="stable")
    final = lgb.LGBMRanker(**params)
    final.fit(X[order], rel[order], group=lgb_query_sizes(qid[order]))
    imp = final.feature_importances_
    top = np.argsort(imp)[::-1][:20]
    print("\n=== feature importance (gain, top 20) ===")
    for i in top:
        print(f"  {feat_cols[i]:28s} {imp[i]}")

    # Export trees to the Rust GbdtModel format.
    dumped = final.booster_.dump_model()
    trees = []
    for t in dumped["tree_info"]:
        flat = []

        def add(node):
            idx = len(flat)
            flat.append(None)
            if "leaf_value" in node:
                flat[idx] = [0, 0.0, 0, 0, float(node["leaf_value"]), 1]
                return idx
            feat = int(node["split_feature"])
            thr = float(node["threshold"])
            # LightGBM: decision "<=" goes to left_child.
            left = add(node["left_child"])
            right = add(node["right_child"])
            flat[idx] = [feat, thr, left, right, 0.0, 0]
            return idx

        add(t["tree_structure"])
        trees.append([tuple(n) for n in flat])

    def traverse(tree, x):
        i = 0
        while True:
            feat, thr, left, right, value, is_leaf = tree[i]
            if is_leaf:
                return value
            i = left if x[feat] <= thr else right

    idx = np.random.RandomState(0).choice(len(X), 2000, replace=False)
    mine = np.array([sum(traverse(t, X[i]) for t in trees) for i in idx])
    lg = final.predict(X[idx])
    err = float(np.max(np.abs(mine - lg)))
    print(f"\nmax |mine - lgb.predict| = {err:.6g}", file=sys.stderr)
    assert err < 1e-3, "tree traversal mismatch"

    model = {"feature_names": feat_cols, "baseline": 0.0, "trees": trees}
    json.dump(model, open(args.out, "w"))
    print(f"wrote {args.out}: {len(trees)} trees", file=sys.stderr)


if __name__ == "__main__":
    main()
