//! Oracle model loader + feature computation for the priority-queue
//! scheduler. Mirrors `scripts/oracle_baseline.py`'s feature schema so that
//! a model trained offline can score trajectories online.

use anyhow::{bail, Context as _};
use serde::Deserialize;
use std::io::BufReader;
use std::path::Path;

/// One observed step in a seed's backward search trajectory.
#[derive(Clone, Copy, Debug)]
pub(super) struct StepRecord {
    pub(super) step: u16,
    pub(super) frontier: usize,
    pub(super) memo: usize,
    pub(super) inner: usize,
    pub(super) ms: u128,
}

/// Standardized Ridge model in the same JSON format produced by
/// `oracle_baseline.py --out-dir`.
#[derive(Debug, Clone, Deserialize)]
pub(super) struct OracleModel {
    #[serde(rename = "type")]
    pub(super) model_type: String,
    pub(super) feature_names: Vec<String>,
    pub(super) feature_means: Vec<f64>,
    pub(super) feature_scales: Vec<f64>,
    pub(super) weights: Vec<f64>,
    pub(super) intercept: f64,
}

impl OracleModel {
    pub(super) fn load(path: &Path) -> anyhow::Result<Self> {
        let file = std::fs::File::open(path)
            .with_context(|| format!("opening oracle model at {}", path.display()))?;
        let model: OracleModel = serde_json::from_reader(BufReader::new(file))
            .with_context(|| format!("parsing oracle model at {}", path.display()))?;
        if model.model_type != "standardized_ridge_v1" {
            bail!(
                "unsupported oracle model type: {} (expected standardized_ridge_v1)",
                model.model_type
            );
        }
        let n = model.feature_names.len();
        if model.feature_means.len() != n
            || model.feature_scales.len() != n
            || model.weights.len() != n
        {
            bail!("oracle model dimension mismatch (n_features={n})");
        }
        Ok(model)
    }

    /// Cold-start score: produced when the trajectory is empty (seed has not
    /// been advanced yet). Returns `intercept`, which equals the model's
    /// unconditional mean prediction. Warm seeds with predicted bpc above the
    /// mean float to the top of the PQ; predicted-low warms sink below cold.
    pub(super) fn cold_score(&self) -> f64 {
        self.intercept
    }

    /// Score the trajectory observed so far. Higher = more promising.
    pub(super) fn score(&self, history: &[StepRecord]) -> f64 {
        if history.is_empty() {
            return self.cold_score();
        }
        let features = compute_features(history, &self.feature_names);
        let mut s = self.intercept;
        for (i, &w) in self.weights.iter().enumerate() {
            let scale = self.feature_scales[i].max(1e-12);
            let x = (features[i] - self.feature_means[i]) / scale;
            s += w * x;
        }
        s
    }
}

fn compute_features(history: &[StepRecord], names: &[String]) -> Vec<f64> {
    debug_assert!(!history.is_empty());
    let last = history.last().unwrap();
    let log_f = (last.frontier.max(1) as f64).ln();
    let log_m = (last.memo.max(1) as f64).ln();
    let log_ms = (last.ms.max(1) as f64).ln();
    let cum_ms: u128 = history.iter().map(|r| r.ms).sum();
    let log_cum_ms = (cum_ms.max(1) as f64).ln();
    let log_inner = (last.inner.max(1) as f64).ln();

    let deltas_f: Vec<f64> = (1..history.len())
        .map(|i| {
            (history[i].frontier.max(1) as f64).ln()
                - (history[i - 1].frontier.max(1) as f64).ln()
        })
        .collect();
    let deltas_m: Vec<f64> = (1..history.len())
        .map(|i| {
            (history[i].memo.max(1) as f64).ln() - (history[i - 1].memo.max(1) as f64).ln()
        })
        .collect();

    let back = |seq: &[f64], idx: usize| -> f64 {
        if seq.len() > idx {
            seq[seq.len() - 1 - idx]
        } else {
            0.0
        }
    };

    let recent_f: Vec<f64> = if deltas_f.is_empty() {
        vec![0.0]
    } else {
        deltas_f.iter().rev().take(3).copied().collect()
    };
    let recent_m: Vec<f64> = if deltas_m.is_empty() {
        vec![0.0]
    } else {
        deltas_m.iter().rev().take(3).copied().collect()
    };
    let mean_d_log_f_3 = recent_f.iter().sum::<f64>() / recent_f.len() as f64;
    let std_d_log_f_3 = if recent_f.len() > 1 {
        let m = mean_d_log_f_3;
        let v = recent_f.iter().map(|&x| (x - m).powi(2)).sum::<f64>() / recent_f.len() as f64;
        v.sqrt()
    } else {
        0.0
    };
    let mean_d_log_m_3 = recent_m.iter().sum::<f64>() / recent_m.len() as f64;

    let (slope_f, slope_m) = if history.len() >= 2 {
        let xs: Vec<f64> = history.iter().map(|r| r.step as f64).collect();
        let ys_f: Vec<f64> = history
            .iter()
            .map(|r| (r.frontier.max(1) as f64).ln())
            .collect();
        let ys_m: Vec<f64> = history.iter().map(|r| (r.memo.max(1) as f64).ln()).collect();
        (linear_slope(&xs, &ys_f), linear_slope(&xs, &ys_m))
    } else {
        (0.0, 0.0)
    };

    let mut out = Vec::with_capacity(names.len());
    for name in names {
        let v = match name.as_str() {
            "step_now" => last.step as f64,
            "log_step" => (last.step.max(1) as f64).ln(),
            "log_frontier" => log_f,
            "log_memo" => log_m,
            "log_ms" => log_ms,
            "log_cum_ms" => log_cum_ms,
            "log_inner" => log_inner,
            "d_log_f_1" => back(&deltas_f, 0),
            "d_log_m_1" => back(&deltas_m, 0),
            "d_log_f_2" => back(&deltas_f, 1),
            "d_log_m_2" => back(&deltas_m, 1),
            "mean_d_log_f_3" => mean_d_log_f_3,
            "std_d_log_f_3" => std_d_log_f_3,
            "mean_d_log_m_3" => mean_d_log_m_3,
            "slope_log_f" => slope_f,
            "slope_log_m" => slope_m,
            _ => 0.0, // unknown feature → 0 (forward-compat)
        };
        out.push(v);
    }
    out
}

fn linear_slope(xs: &[f64], ys: &[f64]) -> f64 {
    debug_assert_eq!(xs.len(), ys.len());
    let n = xs.len() as f64;
    let mean_x = xs.iter().sum::<f64>() / n;
    let mean_y = ys.iter().sum::<f64>() / n;
    let num: f64 = xs
        .iter()
        .zip(ys)
        .map(|(&x, &y)| (x - mean_x) * (y - mean_y))
        .sum();
    let den: f64 = xs.iter().map(|&x| (x - mean_x).powi(2)).sum();
    if den.abs() < 1e-12 {
        0.0
    } else {
        num / den
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cold_score_returns_intercept() {
        let m = OracleModel {
            model_type: "standardized_ridge_v1".into(),
            feature_names: vec!["log_frontier".into()],
            feature_means: vec![3.0],
            feature_scales: vec![2.0],
            weights: vec![0.5],
            intercept: 7.0,
        };
        assert!((m.cold_score() - 7.0).abs() < 1e-12);
        assert!((m.score(&[]) - 7.0).abs() < 1e-12);
    }

    #[test]
    fn warm_score_applies_standardization_and_weights() {
        let m = OracleModel {
            model_type: "standardized_ridge_v1".into(),
            feature_names: vec!["log_frontier".into()],
            feature_means: vec![3.0],
            feature_scales: vec![2.0],
            weights: vec![0.5],
            intercept: 7.0,
        };
        // log_frontier of frontier=20 ≈ 3.0, so standardized x ≈ 0, contribution 0
        let h = vec![StepRecord {
            step: 1,
            frontier: 20,
            memo: 1,
            inner: 1,
            ms: 1,
        }];
        let s = m.score(&h);
        let expected_log_f = (20f64).ln();
        let expected_x = (expected_log_f - 3.0) / 2.0;
        let expected = 7.0 + 0.5 * expected_x;
        assert!((s - expected).abs() < 1e-9);
    }

    #[test]
    fn slope_handles_short_history() {
        let names = vec!["slope_log_f".into()];
        let h_one = vec![StepRecord {
            step: 1,
            frontier: 10,
            memo: 1,
            inner: 1,
            ms: 1,
        }];
        let v = compute_features(&h_one, &names);
        assert_eq!(v[0], 0.0);
    }
}
