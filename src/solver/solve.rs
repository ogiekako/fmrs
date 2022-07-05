use crate::piece::*;
use crate::position;
use crate::position::Movement;
use crate::position::Position;
use crate::position::PositionExt;
use crate::solver::reconstruct::reconstruct_solutions;

use std::collections::HashMap;
use std::collections::HashSet;
use std::hash::Hash;
use std::hash::Hasher;
use std::io::Write;
use std::sync::Arc;

pub type Solution = Vec<Movement>;

pub fn solve(board: Position) -> anyhow::Result<Vec<Solution>> {
    let (tx, _rx) = futures::channel::mpsc::unbounded();
    solve_with_progress(tx, board)
}

pub(super) type Digest = u64;

pub(super) fn digest(board: &Position) -> Digest {
    let mut hasher = twox_hash::Xxh3Hash64::default();
    board.hash(&mut hasher);
    hasher.finish()
}

pub fn solve_with_progress(
    progress: futures::channel::mpsc::UnboundedSender<usize>,
    position: Position,
) -> anyhow::Result<Vec<Solution>> {
    if position.turn() != Black {
        anyhow::bail!("The turn should be from black");
    }
    if position.checked_slow(White) {
        anyhow::bail!("on black's turn, white is already checked.");
    }
    debug_assert_ne!(position.turn() == Black, position.checked_slow(White));

    let mut step = 0;
    // position -> min step
    let mut memo_current_turn = HashMap::new();
    memo_current_turn.insert(digest(&position), step);
    let mut memo_next_turn = HashMap::new();
    let mut current = vec![position];
    let mate_positions = loop {
        step += 1;

        let (advanced, memo) = advance(memo_next_turn, current, step)?;
        memo_next_turn = memo;
        match advanced {
            Advanced::Intermediate(nps) => {
                current = nps;
            }
            Advanced::Mate(mates) => {
                break mates;
            }
        }

        std::mem::swap(&mut memo_current_turn, &mut memo_next_turn);

        progress.unbounded_send(step)?;

        eprint!(".");
        std::io::stderr().flush().unwrap();
    };
    eprintln!();

    let mut res = vec![];
    for mate_position in mate_positions {
        res.append(&mut reconstruct_solutions(
            mate_position,
            &memo_next_turn,
            &memo_current_turn,
        ));
    }
    res.sort();
    Ok(res)
}

enum Advanced {
    Intermediate(Vec<Position>),
    Mate(Vec<Position>),
}

const JOBS: usize = 4;

fn advance(
    memo_next: HashMap<Digest, usize>,
    current: Vec<Position>,
    step: usize,
) -> anyhow::Result<(Advanced, HashMap<Digest, usize>)> {
    let memo_next = Arc::new(memo_next);

    let n = current.len();
    let chunks = current.chunks((n + JOBS - 1) / JOBS);

    let mut handles = vec![];
    for chunk in chunks {
        let chunk = chunk.to_vec();
        let memo_next = memo_next.clone();

        handles.push(std::thread::spawn(move || {
            advance_sub(memo_next, chunk, (NTHREAD / JOBS).max(1))
        }));
    }

    let (advanced, visited): (Vec<_>, Vec<_>) = handles
        .into_iter()
        .map(|handle| handle.join().unwrap().unwrap())
        .unzip();
    let mut memo_next = Arc::try_unwrap(memo_next).unwrap();

    for digests in visited {
        for digest in digests {
            memo_next.insert(digest, step);
        }
    }

    let mut next_positions = vec![];
    let mut mate_positions = vec![];
    for x in advanced {
        match x {
            Advanced::Intermediate(mut nps) => {
                next_positions.append(&mut nps);
            }
            Advanced::Mate(mut mates) => {
                mate_positions.append(&mut mates);
            }
        }
    }

    Ok((
        if mate_positions.is_empty() && !next_positions.is_empty() {
            Advanced::Intermediate(next_positions)
        } else {
            Advanced::Mate(mate_positions)
        },
        memo_next,
    ))
}

fn advance_sub(
    memo_next: Arc<HashMap<Digest, usize>>,
    current: Vec<Position>,
    threads: usize,
) -> anyhow::Result<(Advanced, HashSet<Digest>)> {
    let mut mate_positions = vec![];
    let mut next_positions = vec![];
    let mut visited = HashSet::new();

    let handles = {
        let (tx, rx) = std::sync::mpsc::channel::<(Position, Vec<(Position, Digest)>)>();

        let handles = parallel_advance(current, tx, threads);

        while let Ok((position, advanced)) = rx.recv() {
            let mut movable = false;

            for (np, digest) in advanced {
                movable = true;
                if memo_next.contains_key(&digest) || visited.contains(&digest) {
                    continue;
                }
                visited.insert(digest);
                next_positions.push(np);
            }
            if !movable && position.turn() == White && !position.pawn_drop() {
                // Checkmate
                mate_positions.push(position.clone());
            }
        }
        handles
    };
    for handle in handles {
        handle.join().unwrap();
    }

    Ok((
        if mate_positions.is_empty() && !next_positions.is_empty() {
            Advanced::Intermediate(next_positions)
        } else {
            Advanced::Mate(mate_positions)
        },
        visited,
    ))
}

const NTHREAD: usize = 12;
fn parallel_advance(
    current: Vec<Position>,
    tx: std::sync::mpsc::Sender<(Position, Vec<(Position, Digest)>)>,
    threads: usize,
) -> Vec<std::thread::JoinHandle<()>> {
    let current = Arc::new(current);
    let n = current.len();
    let chunk = (n + threads - 1) / threads;

    let mut children = vec![];
    for id in 0..threads {
        let thread_tx = tx.clone();
        let current = current.clone();
        let child = std::thread::spawn(move || {
            for i in (id * chunk)..((id + 1) * chunk).min(n) {
                let position = &current[i];
                let advanced = position::advance(position)
                    .unwrap()
                    .into_iter()
                    .map(|np| {
                        let digest = digest(&np);
                        (np, digest)
                    })
                    .collect();
                thread_tx.send((position.clone(), advanced)).unwrap();
            }
        });
        children.push(child);
    }
    children
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
            let got = solve(board).unwrap();

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
            let got = solve(board.clone()).unwrap();
            let want: Vec<Vec<Movement>> = vec![];
            eprintln!("Solving {:?}", board);
            assert_eq!(got, want);
        }
    }
}
