#!/usr/bin/env python3
"""
train_beam_model.py — Fit a linear scoring model for single-king-smoke
beam search.

Input  : CSV produced by `cargo run --release -- single-king-smoke
         export-features`.
Output : JSON {feature_names, weights, intercept} compatible with
         smoke_features::LinearModel::load.

Usage:
    python3 scripts/train_beam_model.py \
        --csv path/to/training.csv \
        --out target/beam_model.json \
        [--standardize] [--alpha 1.0] [--gbdt-importance]

The default model is Ridge regression with alpha=1.0; standardize=True
centers and scales features before fitting (recommended). The CSV's
`seed_index` and `step` columns are dropped from the feature set;
`label` is the regression target.
"""
from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--csv", required=True, type=Path, help="training CSV")
    parser.add_argument("--out", required=True, type=Path, help="model JSON output path")
    parser.add_argument("--alpha", type=float, default=1.0, help="Ridge alpha (default 1.0)")
    parser.add_argument(
        "--standardize",
        action="store_true",
        help="Standardize features (mean/std) before fitting; weights are unscaled before output.",
    )
    parser.add_argument(
        "--gbdt-importance",
        action="store_true",
        help="Also fit a GBDT and print feature importances (for analysis).",
    )
    parser.add_argument(
        "--include-step", action="store_true", help="Include step as a feature."
    )
    args = parser.parse_args()

    try:
        import numpy as np
        import pandas as pd
        from sklearn.linear_model import Ridge
        from sklearn.preprocessing import StandardScaler
    except ImportError as e:
        print(f"missing dependency: {e}", file=sys.stderr)
        print("install with: pip install numpy pandas scikit-learn", file=sys.stderr)
        return 1

    df = pd.read_csv(args.csv)
    if "label" not in df.columns:
        print("CSV missing 'label' column", file=sys.stderr)
        return 1

    drop_cols = ["seed_index", "label"]
    if not args.include_step:
        drop_cols.append("step")
    feature_cols = [c for c in df.columns if c not in drop_cols]
    X = df[feature_cols].to_numpy(dtype=np.float64)
    y = df["label"].to_numpy(dtype=np.float64)

    print(f"rows={len(df)} features={len(feature_cols)}", file=sys.stderr)
    print(f"label: mean={y.mean():.3f} std={y.std():.3f} min={y.min()} max={y.max()}", file=sys.stderr)

    if args.standardize:
        scaler = StandardScaler()
        Xs = scaler.fit_transform(X)
        # Avoid division-by-zero: features with constant values get scale=0 → set to 1.
        scales = np.where(scaler.scale_ == 0, 1.0, scaler.scale_)
        means = scaler.mean_
        model = Ridge(alpha=args.alpha)
        model.fit(Xs, y)
        # Convert standardized weights back to raw-feature weights:
        #   y = w · ((x - mean) / scale) + intercept
        #     = (w/scale) · x + (intercept - sum(w·mean/scale))
        raw_weights = model.coef_ / scales
        raw_intercept = model.intercept_ - float(np.sum(model.coef_ * means / scales))
    else:
        model = Ridge(alpha=args.alpha)
        model.fit(X, y)
        raw_weights = model.coef_
        raw_intercept = float(model.intercept_)

    # Score for sanity.
    train_pred = X @ raw_weights + raw_intercept
    rmse = float(np.sqrt(np.mean((train_pred - y) ** 2)))
    r2 = float(1 - np.sum((y - train_pred) ** 2) / np.sum((y - y.mean()) ** 2))
    print(f"train RMSE={rmse:.4f} R²={r2:.4f}", file=sys.stderr)

    out = {
        "feature_names": feature_cols,
        "weights": [float(w) for w in raw_weights],
        "intercept": float(raw_intercept),
    }
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(json.dumps(out, indent=2))
    print(f"wrote {args.out}", file=sys.stderr)

    if args.gbdt_importance:
        try:
            from sklearn.ensemble import GradientBoostingRegressor
        except ImportError:
            print("sklearn.ensemble not available; skipping GBDT", file=sys.stderr)
            return 0
        gbdt = GradientBoostingRegressor(
            n_estimators=200, max_depth=3, learning_rate=0.05, random_state=0
        )
        gbdt.fit(X, y)
        importances = list(zip(feature_cols, gbdt.feature_importances_))
        importances.sort(key=lambda x: -x[1])
        print("\nGBDT feature importance (top 15):", file=sys.stderr)
        for name, imp in importances[:15]:
            print(f"  {name:40s} {imp:.4f}", file=sys.stderr)

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
