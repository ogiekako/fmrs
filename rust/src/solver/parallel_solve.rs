use std::sync::Mutex;
use std::time::Instant;

use fmrs_core::memo::DashMemo;

use fmrs_core::position::advance::advance::advance_aux;
use fmrs_core::position::position::PositionAux;
use fmrs_core::position::{Position, PositionExt as _};

use fmrs_core::solve::{reconstruct_solutions, Solution};
use log::debug;
use rayon::prelude::*;

pub(super) fn parallel_solve(
    position: Position,
    _progress: futures::channel::mpsc::UnboundedSender<usize>,
    solutions_upto: usize,
    start: Option<Instant>,
) -> anyhow::Result<Vec<Solution>> {
    let mut memo = DashMemo::default();
    memo.insert(position.digest(), 0);
    let mut memo_next = DashMemo::default();

    let mate_positions: Mutex<Vec<Position>> = Mutex::new(vec![]);

    let mut positions = vec![position];

    next_positions(&mate_positions, &memo_next, &mut positions, 0);
    std::mem::swap(&mut memo, &mut memo_next);

    for step in (1..).step_by(2) {
        if start.is_some() && step % 75 == 0 {
            debug!(
                "{}: {} ({:.1?})",
                step,
                positions.len(),
                start.unwrap().elapsed()
            );
        }

        if positions.is_empty() {
            return Ok(vec![]);
        }

        next_next_positions(
            &mate_positions,
            &mut memo,
            &mut memo_next,
            &mut positions,
            step,
        );

        if !mate_positions.lock().unwrap().is_empty() {
            if let Some(start) = start {
                eprintln!("found mate in {}: {:.1?}", step, start.elapsed());
            }

            let mut res = vec![];
            for mate_position in mate_positions.into_inner().unwrap() {
                res.append(&mut reconstruct_solutions(
                    &mate_position,
                    &mut memo_next.as_mut(),
                    &mut memo.as_mut(),
                    solutions_upto - res.len(),
                ));
            }
            res.sort();
            return Ok(res);
        }
    }
    unreachable!()
}

fn next_next_positions(
    mate_positions: &Mutex<Vec<Position>>,
    memo: &mut DashMemo,
    memo_next: &mut DashMemo,
    positions: &mut Vec<Position>,
    step: u16,
) {
    *positions = positions
        .into_par_iter()
        .flat_map_iter(|position| {
            let mut movements = vec![];
            let is_mate = advance_aux(
                &mut PositionAux::new(position.clone()),
                &mut memo_next.as_mut(),
                step + 1,
                &Default::default(),
                &mut movements,
            )
            .unwrap();

            if is_mate {
                mate_positions.lock().unwrap().push(position.clone());
            }

            movements.into_iter().flat_map(|m| {
                let mut np = position.clone();
                np.do_move(&m);

                let mut movements = vec![];
                advance_aux(
                    &mut PositionAux::new(np.clone()),
                    &mut memo.as_mut(),
                    step + 2,
                    &Default::default(),
                    &mut movements,
                )
                .unwrap();

                movements.into_iter().map(move |m| {
                    let mut np = np.clone();
                    np.do_move(&m);
                    np
                })
            })
        })
        .collect()
}

fn next_positions(
    mate_positions: &Mutex<Vec<Position>>,
    memo_next: &DashMemo,
    positions: &mut Vec<Position>,
    step: u16,
) {
    *positions = positions
        .into_par_iter()
        .flat_map_iter(|position| {
            let mut movements = vec![];
            let is_mate = advance_aux(
                &mut PositionAux::new(position.clone()),
                &mut memo_next.as_mut(),
                step + 1,
                &Default::default(),
                &mut movements,
            )
            .unwrap();

            if is_mate {
                mate_positions.lock().unwrap().push(position.clone());
            }

            movements.into_iter().map(move |m| {
                let mut np = position.clone();
                np.do_move(&m);
                np
            })
        })
        .collect()
}
