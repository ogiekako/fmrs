use fmrs_core::sfen;

use crate::solver::{self, Algorithm};

use super::parse_to_sfen;

/// For each problem file, find the position `n` moves before the end of the
/// unique solution and print its SFEN. Intended as a one-time helper to
/// generate positions that are then hardcoded into benchmarks.
pub fn solve_bench(files: Vec<String>, n: usize) -> anyhow::Result<()> {
    for file in files {
        let sfen_str = parse_to_sfen(&file)?;
        let initial = sfen::decode_position(&sfen_str)
            .map_err(|_| anyhow::anyhow!("parse failed: {file}"))?;

        let solutions = solver::solve(initial.clone(), Some(1), Algorithm::LowMemStandard, None)
            .map_err(|e| anyhow::anyhow!("{e}"))?;

        if solutions.is_empty() {
            anyhow::bail!("No solution for {file}");
        }
        let solution = &solutions[0];
        let steps = solution.len();

        if steps < n {
            anyhow::bail!("{file}: solution has only {steps} steps, cannot go {n} before mate");
        }

        let replay_count = steps - n;
        let mut position = initial;
        for m in solution[..replay_count].iter() {
            position.do_move(m);
        }
        let mid_sfen = sfen::encode_position(&position);
        println!("{file} (steps={steps}, -{n}): {mid_sfen}");
    }
    Ok(())
}
