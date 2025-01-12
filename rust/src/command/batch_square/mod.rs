pub mod csp;
pub mod frame;
pub mod mate;
pub mod room;

use std::sync::Mutex;

use fmrs_core::{piece::Kind, position::position::PositionAux, search::backward::backward_search};
use frame::FrameFilter;
use log::info;
use mate::MateFilter;
// use rand::{rngs::SmallRng, seq::SliceRandom, SeedableRng};
use rayon::prelude::*;
use room::RoomFilter;
use serde::{Deserialize, Serialize};

pub fn batch_square(filter_file: Option<String>) -> anyhow::Result<()> {
    let filter = if let Some(filter_file) = &filter_file {
        serde_json::from_str::<MateFilter>(&std::fs::read_to_string(filter_file)?)?
    } else {
        MateFilter {
            frame_filter: FrameFilter {
                room_filter: RoomFilter {
                    width: vec![5],
                    height: 2..=5,
                    weakly_decreasing: false,
                    feasible_without_stone: true,
                    area: Some(12..=20),
                },
                max_empty_black_pawn_col: Some(1),
                max_empty_white_pawn_col: Some(2),
            },
            attackers: vec![Kind::Bishop, Kind::Knight],
            no_redundant: true,
            no_less_pro_pawn: 1,
            max_extra_white_hand_pawn: 1.into(),
        }
    };
    eprintln!(
        "Search started with params: {}",
        serde_json::to_string_pretty(&filter)?
    );

    let positions = filter.generate_mates();
    // positions.shuffle(&mut SmallRng::seed_from_u64(20250105));

    if positions.is_empty() {
        eprintln!("No matching positions found");
        return Ok(());
    }

    eprintln!("{} positions {:?}", positions.len(), positions[0]);

    let iter = Mutex::new(0);
    let total_len = positions.len();

    let all_problems: Mutex<Vec<(u16, Vec<PositionAux>)>> = Mutex::new(vec![]);
    let best_problems = Mutex::new((0, vec![]));

    positions.into_par_iter().for_each(|position| {
        let (step, problems) = backward_search(&position, true, 0).unwrap();

        {
            let mut all_problems = all_problems.lock().unwrap();
            match all_problems.iter_mut().find(|(s, _)| *s == step) {
                Some((_, ps)) => ps.extend_from_slice(&problems),
                None => all_problems.push((step, problems.clone())),
            }
        }

        let (best_step, best_url) = {
            let mut best_problems = best_problems.lock().unwrap();
            match step.cmp(&best_problems.0) {
                std::cmp::Ordering::Less => return,
                std::cmp::Ordering::Greater => *best_problems = (step, problems),
                std::cmp::Ordering::Equal => best_problems.1.extend(problems),
            }
            (best_problems.0, best_problems.1.last().unwrap().sfen_url())
        };

        let mut i = iter.lock().unwrap();
        *i += 1;
        if *i % 50 == 0 {
            info!("back {}/{} best {} {}", i, total_len, best_step, best_url);
        }
    });

    let best_problems = best_problems.into_inner().unwrap();
    let mut all_problems = all_problems.into_inner().unwrap();

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

fn log_results(filter: &MateFilter, problems: &[(u16, Vec<PositionAux>)]) -> anyhow::Result<()> {
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

    let mut attackers = filter.attackers.clone();
    attackers.sort();
    let kind = filter
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
        .join("-");

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
    filter: MateFilter,
    problems: Vec<Problem>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Problem {
    step: usize,
    sfen: String,
    url: String,
}
