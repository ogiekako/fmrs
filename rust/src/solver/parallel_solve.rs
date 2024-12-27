use std::io::Write as _;

use fmrs_core::memo::{DashMemo, MemoTrait};

use fmrs_core::position::advance::advance::advance_aux;
use fmrs_core::position::position::PositionAux;
use fmrs_core::position::{Position, PositionExt as _};

use fmrs_core::solve::{reconstruct_solutions, Solution};
use rayon::prelude::*;

pub(super) fn parallel_solve(
    position: Position,
    _progress: futures::channel::mpsc::UnboundedSender<usize>,
    solutions_upto: usize,
) -> anyhow::Result<Vec<Solution>> {
    let mut memo = DashMemo::default();
    memo.contains_or_insert(position.digest(), 0);
    let mut memo_next = DashMemo::default();

    let mut positions = vec![position];
    let mut next_positions = vec![];

    for step in 1.. {
        std::io::stderr().flush().unwrap();

        if positions.is_empty() {
            return Ok(vec![]);
        }
        let mate_nexts = positions
            .into_par_iter()
            .map(|position| {
                let mut movements = vec![];
                let is_mate = advance_aux(
                    &mut PositionAux::new(position.clone()),
                    &memo_next,
                    step,
                    &Default::default(),
                    &mut movements,
                )
                .unwrap();

                let mut next_positions = vec![];
                for movement in movements {
                    let mut next_position = position.clone();
                    next_position.do_move(&movement);
                    next_positions.push(next_position);
                }

                (is_mate.then(|| position), next_positions)
            })
            .collect::<Vec<_>>();

        let mate_positions = mate_nexts
            .iter()
            .filter_map(|(mate, _)| mate.clone())
            .collect::<Vec<_>>();

        if !mate_positions.is_empty() {
            let mut res = vec![];
            for mate_position in mate_positions.iter() {
                res.append(&mut reconstruct_solutions(
                    mate_position,
                    &memo_next,
                    &memo,
                    solutions_upto - res.len(),
                ));
            }
            res.sort();
            return Ok(res);
        }

        for (_, next_position) in mate_nexts {
            next_positions.extend(next_position);
        }

        std::mem::swap(&mut memo, &mut memo_next);
        positions = vec![];
        std::mem::swap(&mut positions, &mut next_positions);
    }
    unreachable!()
}
