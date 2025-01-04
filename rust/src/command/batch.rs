use std::io::Read as _;

use fmrs_core::{position::position::PositionAux, solve::Solution};
use log::info;
use rayon::prelude::*;

use crate::solver::standard_solve::standard_solve;

use super::parse_to_sfen;

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum Criteria {
    MaxUnique,
    AllUnique,
}

pub fn batch(
    sfen_file: Option<String>,
    criteria: Criteria,
) -> anyhow::Result<Vec<(PositionAux, Solution)>> {
    let content = if let Some(file) = sfen_file {
        std::fs::read_to_string(file)?
    } else {
        let mut input = "".to_string();
        std::io::stdin().read_to_string(&mut input)?;
        input
    };
    let positions = content
        .trim()
        .split("\n")
        .map(|sfen| PositionAux::from_sfen(&parse_to_sfen(sfen)?))
        .collect::<Result<Vec<_>, _>>()?;

    batch_solve(positions, criteria)
}

pub fn batch_solve(
    positions: Vec<PositionAux>,
    criteria: Criteria,
) -> anyhow::Result<Vec<(PositionAux, Solution)>> {
    let len = positions.len();

    let mut unique = vec![];

    let mut best = (0, "".to_string());

    let chunk_size = 1000;
    for (i, chunk) in positions.chunks(chunk_size).into_iter().enumerate() {
        let mut sol = chunk
            .to_vec()
            .into_par_iter()
            .flat_map(|position| match standard_solve(position.clone(), 2, true) {
                Ok(mut solution) => {
                    if solution.len() != 1 {
                        return None;
                    } else {
                        let solution = solution.remove(0);
                        return Some(Ok((position, solution)));
                    }
                }
                Err(e) => Some(Err(e)),
            })
            .collect::<Result<Vec<_>, _>>()?;

        for (position, solution) in sol.iter_mut() {
            if solution.0.len() > best.0 {
                best = (solution.0.len(), position.sfen_url());
            }
        }
        info!("{} / {} best = {} {}", i * chunk_size, len, best.0, best.1);

        unique.append(&mut sol);
    }

    Ok(match criteria {
        Criteria::MaxUnique => {
            let max_len = unique
                .iter()
                .map(|(_, solution)| solution.0.len())
                .max()
                .unwrap_or(0);
            unique.retain(|(_, solution)| solution.0.len() == max_len);
            unique
        }
        Criteria::AllUnique => unique,
    })
}
