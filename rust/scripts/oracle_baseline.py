#!/usr/bin/env python3
"""
oracle_baseline.py — (seed × step) → bpc predictor for the single-king-smoke
priority-queue scheduler.

Trains/evaluates a baseline scoring model that takes the partial trajectory
of a backward search and outputs a score correlating with the seed's final
best_piece_count. Designed to support a future best-first scheduler:
  - score is variable-length-adaptive (works at any step ≥ 1)
  - evaluation focuses on RANKING quality (Spearman / Top-K recall),
    which is what the PQ actually consumes; regression R² is reported as
    a secondary diagnostic
  - censored labels (EarlyExit-truncated seeds) are dropped by default
  - baselines (random, single-feature) included for absolute calibration
  - on `--out-dir`, saves a Ridge model artifact + diagnostics JSON

Inputs:
    <seed_log>                      — per-seed terminal records
    <seed_log>.trajectory.jsonl     — per-(seed, step) structural rows

Usage:
    python3 scripts/oracle_baseline.py [--seed-log PATH] [--cond-hash HASH]
        [--keep-early-exit] [--threshold T] [--out-dir DIR]
"""
from __future__ import annotations

import argparse
import json
import math
import sys
from collections import defaultdict
from pathlib import Path

import numpy as np
from scipy import stats
from sklearn.ensemble import GradientBoostingRegressor
from sklearn.linear_model import Ridge
from sklearn.metrics import r2_score, roc_auc_score
from sklearn.model_selection import GroupKFold
from sklearn.preprocessing import StandardScaler


# ---- IO ----------------------------------------------------------------

def load_trajectory(path: Path) -> dict[tuple[str, int], list[dict]]:
    rows: dict[tuple[str, int], list[dict]] = defaultdict(list)
    with path.open() as f:
        for line in f:
            line = line.strip()
            if not line:
                continue
            r = json.loads(line)
            rows[(r["cond"], r["seed"])].append(r)
    for key in rows:
        rows[key].sort(key=lambda r: r["step"])
    return rows


def load_seed_records(path: Path) -> list[dict]:
    out = []
    with path.open() as f:
        for line in f:
            line = line.strip()
            if not line:
                continue
            try:
                out.append(json.loads(line))
            except json.JSONDecodeError:
                continue
    return out


# ---- cond identification ----------------------------------------------

def find_cond(
    trajectory: dict[tuple[str, int], list[dict]],
    seed_records: list[dict],
    cond_hash: str | None,
) -> tuple[str, dict, dict[int, dict]]:
    """Return (cond_hash, cond_meta, terminal_by_seed)."""
    if cond_hash is None:
        cond_counts: dict[str, int] = defaultdict(int)
        for c, _ in trajectory:
            cond_counts[c] += 1
        if not cond_counts:
            raise RuntimeError("trajectory log is empty")
        cond_hash = max(cond_counts, key=cond_counts.__getitem__)

    traj_seeds = {s for c, s in trajectory if c == cond_hash}
    cond_meta: dict | None = None
    for rec in reversed(seed_records):
        if rec["seed_index"] not in traj_seeds:
            continue
        # Trajectory was added together with `terminal_step`; only post-update
        # records have terminal_step > 0, and only those reliably carry the
        # current binary's constraint shape.
        if rec.get("terminal_step", 0) == 0:
            continue
        cond_meta = {
            "max_step": rec.get("max_step"),
            "constraints": rec.get("constraints", {}),
        }
        break
    if cond_meta is None:
        raise RuntimeError(
            f"no post-update seed record matches cond_hash={cond_hash}; "
            "wait for at least one trajectory seed to finish."
        )

    terminal: dict[int, dict] = {}
    for rec in seed_records:
        if rec.get("max_step") != cond_meta["max_step"]:
            continue
        if rec.get("max_frontier") is not None:
            continue
        if rec.get("constraints") != cond_meta["constraints"]:
            continue
        terminal[rec["seed_index"]] = rec  # last occurrence wins
    return cond_hash, cond_meta, terminal


# ---- features ----------------------------------------------------------

FEATURE_NAMES = [
    "step_now",
    "log_step",
    "log_frontier",
    "log_memo",
    "log_ms",
    "log_cum_ms",
    "log_inner",
    "d_log_f_1",
    "d_log_m_1",
    "d_log_f_2",
    "d_log_m_2",
    "mean_d_log_f_3",
    "std_d_log_f_3",
    "mean_d_log_m_3",
    "slope_log_f",
    "slope_log_m",
]


def build_features(history: list[dict]) -> dict[str, float]:
    """Adaptive-length features defined at any step ≥ 1.

    Missing values (e.g., 2nd-back step at step=1) default to 0.
    """
    last = history[-1]
    step_now = last["step"]
    log_f = math.log(max(last["frontier"], 1))
    log_m = math.log(max(last["memo"], 1))
    log_ms = math.log(max(last["ms"], 1))
    cum_ms = sum(r["ms"] for r in history)

    deltas_f = [
        math.log(max(history[i]["frontier"], 1))
        - math.log(max(history[i - 1]["frontier"], 1))
        for i in range(1, len(history))
    ]
    deltas_m = [
        math.log(max(history[i]["memo"], 1))
        - math.log(max(history[i - 1]["memo"], 1))
        for i in range(1, len(history))
    ]

    def back(seq: list[float], idx: int) -> float:
        return seq[-(idx + 1)] if len(seq) > idx else 0.0

    recent_f = deltas_f[-3:] or [0.0]
    recent_m = deltas_m[-3:] or [0.0]

    if len(history) >= 2:
        steps_arr = np.array([r["step"] for r in history], dtype=float)
        log_f_arr = np.array(
            [math.log(max(r["frontier"], 1)) for r in history], dtype=float
        )
        log_m_arr = np.array(
            [math.log(max(r["memo"], 1)) for r in history], dtype=float
        )
        slope_f = float(np.polyfit(steps_arr, log_f_arr, 1)[0])
        slope_m = float(np.polyfit(steps_arr, log_m_arr, 1)[0])
    else:
        slope_f = 0.0
        slope_m = 0.0

    return {
        "step_now": float(step_now),
        "log_step": math.log(max(step_now, 1)),
        "log_frontier": log_f,
        "log_memo": log_m,
        "log_ms": log_ms,
        "log_cum_ms": math.log(max(cum_ms, 1)),
        "log_inner": math.log(max(last["inner"], 1)),
        "d_log_f_1": back(deltas_f, 0),
        "d_log_m_1": back(deltas_m, 0),
        "d_log_f_2": back(deltas_f, 1),
        "d_log_m_2": back(deltas_m, 1),
        "mean_d_log_f_3": float(np.mean(recent_f)),
        "std_d_log_f_3": float(np.std(recent_f)) if len(recent_f) > 1 else 0.0,
        "mean_d_log_m_3": float(np.mean(recent_m)),
        "slope_log_f": slope_f,
        "slope_log_m": slope_m,
    }


# ---- dataset -----------------------------------------------------------

def build_dataset(
    cond_hash: str,
    trajectory: dict[tuple[str, int], list[dict]],
    terminal: dict[int, dict],
    drop_early_exit: bool,
) -> tuple[np.ndarray, np.ndarray, np.ndarray, np.ndarray, list[str], dict]:
    Xs, ys, sids, ks = [], [], [], []
    excluded_no_label = 0
    excluded_early_exit = 0

    for (h, seed), traj in trajectory.items():
        if h != cond_hash:
            continue
        rec = terminal.get(seed)
        if rec is None:
            excluded_no_label += 1
            continue
        if drop_early_exit and rec.get("termination_reason") == "early_exit":
            excluded_early_exit += 1
            continue
        bpc = rec["best_piece_count"]
        for k in range(1, len(traj) + 1):
            history = traj[:k]
            feats = build_features(history)
            Xs.append([feats[name] for name in FEATURE_NAMES])
            ys.append(bpc)
            sids.append(seed)
            ks.append(history[-1]["step"])

    X = np.array(Xs, dtype=float)
    y = np.array(ys, dtype=float)
    sid = np.array(sids, dtype=int)
    step = np.array(ks, dtype=int)
    info = {
        "n_rows": int(len(y)),
        "n_seeds": int(len(np.unique(sid))) if len(y) else 0,
        "excluded_no_label": excluded_no_label,
        "excluded_early_exit": excluded_early_exit,
    }
    return X, y, sid, step, FEATURE_NAMES, info


# ---- evaluation helpers -----------------------------------------------

def safe_spearman(y_true: np.ndarray, y_pred: np.ndarray) -> float:
    if len(np.unique(y_true)) < 2 or len(np.unique(y_pred)) < 2:
        return float("nan")
    return float(stats.spearmanr(y_true, y_pred).correlation)


def safe_kendall(y_true: np.ndarray, y_pred: np.ndarray) -> float:
    if len(np.unique(y_true)) < 2 or len(np.unique(y_pred)) < 2:
        return float("nan")
    return float(stats.kendalltau(y_true, y_pred).statistic)


def safe_r2(y_true: np.ndarray, y_pred: np.ndarray) -> float:
    if len(np.unique(y_true)) < 2:
        return float("nan")
    return float(r2_score(y_true, y_pred))


def safe_auc(y_bin: np.ndarray, y_score: np.ndarray) -> float:
    if len(np.unique(y_bin)) < 2:
        return float("nan")
    return float(roc_auc_score(y_bin, y_score))


def top_k_recall(y_true: np.ndarray, y_pred: np.ndarray, k: int) -> float:
    """Of the k seeds with highest y_true, fraction in predicted top-k."""
    if len(y_true) == 0:
        return float("nan")
    k = max(1, min(k, len(y_true)))
    idx_true = set(np.argsort(y_true, kind="stable")[-k:].tolist())
    idx_pred = set(np.argsort(y_pred, kind="stable")[-k:].tolist())
    return len(idx_true & idx_pred) / k


def aggregate_per_seed(
    pred_row: np.ndarray, sid_row: np.ndarray, step_row: np.ndarray
) -> tuple[np.ndarray, np.ndarray]:
    """Reduce row-level preds to one prediction per seed (latest step)."""
    by_seed: dict[int, tuple[int, float]] = {}
    for i in range(len(pred_row)):
        s = int(sid_row[i])
        k = int(step_row[i])
        if s not in by_seed or k > by_seed[s][0]:
            by_seed[s] = (k, float(pred_row[i]))
    seeds = np.array(sorted(by_seed), dtype=int)
    preds = np.array([by_seed[s][1] for s in seeds], dtype=float)
    return seeds, preds


def per_seed_labels(
    y_row: np.ndarray, sid_row: np.ndarray
) -> tuple[np.ndarray, np.ndarray]:
    seen: dict[int, float] = {}
    for i in range(len(y_row)):
        seen[int(sid_row[i])] = float(y_row[i])
    seeds = np.array(sorted(seen), dtype=int)
    labels = np.array([seen[s] for s in seeds], dtype=float)
    return seeds, labels


# ---- models -----------------------------------------------------------

class StandardizedRidge:
    def __init__(self, alpha: float = 1.0) -> None:
        self.scaler = StandardScaler()
        self.ridge = Ridge(alpha=alpha)

    def fit(self, X: np.ndarray, y: np.ndarray) -> "StandardizedRidge":
        self.ridge.fit(self.scaler.fit_transform(X), y)
        return self

    def predict(self, X: np.ndarray) -> np.ndarray:
        return self.ridge.predict(self.scaler.transform(X))

    def to_dict(self) -> dict:
        return {
            "feature_means": self.scaler.mean_.tolist(),
            "feature_scales": self.scaler.scale_.tolist(),
            "weights": self.ridge.coef_.tolist(),
            "intercept": float(self.ridge.intercept_),
        }


def make_gbdt() -> GradientBoostingRegressor:
    return GradientBoostingRegressor(
        n_estimators=120, max_depth=3, learning_rate=0.08, random_state=0
    )


# ---- evaluation pipeline ----------------------------------------------

def eval_models(
    X: np.ndarray,
    y: np.ndarray,
    sid: np.ndarray,
    step: np.ndarray,
    feature_names: list[str],
    threshold: float,
    rng_seed: int = 0,
) -> dict:
    n_seeds = int(len(np.unique(sid)))
    n_splits = min(5, n_seeds)
    if n_splits < 2:
        return {
            "n_rows": int(len(y)),
            "n_seeds": n_seeds,
            "n_splits": 0,
            "threshold": threshold,
            "skip_reason": "fewer than 2 distinct seeds; cannot CV",
            "models": {},
        }

    rng = np.random.default_rng(rng_seed)
    gkf = GroupKFold(n_splits=n_splits)

    K_FRACS = (0.05, 0.10, 0.25)

    def empty() -> dict:
        return {
            "row_r2": [],
            "row_spearman": [],
            "seed_r2": [],
            "seed_spearman": [],
            "seed_kendall": [],
            "seed_auc": [],
            "seed_top_frac": {f: [] for f in K_FRACS},
        }

    models = ["ridge", "gbdt", "rand", "log_frontier", "log_memo", "log_cum_ms"]
    metrics = {m: empty() for m in models}

    for tr_idx, te_idx in gkf.split(X, y, groups=sid):
        X_tr, X_te = X[tr_idx], X[te_idx]
        y_tr, y_te = y[tr_idx], y[te_idx]
        sid_te, step_te = sid[te_idx], step[te_idx]

        ridge = StandardizedRidge(alpha=1.0).fit(X_tr, y_tr)
        gbdt = make_gbdt().fit(X_tr, y_tr)

        preds = {
            "ridge": ridge.predict(X_te),
            "gbdt": gbdt.predict(X_te),
            "rand": rng.standard_normal(len(y_te)),
            "log_frontier": X_te[:, feature_names.index("log_frontier")],
            "log_memo": X_te[:, feature_names.index("log_memo")],
            "log_cum_ms": X_te[:, feature_names.index("log_cum_ms")],
        }

        for name, p in preds.items():
            m = metrics[name]
            m["row_r2"].append(safe_r2(y_te, p))
            m["row_spearman"].append(safe_spearman(y_te, p))
            seeds_te, sp = aggregate_per_seed(p, sid_te, step_te)
            _, sy = per_seed_labels(y_te, sid_te)
            m["seed_r2"].append(safe_r2(sy, sp))
            m["seed_spearman"].append(safe_spearman(sy, sp))
            m["seed_kendall"].append(safe_kendall(sy, sp))
            m["seed_auc"].append(safe_auc((sy >= threshold).astype(int), sp))
            for frac in K_FRACS:
                k = max(1, int(round(len(sy) * frac)))
                m["seed_top_frac"][frac].append(top_k_recall(sy, sp, k))

    aggregated = {}
    for name, m in metrics.items():
        d = {
            "row_r2": float(np.nanmean(m["row_r2"])),
            "row_spearman": float(np.nanmean(m["row_spearman"])),
            "seed_r2": float(np.nanmean(m["seed_r2"])),
            "seed_spearman": float(np.nanmean(m["seed_spearman"])),
            "seed_kendall": float(np.nanmean(m["seed_kendall"])),
            "seed_auc": float(np.nanmean(m["seed_auc"])),
        }
        for frac in K_FRACS:
            d[f"top_{int(frac*100)}pct"] = float(np.nanmean(m["seed_top_frac"][frac]))
        aggregated[name] = d

    return {
        "n_rows": int(len(y)),
        "n_seeds": n_seeds,
        "n_splits": n_splits,
        "threshold": threshold,
        "K_fracs": list(K_FRACS),
        "models": aggregated,
    }


def eval_at_step_buckets(
    X: np.ndarray,
    y: np.ndarray,
    sid: np.ndarray,
    step: np.ndarray,
    feature_names: list[str],
) -> dict:
    """Train on all data, then evaluate per-seed using rows at step ≤ k.

    Reports how ranking quality evolves as the search proceeds: at "early"
    steps how well can we already rank?
    """
    n_seeds = int(len(np.unique(sid)))
    n_splits = min(5, n_seeds)
    if n_splits < 2:
        return {"skip_reason": "fewer than 2 distinct seeds"}

    gkf = GroupKFold(n_splits=n_splits)
    BUCKETS = [1, 3, 5, 10, 9999]  # 9999 = "all observed"

    def empty() -> dict:
        return {b: {"spearman": [], "top10pct": []} for b in BUCKETS}

    per_model = {"ridge": empty(), "gbdt": empty()}

    for tr_idx, te_idx in gkf.split(X, y, groups=sid):
        X_tr, X_te = X[tr_idx], X[te_idx]
        y_tr, y_te = y[tr_idx], y[te_idx]
        sid_te, step_te = sid[te_idx], step[te_idx]

        ridge = StandardizedRidge(alpha=1.0).fit(X_tr, y_tr)
        gbdt = make_gbdt().fit(X_tr, y_tr)

        for name, model in (("ridge", ridge), ("gbdt", gbdt)):
            for b in BUCKETS:
                mask = step_te <= b
                if not mask.any():
                    continue
                p = model.predict(X_te[mask])
                seeds_b, sp = aggregate_per_seed(
                    p, sid_te[mask], step_te[mask]
                )
                _, sy = per_seed_labels(y_te[mask], sid_te[mask])
                if len(sy) < 2:
                    continue
                per_model[name][b]["spearman"].append(safe_spearman(sy, sp))
                k = max(1, int(round(len(sy) * 0.1)))
                per_model[name][b]["top10pct"].append(top_k_recall(sy, sp, k))

    summary = {}
    for name, buckets in per_model.items():
        summary[name] = {}
        for b, vals in buckets.items():
            sp = vals["spearman"]
            tk = vals["top10pct"]
            summary[name][str(b)] = {
                "n_folds": len(sp),
                "spearman_mean": float(np.nanmean(sp)) if sp else float("nan"),
                "top10pct_mean": float(np.nanmean(tk)) if tk else float("nan"),
            }
    return summary


# ---- output ----------------------------------------------------------

def fmt_eval_table(out: dict) -> str:
    lines = []
    if "skip_reason" in out:
        return f"  skipped: {out['skip_reason']}"
    K_fracs = out["K_fracs"]
    lines.append(
        f"\n  evaluation: n_rows={out['n_rows']} n_seeds={out['n_seeds']} "
        f"n_splits={out['n_splits']} threshold={out['threshold']}"
    )
    cols = ["model", "row_r2", "row_ρ", "seed_r2", "seed_ρ", "kendall", "auc≥T"]
    cols += [f"top{int(f*100)}%" for f in K_fracs]
    header = " ".join(c.rjust(9) for c in cols)
    lines.append("  " + header)
    for name, d in out["models"].items():
        row = [name.ljust(9)]
        row.append(f"{d['row_r2']:+.3f}")
        row.append(f"{d['row_spearman']:+.3f}")
        row.append(f"{d['seed_r2']:+.3f}")
        row.append(f"{d['seed_spearman']:+.3f}")
        row.append(f"{d['seed_kendall']:+.3f}")
        row.append(f"{d['seed_auc']:+.3f}")
        for f in K_fracs:
            row.append(f"{d[f'top_{int(f*100)}pct']:.3f}")
        lines.append("  " + " ".join(c.rjust(9) for c in row))
    return "\n".join(lines)


def fmt_step_buckets(s: dict) -> str:
    if "skip_reason" in s:
        return f"  skipped: {s['skip_reason']}"
    lines = ["\n  ranking quality by step bucket (rows where step ≤ b):"]
    lines.append(
        "  " + " ".join(c.rjust(10) for c in ["model", "≤1", "≤3", "≤5", "≤10", "all"])
    )
    for name, buckets in s.items():
        line = [name.ljust(10)]
        for b in ("1", "3", "5", "10", "9999"):
            v = buckets.get(b, {})
            sp = v.get("spearman_mean", float("nan"))
            line.append("nan" if math.isnan(sp) else f"{sp:+.3f}")
        lines.append("  " + " ".join(c.rjust(10) for c in line))
    lines.append(
        "  " + " ".join(c.rjust(10) for c in ["model", "≤1", "≤3", "≤5", "≤10", "all"])
        + "  (top-10%)"
    )
    for name, buckets in s.items():
        line = [name.ljust(10)]
        for b in ("1", "3", "5", "10", "9999"):
            v = buckets.get(b, {})
            t = v.get("top10pct_mean", float("nan"))
            line.append("nan" if math.isnan(t) else f"{t:.3f}")
        lines.append("  " + " ".join(c.rjust(10) for c in line))
    return "\n".join(lines)


# ---- main ------------------------------------------------------------

def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.strip().splitlines()[0])
    ap.add_argument(
        "--seed-log",
        type=Path,
        default=Path("target/single-king-smoke-ideal-backward-seeds.jsonl"),
    )
    ap.add_argument("--cond-hash", type=str, default=None)
    ap.add_argument(
        "--keep-early-exit",
        action="store_true",
        help="don't drop seeds whose termination_reason=early_exit (their bpc is censored)",
    )
    ap.add_argument(
        "--threshold",
        type=float,
        default=None,
        help="bpc threshold T for binary classification (default: 75th percentile)",
    )
    ap.add_argument(
        "--out-dir",
        type=Path,
        default=None,
        help="save Ridge model artifact + diagnostics under <out_dir>/<cond_hash>/",
    )
    args = ap.parse_args()

    seed_log: Path = args.seed_log
    traj_log = Path(str(seed_log) + ".trajectory.jsonl")
    if not traj_log.exists():
        print(f"ERROR: trajectory log not found at {traj_log}", file=sys.stderr)
        return 1

    print(f"reading {traj_log}")
    trajectory = load_trajectory(traj_log)
    print(f"reading {seed_log}")
    seed_records = load_seed_records(seed_log)
    print(
        f"  trajectory: {sum(len(v) for v in trajectory.values())} rows / "
        f"{len(trajectory)} (cond,seed)"
    )
    print(f"  seeds.jsonl: {len(seed_records)} records")

    cond_hash, cond_meta, terminal = find_cond(
        trajectory, seed_records, args.cond_hash
    )
    cstr = cond_meta["constraints"]
    print(f"\nactive cond hash={cond_hash}")
    print(
        f"  max_step={cond_meta['max_step']}  no_pawn={cstr.get('no_pawn')}  "
        f"slack={cstr.get('slack')}  allow_white={cstr.get('allow_white_pieces')}  "
        f"max_promoted_pct={cstr.get('max_promoted_pct')}  "
        f"mate_squares_set={(cstr.get('mate_squares', 0) or 0) != 0}"
    )
    print(f"  matched terminal records: {len(terminal)}")

    X, y, sid, step, feature_names, info = build_dataset(
        cond_hash, trajectory, terminal, drop_early_exit=not args.keep_early_exit
    )
    print(
        f"\ndataset: n_rows={info['n_rows']}  n_seeds={info['n_seeds']}  "
        f"excluded_no_label={info['excluded_no_label']}  "
        f"excluded_early_exit={info['excluded_early_exit']}"
    )
    if info["n_rows"] == 0:
        print("ERROR: no labeled (seed, step) rows. Wait for more data.", file=sys.stderr)
        return 2
    print(
        f"  bpc∈[{int(y.min())},{int(y.max())}] mean={y.mean():.2f} std={y.std():.2f}  "
        f"step∈[{int(step.min())},{int(step.max())}]"
    )

    if args.threshold is not None:
        threshold = float(args.threshold)
    elif len(np.unique(y)) > 1:
        threshold = float(np.quantile(y, 0.75))
    else:
        threshold = float(y[0])

    eval_main = eval_models(X, y, sid, step, feature_names, threshold)
    print(fmt_eval_table(eval_main))
    eval_buckets = eval_at_step_buckets(X, y, sid, step, feature_names)
    print(fmt_step_buckets(eval_buckets))

    print("\nper-feature Pearson with bpc (all rows):")
    for j, name in enumerate(feature_names):
        col = X[:, j]
        if col.std() < 1e-12 or y.std() < 1e-12:
            r = float("nan")
        else:
            r = float(np.corrcoef(col, y)[0, 1])
        marker = ""
        if not math.isnan(r) and abs(r) >= 0.5:
            marker = "  *"
        print(f"  {name:<22} {r:+.3f}{marker}")

    if args.out_dir is not None:
        out_dir = args.out_dir / cond_hash
        out_dir.mkdir(parents=True, exist_ok=True)
        full = StandardizedRidge(alpha=1.0).fit(X, y)
        artifact = {
            "type": "standardized_ridge_v1",
            "cond_hash": cond_hash,
            "feature_names": feature_names,
            **full.to_dict(),
        }
        (out_dir / "model_ridge.json").write_text(json.dumps(artifact, indent=2))
        diag = {
            "cond_hash": cond_hash,
            "cond_meta": cond_meta,
            "dataset_info": info,
            "threshold": threshold,
            "evaluation": eval_main,
            "evaluation_step_buckets": eval_buckets,
            "per_feature_pearson": {
                name: float(np.corrcoef(X[:, j], y)[0, 1])
                if X[:, j].std() > 1e-12 and y.std() > 1e-12
                else float("nan")
                for j, name in enumerate(feature_names)
            },
        }
        (out_dir / "diagnostics.json").write_text(
            json.dumps(diag, indent=2, default=str)
        )
        print(f"\nsaved {out_dir}/model_ridge.json")
        print(f"saved {out_dir}/diagnostics.json")

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
