#!/usr/bin/env python3
"""Train a beam scoring model on the smoke-cone dataset.

Input  : training CSV from `single-king-smoke cone-features`
         (columns: label, group, live_deeper, <feature columns...>).
Targets: `label` = best_piece_reachable (regression).
Output : a LinearModel JSON ({feature_names, weights, intercept}) for the Rust
         beam (`--beam-model`), plus an analysis report to stdout.

Methodology (leakage-safe + ranking-focused):
  * GroupKFold on `group` (= max_best_depth): every position on one best's
    solution path shares a max_best_depth, so grouping keeps a path in one fold.
    Dead rows (group 0) are independent frontier samples -> each gets a unique
    group so they split freely.
  * Beam ranks positions WITHIN a step, so we report per-step Spearman rank
    correlation (the metric that actually matters) alongside global MAE/R2.
  * Ridge (standardized, unscaled back to raw space for the Rust model) and a
    GBDT (HistGradientBoostingRegressor) for nonlinear fit + permutation
    importances ("why does a position live?").

Usage:
  python3 analysis/smoke_cone/train_cone_model.py \
      --csv /tmp/train_light.csv --out models/cone_beam_model.json
"""
import argparse
import json
import sys

import numpy as np
import pandas as pd
from scipy.stats import spearmanr
from sklearn.ensemble import HistGradientBoostingRegressor
from sklearn.inspection import permutation_importance
from sklearn.linear_model import Ridge
from sklearn.metrics import mean_absolute_error, r2_score
from sklearn.model_selection import GroupKFold
from sklearn.preprocessing import StandardScaler

META = ["label", "group", "live_deeper"]


def make_groups(df):
    """max_best_depth as group; dead rows (0) get unique ids."""
    g = df["group"].to_numpy().astype(np.int64).copy()
    dead = g == 0
    g[dead] = np.arange(1_000_000, 1_000_000 + dead.sum())
    return g


def grouped_spearman(keys, y_true, y_pred):
    """Mean (size-weighted) within-group Spearman(pred, true).
    `keys` is a DataFrame of grouping columns (e.g. step, or step+piece)."""
    rhos, weights = [], []
    df = keys.copy()
    df["t"], df["p"] = y_true, y_pred
    for _, grp in df.groupby(list(keys.columns)):
        if len(grp) < 10 or grp["t"].nunique() < 2:
            continue
        rho = spearmanr(grp["p"], grp["t"]).correlation
        if rho == rho:
            rhos.append(rho)
            weights.append(len(grp))
    if not rhos:
        return float("nan")
    return float(np.average(rhos, weights=weights))


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--csv", required=True)
    ap.add_argument("--out", required=True)
    ap.add_argument("--folds", type=int, default=5)
    ap.add_argument("--drop", nargs="*", default=[], help="feature columns to drop (ablation)")
    ap.add_argument("--target", choices=["label", "gain"], default="gain",
                    help="gain = best_piece_reachable - board_total (isolates 'promise' "
                         "above the trivial piece-count floor)")
    args = ap.parse_args()

    df = pd.read_csv(args.csv)
    feat_cols = [c for c in df.columns if c not in META and c not in args.drop]
    # Drop constant columns from the model fit (e.g. heavy feature left at 0).
    nunique = df[feat_cols].nunique()
    const_cols = [c for c in feat_cols if nunique[c] <= 1]
    if const_cols:
        print(f"note: {len(const_cols)} constant feature(s) dropped: {const_cols}", file=sys.stderr)
    fit_cols = [c for c in feat_cols if c not in const_cols]

    X = df[fit_cols].to_numpy(np.float64)
    if args.target == "gain":
        y = (df["label"] - df["board_total"]).to_numpy(np.float64)
    else:
        y = df["label"].to_numpy(np.float64)
    step = df["step"].to_numpy()
    keys_step = df[["step"]]
    keys_cell = df[["step", "board_total"]]  # within (step, piece count)
    groups = make_groups(df)
    print(f"rows={len(df)} features={len(fit_cols)} groups={len(np.unique(groups))} "
          f"target={args.target} y>0={int((y>0).sum())}", file=sys.stderr)

    gkf = GroupKFold(n_splits=args.folds)
    R = {k: [] for k in ["r_mae", "r_step", "r_cell", "g_mae", "g_step", "g_cell"]}
    for tr, te in gkf.split(X, y, groups):
        sc = StandardScaler().fit(X[tr])
        rid = Ridge(alpha=10.0).fit(sc.transform(X[tr]), y[tr])
        pr = rid.predict(sc.transform(X[te]))
        R["r_mae"].append(mean_absolute_error(y[te], pr))
        R["r_step"].append(grouped_spearman(keys_step.iloc[te], y[te], pr))
        R["r_cell"].append(grouped_spearman(keys_cell.iloc[te], y[te], pr))

        gb = HistGradientBoostingRegressor(max_iter=300, learning_rate=0.05,
                                           max_leaf_nodes=31, l2_regularization=1.0)
        gb.fit(X[tr], y[tr])
        pg = gb.predict(X[te])
        R["g_mae"].append(mean_absolute_error(y[te], pg))
        R["g_step"].append(grouped_spearman(keys_step.iloc[te], y[te], pg))
        R["g_cell"].append(grouped_spearman(keys_cell.iloc[te], y[te], pg))

    def stat(a):
        return f"{np.mean(a):.3f} ± {np.std(a):.3f}"

    print(f"\n=== CV (GroupKFold, target={args.target}) ===")
    print("  per-step  = ranking within a step; per-cell = within (step, piece count)")
    print("  per-cell is THE beam-relevant metric: which same-piece positions extend deeper.")
    print(f"Ridge : MAE {stat(R['r_mae'])}  per-step {stat(R['r_step'])}  per-cell {stat(R['r_cell'])}")
    print(f"GBDT  : MAE {stat(R['g_mae'])}  per-step {stat(R['g_step'])}  per-cell {stat(R['g_cell'])}")

    # Fit final GBDT on all data for permutation importance.
    gb = HistGradientBoostingRegressor(max_iter=300, learning_rate=0.05,
                                       max_leaf_nodes=31, l2_regularization=1.0).fit(X, y)
    # Permutation importance on a subsample (speed).
    n = min(20000, len(X))
    idx = np.random.RandomState(0).choice(len(X), n, replace=False)
    imp = permutation_importance(gb, X[idx], y[idx], n_repeats=5, random_state=0,
                                 scoring="r2")
    order = np.argsort(imp.importances_mean)[::-1]
    print("\n=== GBDT permutation importance (top 20) ===")
    for i in order[:20]:
        print(f"  {fit_cols[i]:28s} {imp.importances_mean[i]:.4f}")

    # Final Ridge on all data; export unscaled to raw feature space.
    sc = StandardScaler().fit(X)
    rid = Ridge(alpha=10.0).fit(sc.transform(X), y)
    w_scaled = rid.coef_
    w_raw = w_scaled / sc.scale_
    intercept = float(rid.intercept_ - np.sum(w_scaled * sc.mean_ / sc.scale_))
    print("\n=== Ridge standardized coefficients (top 20 by |w|) ===")
    cstd = sorted(zip(fit_cols, w_scaled), key=lambda t: -abs(t[1]))
    for name, w in cstd[:20]:
        print(f"  {name:28s} {w:+.3f}")

    # The Rust LinearModel needs the FULL feature schema (including dropped/const
    # columns) with weight 0 for the ones we didn't fit.
    full_names = [c for c in df.columns if c not in META]
    wmap = dict(zip(fit_cols, w_raw))
    weights = [float(wmap.get(c, 0.0)) for c in full_names]
    # If trained on gain, make the beam score predict the reachable VALUE itself
    # (= board_total + gain) so it ranks high-piece positions first (across
    # cells) and uses the learned promise as the within-cell tiebreak.
    if args.target == "gain":
        bt = full_names.index("board_total")
        weights[bt] += 1.0
    model = {"feature_names": full_names, "weights": weights, "intercept": intercept}
    with open(args.out, "w") as f:
        json.dump(model, f)
    print(f"\nwrote LinearModel -> {args.out}  ({len(full_names)} features)", file=sys.stderr)


if __name__ == "__main__":
    main()
