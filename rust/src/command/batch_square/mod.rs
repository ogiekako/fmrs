pub mod csp;
pub mod frame;
pub mod mate;
pub mod room;

use std::{collections::HashSet, path::PathBuf, sync::Mutex};

use fmrs_core::{
    nohash::NoHashSet64,
    piece::{Color, Kind, KINDS, NUM_HAND_KIND},
    position::{position::PositionAux, Hands},
    search::backward::backward_search,
    solve::standard_solve::standard_solve,
};
use frame::{Frame, FrameFilter};
use log::info;
use mate::MateFilter;
// use rand::{rngs::SmallRng, seq::SliceRandom, SeedableRng};
use rayon::prelude::*;
use regex::Regex;
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
                    height: 3..=4,
                    weakly_decreasing: false,
                    feasible_without_stone: true,
                    area: Some(17..=17),
                },
                max_empty_black_pawn_col: Some(2),
                max_empty_white_pawn_col: Some(1),
            },
            attackers: vec![Kind::Rook],
            no_redundant: true,
            no_less_pro_pawn: 1,
            max_extra_white_hand_pawn: Some(0),
            skip_known_mates: true,
        }
    };
    eprintln!(
        "Search started with params: {}",
        serde_json::to_string_pretty(&filter)?
    );

    let mut mates = filter.generate_mates();

    if filter.skip_known_mates {
        info!("Filtering {} mate positions with known mates", mates.len(),);
        retain_unknown_mates(&mut mates);
    }
    // mates.shuffle(&mut SmallRng::seed_from_u64(20250105));

    if mates.is_empty() {
        eprintln!("No matching mates found");
        return Ok(());
    }

    eprintln!("{} mates {:?}", mates.len(), mates[0].1.sfen_url());

    let iter = Mutex::new(0);
    let total_len = mates.len();

    let all_problems: Mutex<Vec<(u16, Vec<PositionAux>)>> = Mutex::new(vec![]);
    let best_problems = Mutex::new((0, vec![]));

    mates.into_par_iter().for_each(|(_, mate)| {
        let (step, problems) = backward_search(&mate, true, 0).unwrap();

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

fn used_pieces(position: &PositionAux) -> Hands {
    let mut res = Hands::default();
    for &k in KINDS[0..NUM_HAND_KIND].iter() {
        let mut count = 0;
        for c in Color::iter() {
            count += position.hands().count(c, k);
            count += position.bitboard(c, k).count_ones() as usize;
            if let Some(k) = k.promote() {
                count += position.bitboard(c, k).count_ones() as usize;
            }
        }
        res.add_n(Color::WHITE, k, count);
    }
    res
}

fn retain_unknown_mates(mates: &mut Vec<(Frame, PositionAux)>) {
    let Ok(files) = std::fs::read_dir(log_dir()) else {
        return;
    };

    let mut pieces_to_check = HashSet::new();
    for mate in mates.iter() {
        pieces_to_check.insert(used_pieces(&mate.1));
    }
    let mut frames = HashSet::new();
    for (frame, _) in mates.iter() {
        frames.insert(frame.clone());
    }

    let re = Regex::new(r#""((:?[^/ ]+/){8}[^/ ]+ [wb] [^ ]+ -?\d+)""#).unwrap();
    let mut positions = vec![];

    for file in files {
        let Ok(file) = file else { continue };
        if file.path().extension().and_then(|x| x.to_str()) != Some("json") {
            continue;
        }
        let Ok(content) = std::fs::read_to_string(file.path()) else {
            continue;
        };
        for cap in re.captures_iter(&content) {
            if let Ok(position) = PositionAux::from_sfen(&cap[1]) {
                if position.is_illegal_initial_position() {
                    continue;
                }
                if !pieces_to_check.contains(&used_pieces(&position)) {
                    continue;
                }
                if !frames.iter().any(|frame| frame.matches(&position)) {
                    continue;
                }
                positions.push(position);
            }
        }
    }
    positions.sort_by_key(|position| position.digest());
    positions.dedup();

    let iter = Mutex::new(0);
    let total_len = positions.len();

    let known_mate_digests: NoHashSet64 = positions
        .into_par_iter()
        .filter_map(|mut position| {
            let Ok(mut solutions) = standard_solve(position.clone(), 1, true) else {
                return None;
            };
            if solutions.is_empty() {
                return None;
            }
            for m in solutions.remove(0) {
                position.do_move(&m);
            }

            {
                let mut i = iter.lock().unwrap();
                *i += 1;
                if *i % 100 == 0 {
                    info!("known {}/{}", *i, total_len);
                }
            }

            Some(position.digest())
        })
        .collect();

    mates.retain(|(_, position)| !known_mate_digests.contains(&position.digest()));
}

fn log_dir() -> PathBuf {
    std::path::Path::new(file!()).with_file_name("logs")
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
    let log_dir = log_dir();

    let mut attackers = filter.attackers.clone();
    attackers.sort();
    let kind = attackers
        .iter()
        .map(|k| match k {
            Kind::Pawn => "pawn",
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
