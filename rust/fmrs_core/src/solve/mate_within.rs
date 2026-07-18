use crate::position::position::PositionAux;

use super::low_mem_standard::LowMemStandardSolver;
use super::parallel_solve::ParallelSolver;
use super::SolverStatus;

/// `positions` のいずれかから `cap` 手以内に詰みがあるかを 1 回の共有 BFS
/// (グローバル dedup) で調べ、あれば最短の詰み手数を返す。`cap` を超える
/// 深さは探索しない。
///
/// 「どの根から詰むか」の帰属は行わない (min over roots)。逆算の唯一性検証
/// では「redo 以外のどの後続局面からも S 手以内に詰みがないこと」という
/// 存在判定だけで足りるため、これで候補集合をまとめて証明できる。
///
/// `parallel` は rayon の現在のスレッドプールを使う (呼び出し側で
/// `ThreadPool::install` されていればそのプール)。
pub fn shallowest_mate_within(
    positions: Vec<PositionAux>,
    cap: u16,
    parallel: bool,
) -> anyhow::Result<Option<u16>> {
    if positions.is_empty() {
        return Ok(None);
    }
    if parallel {
        let mut solver = ParallelSolver::with_multiple(positions, 0)?;
        drive(cap, move || solver.advance())
    } else {
        let mut solver = LowMemStandardSolver::with_multiple(positions, 0, true)?;
        drive(cap, move || solver.advance())
    }
}

fn drive(
    cap: u16,
    mut advance: impl FnMut() -> anyhow::Result<SolverStatus>,
) -> anyhow::Result<Option<u16>> {
    loop {
        match advance()? {
            SolverStatus::Intermediate(step) => {
                // Intermediate(step) の次の advance は深さ `step` の層を
                // 展開する (詰み検出も深さ step)。step > cap なら以降の
                // 詰みは cap 超なので打ち切ってよい。
                if step as u16 > cap {
                    return Ok(None);
                }
            }
            SolverStatus::Mate(reconstructor) => {
                let mate_in = reconstructor
                    .mate_in()
                    .expect("Mate status implies at least one mate");
                return Ok((mate_in <= cap).then_some(mate_in));
            }
            SolverStatus::NoSolution => return Ok(None),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::shallowest_mate_within;
    use crate::position::position::PositionAux;
    use crate::solve::standard_solve::standard_solve;

    #[test]
    fn shallowest_mate_within_respects_cap() {
        // 石壁のミニ問題 (唯一解)。最短手数は standard_solve を正とする。
        let position = PositionAux::from_sfen("9/9/9/9/9/5OOOO/5OPRP/5O2k/5O3 b - 1").unwrap();
        let mate_len = standard_solve(position.clone(), 1, true)
            .unwrap()
            .solutions()[0]
            .len();
        let mate_len = u16::try_from(mate_len).unwrap();

        for parallel in [false, true] {
            let found =
                shallowest_mate_within(vec![position.clone()], mate_len + 5, parallel).unwrap();
            assert_eq!(found, Some(mate_len), "parallel={parallel}");

            // cap 未満なら探索を打ち切って None
            let capped =
                shallowest_mate_within(vec![position.clone()], mate_len - 1, parallel).unwrap();
            assert_eq!(capped, None, "parallel={parallel}");
        }
    }
}
