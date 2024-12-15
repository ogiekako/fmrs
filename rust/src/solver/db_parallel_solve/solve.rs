use fmrs_core::{
    position::{advance_old, Movement, Position, PositionExt},
    solve::Solution,
};

use super::{db::Database, reconstruct::reconstruct_solutions};

pub fn db_parallel_solve(
    mut initial_position: Position,
    progress: futures::channel::mpsc::UnboundedSender<usize>,
    solutions_upto: usize,
) -> anyhow::Result<Vec<Solution>> {
    let mut current_white_positions = vec![];
    let mut movements = vec![];
    advance_old(&mut initial_position, &mut movements)?;
    if movements.is_empty() {
        return Ok(vec![]);
    }
    let memo_white_positions = Database::new()?;
    for movement in movements.iter() {
        let undo = initial_position.do_move(movement);
        memo_white_positions.insert_if_empty(initial_position.digest(), 0i32)?;
        current_white_positions.push(initial_position.clone());
        initial_position.undo_move(&undo);
    }

    let mut half_step = 1;
    let mut mate_positions = loop {
        let status = step(current_white_positions, &memo_white_positions, half_step)?;

        match status {
            SolveStatus::Intermediate(all_next_white_positions) => {
                current_white_positions = all_next_white_positions
            }
            SolveStatus::Mate(mate_positions) => break mate_positions,
        }

        progress.unbounded_send(half_step as usize * 2)?;
        eprintln!(
            "step = {}, queue = {}",
            half_step * 2,
            current_white_positions.len(),
        );
        half_step += 1;
    };
    mate_positions.sort();
    mate_positions.dedup();

    let mut res = std::thread::Builder::new()
        .stack_size(512 * 1024 * 1024)
        .spawn(move || -> anyhow::Result<Vec<Vec<Movement>>> {
            let mut res = vec![];
            for mate_position in mate_positions {
                res.append(&mut reconstruct_solutions(
                    initial_position.digest(),
                    mate_position,
                    &memo_white_positions,
                    solutions_upto - res.len(),
                )?);
            }
            Ok(res)
        })?
        .join()
        .unwrap()?;
    res.sort();
    Ok(res)
}

const NTHREAD: usize = 32;
fn step(
    current_white_positions: Vec<Position>,
    memo_white_positions: &Database,
    half_step: i32,
) -> anyhow::Result<SolveStatus> {
    let chunk_size = (current_white_positions.len() + NTHREAD - 1) / NTHREAD;
    let chunks = current_white_positions
        .chunks(chunk_size)
        .into_iter()
        .collect::<Vec<_>>();
    let mut handles = vec![];
    for chunk in chunks {
        let chunk = chunk.to_vec();
        let memo_white_positions = memo_white_positions.clone();
        handles.push(std::thread::spawn(
            move || -> anyhow::Result<SolveStatus> {
                step_small(chunk, &memo_white_positions, half_step)
            },
        ));
    }
    drop(current_white_positions);

    let mut all_next_white_positions = vec![];
    let mut mate_positions = vec![];
    for handle in handles {
        let status = handle.join().unwrap()?;
        match status {
            SolveStatus::Intermediate(mut x) => all_next_white_positions.append(&mut x),
            SolveStatus::Mate(mut x) => mate_positions.append(&mut x),
        }
    }
    Ok(
        if mate_positions.is_empty() && !all_next_white_positions.is_empty() {
            SolveStatus::Intermediate(all_next_white_positions)
        } else {
            SolveStatus::Mate(mate_positions)
        },
    )
}

fn step_small(
    mut current_white_positions: Vec<Position>,
    memo_white_positions: &Database,
    half_step: i32,
) -> anyhow::Result<SolveStatus> {
    let mut all_next_white_positions = vec![];
    let mut mate_positions = vec![];

    let mut black_movements = vec![];

    while let Some(mut white_position) = current_white_positions.pop() {
        let mut has_next_position = false;
        let mut white_movements = vec![];
        advance_old(&mut white_position, &mut white_movements)?;

        while let Some(white_movement) = white_movements.pop() {
            has_next_position = true;
            if !mate_positions.is_empty() {
                break;
            }

            let undo_white = white_position.do_move(&white_movement);

            advance_old(&mut white_position, &mut black_movements)?;
            while let Some(black_movement) = black_movements.pop() {
                let black_undo = white_position.do_move(&black_movement);

                let digest = white_position.digest();
                if memo_white_positions.insert_if_empty(digest, half_step)? {
                    white_position.undo_move(&black_undo);
                    continue;
                }
                all_next_white_positions.push(white_position.clone());
                white_position.undo_move(&black_undo);
            }

            white_position.undo_move(&undo_white);
        }

        if !has_next_position && !white_position.pawn_drop() {
            mate_positions.push(white_position);
        }
    }
    Ok(
        if !mate_positions.is_empty() || all_next_white_positions.is_empty() {
            SolveStatus::Mate(mate_positions)
        } else {
            SolveStatus::Intermediate(all_next_white_positions)
        },
    )
}

enum SolveStatus {
    Intermediate(Vec<Position>),
    Mate(Vec<Position>),
}
