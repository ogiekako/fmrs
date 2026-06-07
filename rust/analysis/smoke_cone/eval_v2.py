#!/usr/bin/env python3
"""Fair head-to-head: NEW GBDT (trained on v2) vs current SOTA model, on the SAME
held-out folds of the v2 data. Metric = within-(step,piece) Spearman of predicted
vs true best_piece_reachable (the beam-relevant "which same-piece position extends
deeper" signal). Also reports per-step Spearman and MAE.

Usage:
  python3 eval_v2.py --csv data_v2/train_v2.csv --sota ../../models/cone_dp_gbdt.json [--folds 5]
"""
import argparse, json, sys
import numpy as np
import pandas as pd
from scipy.stats import spearmanr
from sklearn.ensemble import HistGradientBoostingRegressor
from sklearn.model_selection import GroupKFold

META = ["label", "group", "live_deeper"]


def grouped_spearman(keys: pd.DataFrame, y_true, y_pred):
    df = keys.copy()
    df["t"] = np.asarray(y_true)
    df["p"] = np.asarray(y_pred)
    num = den = 0.0
    for _, g in df.groupby(list(keys.columns)):
        if len(g) < 3 or g["t"].nunique() < 2 or g["p"].nunique() < 2:
            continue
        rho = spearmanr(g["p"], g["t"]).correlation
        if rho == rho:  # not NaN
            num += rho * len(g)
            den += len(g)
    return num / den if den else float("nan")


def load_sota(path):
    m = json.load(open(path))
    return m["feature_names"], float(m["baseline"]), m["trees"]


def sota_predict(feat_names, baseline, trees, df):
    # vectorized per-tree traversal over all rows
    X = df[feat_names].to_numpy(np.float64)
    n = len(X)
    out = np.full(n, baseline, dtype=np.float64)
    for tree in trees:
        t = np.asarray(tree, dtype=np.float64)  # [feat,thr,left,right,value,is_leaf]
        node = np.zeros(n, dtype=np.int64)
        active = np.ones(n, dtype=bool)
        # iterate until all rows reach a leaf
        for _ in range(64):
            feat = t[node, 0].astype(np.int64)
            isleaf = t[node, 5] > 0.5
            done = isleaf & active
            if done.any():
                out[done] += t[node[done], 4]
                active &= ~isleaf
            if not active.any():
                break
            thr = t[node, 1]
            xv = X[np.arange(n), feat]
            go_left = xv <= thr
            nxt = np.where(go_left, t[node, 2], t[node, 3]).astype(np.int64)
            node = np.where(active, nxt, node)
    return out


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--csv", required=True)
    ap.add_argument("--sota", required=True)
    ap.add_argument("--folds", type=int, default=5)
    args = ap.parse_args()

    df = pd.read_csv(args.csv)
    feat_cols = [c for c in df.columns if c not in META]
    X = df[feat_cols].to_numpy(np.float64)
    y = df["label"].to_numpy(np.float64)
    keys_cell = df[["step", "board_total"]]
    keys_step = df[["step"]]

    g = df["group"].to_numpy(np.int64).copy()
    # dead rows (group 0) get unique ids so they split freely
    z = g == 0
    g[z] = np.arange(g.max() + 1, g.max() + 1 + z.sum())
    print(f"rows={len(df)} feats={len(feat_cols)} groups={len(np.unique(g))} "
          f"label range={int(y.min())}..{int(y.max())} y>pieces={(y>df['board_total']).sum()}",
          file=sys.stderr)

    fn, bl, trees = load_sota(args.sota)
    assert set(fn) <= set(feat_cols), "SOTA feature names not subset of csv columns"

    gkf = GroupKFold(n_splits=args.folds)
    res = {k: [] for k in ["new_cell", "sota_cell", "new_step", "sota_step", "new_mae", "sota_mae"]}
    for k, (tr, te) in enumerate(gkf.split(X, y, g)):
        gb = HistGradientBoostingRegressor(max_iter=300, learning_rate=0.05,
                                           max_leaf_nodes=31, l2_regularization=1.0)
        gb.fit(X[tr], y[tr])
        pnew = gb.predict(X[te])
        psota = sota_predict(fn, bl, trees, df.iloc[te])
        res["new_cell"].append(grouped_spearman(keys_cell.iloc[te], y[te], pnew))
        res["sota_cell"].append(grouped_spearman(keys_cell.iloc[te], y[te], psota))
        res["new_step"].append(grouped_spearman(keys_step.iloc[te], y[te], pnew))
        res["sota_step"].append(grouped_spearman(keys_step.iloc[te], y[te], psota))
        res["new_mae"].append(float(np.mean(np.abs(pnew - y[te]))))
        res["sota_mae"].append(float(np.mean(np.abs(psota - y[te]))))
        print(f"fold {k}: new_cell={res['new_cell'][-1]:.3f} sota_cell={res['sota_cell'][-1]:.3f}",
              file=sys.stderr)

    def st(a):
        return f"{np.mean(a):.3f} ± {np.std(a):.3f}"

    print("\n=== v2 held-out comparison (GroupKFold, target=best_piece_reachable) ===")
    print("  per-cell = within (step, piece count): THE beam-relevant metric")
    print(f"NEW  (v2-trained GBDT): per-cell {st(res['new_cell'])}  per-step {st(res['new_step'])}  MAE {st(res['new_mae'])}")
    print(f"SOTA (current model)  : per-cell {st(res['sota_cell'])}  per-step {st(res['sota_step'])}  MAE {st(res['sota_mae'])}")
    d = np.mean(res["new_cell"]) - np.mean(res["sota_cell"])
    print(f"\nΔ per-cell (new - sota) = {d:+.3f}  -> {'NEW better' if d>0 else 'SOTA better or equal'}")


if __name__ == "__main__":
    main()
