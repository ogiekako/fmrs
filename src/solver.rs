use crate::piece::*;
use crate::position::*;
use crate::reconstruct::reconstruct_solutions;

pub enum SolutionReply {
    Progress(usize),
    Solutions(Vec<Solution>),
}

pub type Solution = Vec<Movement>;

pub fn solve(board: Position, solutions_upto: Option<usize>) -> anyhow::Result<Vec<Solution>> {
    let (tx, _rx) = futures::channel::mpsc::unbounded();
    solve_with_progress(tx, board, solutions_upto)
}

use std::collections::HashSet;
use std::collections::{HashMap, VecDeque};
use std::hash::Hash;
use std::hash::Hasher;

pub type Digest = u64;

pub(super) fn digest(board: &Position) -> u64 {
    let mut hasher = twox_hash::Xxh3Hash64::default();
    board.hash(&mut hasher);
    hasher.finish()
}

fn pretty(size: usize) -> String {
    let giga = (size as f64) / 1000.0 / 1000.0 / 1000.0;
    format!("{:.2}G", giga)
}

pub fn solve_with_progress(
    progress: futures::channel::mpsc::UnboundedSender<usize>,
    board: Position,
    solutions_upto: Option<usize>,
) -> anyhow::Result<Vec<Solution>> {
    if board.turn() != Black {
        anyhow::bail!("The turn should be from black");
    }
    if board.checked(White) {
        anyhow::bail!("on black's turn, white is already checked.");
    }
    debug_assert_ne!(board.turn() == Black, board.checked(White));

    // position -> min step
    let mut memo = HashMap::new();
    memo.insert(digest(&board), 0);
    let mut queue = VecDeque::new();
    queue.push_back((0, board.clone()));

    let mut mate_in = None;
    let mut mate_positions = vec![];

    let mut dead_end: HashSet<u64> = HashSet::new();

    let mut current_step = 0;
    while let Some((step, board)) = queue.pop_front() {
        let n_step = step + 1;
        debug_assert!(memo.get(&digest(&board)).is_some());

        if let Some(s) = mate_in {
            if s < step {
                break;
            }
        }

        if step > current_step {
            eprintln!(
                "step {}, queue len = {}, hash len = {}({}), deadend = {}({})",
                step,
                pretty(queue.len() * std::mem::size_of::<Position>()),
                memo.len(),
                pretty(
                    memo.len()
                        * (std::mem::size_of::<u64>() * 2 + std::mem::size_of::<Vec<UndoMove>>())
                ),
                dead_end.len(),
                pretty(dead_end.len() * std::mem::size_of::<u64>()),
            );
            current_step = step;

            progress.unbounded_send(step)?;
        }

        let mut movable = false;

        for np in advance(board.clone())? {
            movable = true;
            if mate_in.is_some() {
                break;
            }
            let h = digest(&np);
            if dead_end.contains(&h) {
                continue;
            }
            if memo.contains_key(&h) {
                continue;
            }
            memo.insert(h, n_step);
            queue.push_back((n_step, np));
        }
        if !movable {
            if board.turn() == White && !board.pawn_drop() {
                // Checkmate
                mate_positions.push(board);
                mate_in = Some(step);
            } else {
                // Deadend. We can forgot history.
                let h = digest(&board);
                memo.remove(&h);
                dead_end.insert(h);
            }
        }
    }

    let mut res = vec![];
    for mate_position in mate_positions {
        res.append(&mut reconstruct_solutions(mate_position, &memo));
    }
    Ok(res)
}

fn update(
    res: &mut Vec<Solution>,
    mut rev_sol: &mut Solution,
    memo: &HashMap<u64, (usize, Vec<UndoMove>)>,
    g: &mut Position,
    limit: Option<usize>,
) {
    if let Some(lim) = limit {
        if lim <= res.len() {
            return;
        }
    }
    let (step, toks) = memo.get(&digest(&g)).unwrap();
    if *step == 0 {
        res.push(rev_sol.clone().into_iter().rev().collect());
        return;
    }
    for tok in toks {
        let mv = g.undo_move(tok);

        rev_sol.push(mv);
        update(res, &mut rev_sol, memo, g, limit);
        let mv = rev_sol.pop().unwrap();

        g.do_move(&mv);
    }
}
#[cfg(test)]
mod tests {
    use crate::{
        position::Movement,
        solver::{solve, Solution},
    };

    #[test]
    fn test_solve() {
        use crate::sfen;

        for tc in vec![
            (
                "3+pks3/9/4+P4/9/9/8B/9/9/9 b S2rb4g2s4n4l16p 1",
                vec!["1f5b+ 4a5b S*4b"],
            ),
            (
                "9/9/9/3bkb3/9/3+R1+R3/9/9/9 b 4g4s4n4l18p 1",
                vec!["4644 5463 4433 6354 B*63", "6664 5443 6473 4354 B*43"],
            ),
            (
                "9/6b2/7k1/5b3/7sL/9/9/9/9 b Rr4g3s4n3l18p 1",
                vec!["1513+ 2324 1323 2414 R*13"],
            ),
            (
                "9/9/9/9/9/9/9/5G2l/K4+B1kr b rb3g4s4n3l18p",
                vec!["4938"],
            ),
            (
                // Double check should not be counted twice.
                "7nk/9/9/9/8+B/9/9/9/8L b 2rb4g4s3n3l18p 1",
                vec!["1533"],
            ),
            (
                "7br/5ssss/5gggg/9/9/B8/Rn1K5/9/3k5 b 3n4l18p 1",
                vec!["9799"],
            ),
            (
                "9/6K1k/9/9/9/8N/8R/9/9 b r2b4g4s3n4l18p 1",
                vec!["1624"]
            ),
            // http://cavesfairy.g1.xrea.com/pub/qgfairy/
            (
                // 06-07 (57 steps)
                "9/9/9/9/7bb/1ppssssp1/K5k2/+RL1l1gg2/rL3gg1+l b 4N15P 1",
                vec!["N*29 3829 P*38 3747 N*59 4859 P*48 4757 N*69 6869+ P*58 5767 P*68 6778 P*79 6979 8886 7877 P*78 7768 P*69 5969 7877 6867 P*68 6758 P*59 4959 6867 5857 P*58 5748 P*49 3949 5857 4847 P*48 4738 P*39 2939 4847 3837 P*38 3728 3837 3938 P*29 2817 P*18 1716 N*28 3828 1817 1627 2928 2737 G*38"]
            ),
        ] {
            let board = sfen::decode_position(tc.0).expect("Failed to parse");
            let want: Vec<Solution> =
                tc.1.into_iter()
                    .map(|x| sfen::decode_moves(x).unwrap())
                    .collect();

            eprintln!("Solving {:?}", board);
            let got = solve(board, None).unwrap();

            assert_eq!(got, want);
        }
    }

    #[test]
    fn no_answer() {
        use crate::sfen;

        for sfen in [
            "4k4/9/4P4/9/9/8p/8K/9/9 b G2r2b3g4s4n4l16p 1",
            "9/9/9/5bp1G/6k2/6l1P/8K/9/8N b 2rb3g4s3n3l16p 1",
            "9/9/9/9/9/pp7/kl7/9/K8 b P2r2b4g4s4n3l15p 1",
            "7pk/7bg/9/8K/8N/8P/8L/9/9 b 2rb3g4s3n3l16p",
            "9/9/9/4k4/9/3pK3+r/4LP3/9/9 b r2b4g4s4n3l16p 1",
            "9/9/9/4k4/9/4K1P1+r/9/8B/9 b rb4g4s4n4l17p 1",
        ] {
            let board = sfen::decode_position(sfen).unwrap();
            let got = solve(board.clone(), None).unwrap();
            let want: Vec<Vec<Movement>> = vec![];
            eprintln!("Solving {:?}", board);
            assert_eq!(got, want);
        }
    }
}
