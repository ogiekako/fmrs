use actix_web::rt::time::Instant;
use fmrs_core::{
    piece::Color,
    position::{Position, PositionExt},
};
use log::info;
use rand::{rngs::SmallRng, seq::SliceRandom, Rng, SeedableRng};
use rayon::prelude::*;

use super::{action::Action, solve::one_way_mate_steps};

pub(super) fn generate_one_way_mate_with_beam(
    seed: u64,
    start: usize,
    parallel: usize,
) -> anyhow::Result<()> {
    info!(
        "generate_one_way_mate_with_beam: seed={} start={} parallel={}",
        seed, start, parallel,
    );

    let problems = generate(seed, start, parallel);

    for problem in problems.iter() {
        println!(
            "generated problem (step = {}): {}",
            problem.step,
            problem.position.sfen_url()
        );
    }

    Ok(())
}

const SEARCH_DEPTH: usize = 7;
const SEARCH_ITER_MULT: usize = 15000;
const USE_MULT: usize = 4;
const MAX_PRODUCE: usize = 2;

fn insert(all_problems: &mut Vec<Vec<Problem>>, mut position: Position) {
    let mut movements = vec![];
    let step = one_way_mate_steps(&position, &mut movements).unwrap();

    if step >= all_problems.len() {
        all_problems.resize(step + 1, vec![]);
    }
    all_problems[step].push(Problem::new(position.clone(), step));
    for (i, movement) in movements.iter().enumerate() {
        position.do_move(movement);
        all_problems[step - 1 - i].push(Problem::new(position.clone(), step - 1 - i));
    }
}

fn generate(mut seed: u64, bucket: usize, parallel: usize) -> Vec<Problem> {
    let mut all_problems: Vec<Vec<Problem>> = vec![];

    let initial_cands = random_one_way_mate_positions(seed, bucket);
    seed += bucket as u64;

    for cand in initial_cands.into_iter() {
        insert(&mut all_problems, cand.position);
    }

    let start = Instant::now();
    for step in 0.. {
        if step >= all_problems.len() {
            break;
        }

        all_problems[step].shuffle(&mut SmallRng::seed_from_u64(seed));
        seed += 1;
        all_problems[step].truncate(bucket);

        let n = all_problems[step].len();

        info!(
            "step = {} #problems = {} best = {} elapsed={:.1}s",
            step,
            n,
            all_problems.len() - 1,
            start.elapsed().as_secs_f64()
        );
        if n == 0 {
            continue;
        }

        let chunks = all_problems[step]
            .chunks_mut((n + parallel - 1) / parallel)
            .collect::<Vec<_>>();

        let new_positions = chunks
            .into_par_iter()
            .enumerate()
            .map(|(i, problems)| {
                let mut rng = SmallRng::seed_from_u64(seed + i as u64);

                let num_use = (USE_MULT as f64 * ((step as f64 + 1.).log2() + 1.)).ceil() as usize;

                let mut count = 0;
                let mut new_positions = vec![];
                for _ in 0..num_use {
                    for problem in problems.iter_mut() {
                        assert_eq!(step, problem.step);

                        if problem.produced >= MAX_PRODUCE {
                            continue;
                        }
                        if let Some(new_problem) = compute_better_problem(&mut rng, problem) {
                            count += new_problem.step - problem.step;

                            new_positions.push(new_problem.position);
                            problem.produced += 1;
                        }
                    }
                    if count * parallel >= bucket {
                        break;
                    }
                }
                new_positions
            })
            .collect::<Vec<_>>()
            .concat();

        seed += bucket as u64;

        for new_position in new_positions {
            insert(&mut all_problems, new_position);
        }
    }

    all_problems.remove(all_problems.len() - 1)
}

fn compute_better_problem(rng: &mut SmallRng, problem: &Problem) -> Option<Problem> {
    let mut position = problem.position.clone();
    let mut solvable_position = position.clone();
    let mut inferior_count = 0;

    let mut movements = vec![];

    let iteration =
        (SEARCH_ITER_MULT as f64 * ((problem.step as f64 + 1.).log2() + 1.)).ceil() as usize;
    for _ in 0..iteration {
        let action = random_action(rng, true);
        if action.try_apply(&mut position).is_err() {
            continue;
        }
        movements.clear();
        let step = one_way_mate_steps(&position, &mut movements);

        if step.is_none() || step.unwrap() < problem.step {
            inferior_count += 1;
            if inferior_count >= SEARCH_DEPTH {
                position = solvable_position.clone();
            }
            continue;
        }

        let step = step.unwrap();

        if step > problem.step {
            return Problem::new(position, step).into();
        }

        solvable_position = position.clone();
    }
    None
}

fn random_action(rng: &mut SmallRng, allow_black_capture: bool) -> Action {
    loop {
        match rng.gen_range(0..100) {
            0..=9 => return Action::Move(rng.gen(), rng.gen()),
            10..=19 => return Action::Swap(rng.gen(), rng.gen()),
            20..=29 => return Action::FromHand(rng.gen(), rng.gen(), rng.gen(), rng.gen()),
            30..=39 => {
                return Action::ToHand(
                    rng.gen(),
                    if allow_black_capture {
                        rng.gen()
                    } else {
                        Color::WHITE
                    },
                )
            }
            40..=49 => return Action::Shift(rng.gen()),
            50..=59 => return Action::ChangeTurn,
            _ => (),
        }
    }
}

fn random_one_way_mate_positions(seed: u64, count: usize) -> Vec<Problem> {
    let initial_position =
        Position::from_sfen("4k4/9/9/9/9/9/9/9/4K4 b 2r2b4g4s4n4l18p 1").unwrap();

    (0..count)
        .into_par_iter()
        .map(|i| {
            let mut rng = SmallRng::seed_from_u64(seed + i as u64);
            if (i + 1) % 1000 == 0 {
                info!("generate_one_way_mate_positions: {}", i + 1);
            }
            let mut position = initial_position.clone();

            loop {
                let action = random_action(&mut rng, false);
                if action.try_apply(&mut position).is_err() {
                    continue;
                }
                if let Some(step) = one_way_mate_steps(&position, &mut vec![]) {
                    return Problem::new(position, step);
                }
            }
        })
        .collect::<Vec<_>>()
}

#[derive(Clone, Debug)]
struct Problem {
    position: Position,
    step: usize,
    produced: usize,
}

impl Problem {
    fn new(position: Position, step: usize) -> Self {
        Self {
            position,
            step,
            produced: 0,
        }
    }
}
