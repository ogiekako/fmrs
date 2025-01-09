pub mod csp;
pub mod frame;
pub mod mate_formation;
pub mod room;

use fmrs_core::{piece::Kind, position::position::PositionAux, search::backward::backward_search};
use frame::FrameFilter;
use log::info;
use mate_formation::MateFormationFilter;
// use rand::{rngs::SmallRng, seq::SliceRandom, SeedableRng};
use rayon::prelude::*;
use room::RoomFilter;
use serde::{Deserialize, Serialize};

pub fn batch_square(filter_file: Option<String>) -> anyhow::Result<()> {
    let filter = if let Some(filter_file) = &filter_file {
        serde_json::from_str::<FrameFilter>(&std::fs::read_to_string(filter_file)?)?
    } else {
        FrameFilter {
            room_filter: RoomFilter {
                width: vec![7, 9],
                height: 2..=4,
                weakly_decreasing: false,
                feasible_without_stone: true,
                area: Some(12..=24),
            },
            no_black_pawn_count: Some(1..=3),
            no_white_pawn_count: Some(1..=3),
            mate_formation_filter: Some(MateFormationFilter {
                attackers: vec![Kind::Rook],
                no_redundant: true,
                unique: false,
                no_less_pro_pawn: 1,
                pawn_maximally_constrained: true,
            }),
        }
    };
    eprintln!(
        "Search started with params: {}",
        serde_json::to_string_pretty(&filter)?
    );

    let frames = filter.generate_frames();

    let positions: Vec<_> = frames
        .into_iter()
        .filter_map(|(_, metadata)| metadata.mate_with_minimum_pawn)
        .flatten()
        .collect();
    // positions.shuffle(&mut SmallRng::seed_from_u64(20250105));

    if positions.is_empty() {
        eprintln!("No matching positions found");
        return Ok(());
    }

    eprintln!("{} positions {:?}", positions.len(), positions[0]);

    let chunk_size = 50;
    let chunks = positions.chunks(chunk_size).collect::<Vec<_>>();

    let mut all_problems = vec![];
    let mut best_problems = (0, vec![]);
    for (i, chunk) in chunks.into_iter().enumerate() {
        let problems = chunk
            .into_par_iter()
            .map(|position| {
                let res = backward_search(position, true, 0).unwrap();
                debug_assert!(!res.1.is_empty(), "{} {:?}", res.0, position);
                res
            })
            .collect::<Vec<_>>();

        for (step, positions) in problems {
            all_problems.push((step, positions.clone()));
            match step.cmp(&best_problems.0) {
                std::cmp::Ordering::Less => continue,
                std::cmp::Ordering::Greater => best_problems = (step, positions),
                std::cmp::Ordering::Equal => best_problems.1.extend(positions),
            }
        }
        info!(
            "{}/{} best {} {:?}",
            ((i + 1) * chunk_size).min(positions.len()),
            positions.len(),
            best_problems.0,
            best_problems.1.last().unwrap(),
        );
    }
    eprintln!("mate in {} ({}):", best_problems.0, best_problems.1.len());
    for position in best_problems.1.iter() {
        eprintln!("{}", position.sfen_url());
        println!("{}", position.sfen());
    }

    all_problems.sort_by_key(|(step, _)| *step);

    if let Err(err) = log_results(&filter, &all_problems) {
        info!("logging failed: {}", err);
    }

    Ok(())
}

fn log_results(filter: &FrameFilter, problems: &[(u16, Vec<PositionAux>)]) -> anyhow::Result<()> {
    let result = RunResult {
        filter: filter.clone(),
        problems: problems
            .iter()
            .flat_map(|(step, positions)| {
                positions.iter().map(|position| Problem {
                    step: *step as usize,
                    sfen: position.sfen(),
                    url: position.sfen_url(),
                })
            })
            .collect(),
    };
    let log_dir = std::path::Path::new(file!()).with_file_name("logs");

    let kind = if let Some(mate_filter) = &filter.mate_formation_filter {
        let mut attackers = mate_filter.attackers.clone();
        attackers.sort();
        mate_filter
            .attackers
            .iter()
            .map(|k| match k {
                Kind::Lance => "lance",
                Kind::Knight => "knight",
                Kind::Silver => "silver",
                Kind::Gold => "gold",
                Kind::Bishop => "bishop",
                Kind::Rook => "rook",
                _ => "unknown",
            })
            .collect::<Vec<_>>()
            .join("-")
    } else {
        "unknown".to_string()
    };

    let best_step = problems.iter().map(|(step, _)| *step).max().unwrap_or(0);

    let name = format!(
        "{}_{:03}_{}.json",
        kind,
        best_step,
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs()
    );

    let log_file = log_dir.join(name);

    std::fs::create_dir_all(log_dir)?;
    std::fs::write(&log_file, serde_json::to_string_pretty(&result)?)?;

    eprintln!("result written to: {}", log_file.display());

    Ok(())
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct RunResult {
    filter: FrameFilter,
    problems: Vec<Problem>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Problem {
    step: usize,
    sfen: String,
    url: String,
}
