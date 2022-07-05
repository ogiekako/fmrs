use std::{
    collections::HashMap,
    io::Write,
    sync::{Arc, Mutex},
};

use rand::{prelude::SliceRandom, SeedableRng};

use crate::{
    piece::Color,
    position::{advance, Position},
};

use super::{
    reconstruct::reconstruct_solutions,
    solve::{digest, Digest},
    Solution,
};

pub(super) fn solve(
    position: Position,
    progress: futures::channel::mpsc::UnboundedSender<usize>,
) -> anyhow::Result<Vec<Solution>> {
    let step = 0;
    let mut memo = HashMap::new();
    memo.insert(digest(&position), step);
    let memo_next = HashMap::new();
    let all_positions = vec![position];
    let mut res = solve_sub(
        all_positions,
        memo,
        memo_next,
        Mutex::new(None).into(),
        step,
        progress,
        None,
    )?;
    res.sort();
    Ok(res)
}

#[cfg(test)]
const TRIGGER_PARALLEL_SOLVE: usize = 2;
#[cfg(not(test))]
const TRIGGER_PARALLEL_SOLVE: usize = 2_000_000;

const NTHREAD: usize = 15;

fn solve_sub(
    mut all_positions: Vec<Position>,
    mut memo: HashMap<Digest, usize>,
    mut memo_next: HashMap<Digest, usize>,
    mate_in: Arc<Mutex<Option<usize>>>,
    current_step: usize,
    progress: futures::channel::mpsc::UnboundedSender<usize>,
    thread_id: Option<usize>,
) -> anyhow::Result<Vec<Solution>> {
    let mut mate_positions = vec![];
    let mut all_next_positions = Vec::new();
    for step in current_step.. {
        if step > 10 && all_positions.len() >= TRIGGER_PARALLEL_SOLVE && thread_id.is_none() {
            let chunk_size = (all_positions.len() + NTHREAD - 1) / NTHREAD;
            let mut handles = vec![];
            for (id, chunk) in all_positions.chunks(chunk_size).enumerate() {
                let all_positions = chunk.to_vec();
                let memo = memo.clone();
                let memo_next = memo_next.clone();
                let mate_in = mate_in.clone();
                let progress = progress.clone();
                handles.push(std::thread::spawn(move || {
                    solve_sub(
                        all_positions,
                        memo,
                        memo_next,
                        mate_in,
                        step,
                        progress,
                        Some(id),
                    )
                }));
            }
            let mut all_solutions = vec![];
            for handle in handles {
                all_solutions.append(&mut handle.join().unwrap()?);
            }
            if all_solutions.is_empty() {
                return Ok(all_solutions);
            }
            let mate_in = mate_in.lock().unwrap().unwrap();
            let mut shortest_solutions = vec![];
            for solution in all_solutions {
                if solution.len() == mate_in {
                    shortest_solutions.push(solution);
                }
            }
            return Ok(shortest_solutions);
        }

        let mate_bound = mate_in.lock().unwrap().unwrap_or(usize::MAX);
        if step > mate_bound {
            return Ok(vec![]);
        }

        while let Some(position) = all_positions.pop() {
            let mut has_next_position = false;
            let next_positions = advance(&position)?;

            for np in next_positions {
                has_next_position = true;

                if step == mate_bound {
                    break;
                }

                let digest = digest(&np);
                if memo_next.contains_key(&digest) {
                    continue;
                }
                memo_next.insert(digest, step + 1);
                all_next_positions.push(np);
            }
            if !has_next_position && position.turn() == Color::White && !position.pawn_drop() {
                mate_positions.push(position);

                let mut g = mate_in.lock().unwrap();
                if g.is_none() || g.unwrap() > step {
                    *g = Some(step);
                }
            }
        }
        if !mate_positions.is_empty() || all_next_positions.is_empty() {
            break;
        }

        std::mem::swap(&mut memo, &mut memo_next);
        std::mem::swap(&mut all_positions, &mut all_next_positions);

        progress.unbounded_send(current_step)?;

        eprintln!(
            "{}: step {}: queue len = {}",
            thread_id
                .map(|i| format!("child {}", i))
                .unwrap_or_else(|| "parent".into()),
            step,
            all_positions.len(),
        );
        std::io::stderr().flush().unwrap();
    }

    let mut res = vec![];
    for mate_position in mate_positions {
        res.append(&mut reconstruct_solutions(mate_position, &memo_next, &memo));
    }
    Ok(res)
}
