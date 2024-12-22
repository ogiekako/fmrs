use std::{io::Write as _, time::Instant, usize};

use fmrs_core::{
    piece::Color,
    position::{Position, PositionExt},
};
use log::{debug, info};
use rand::{rngs::SmallRng, seq::SliceRandom, Rng, SeedableRng};
use rayon::prelude::*;

use super::{action::Action, solve::one_way_mate_steps};

pub(super) fn generate_one_way_mate_with_beam(
    mut seed: u64,
    parallel: usize,
    goal: Option<usize>,
) -> anyhow::Result<()> {
    info!(
        "generate_one_way_mate_with_beam: seed={} parallel={}",
        seed, parallel,
    );

    let start_time = Instant::now();

    let mut best_problems: Vec<Problem> = vec![];
    loop {
        eprint!(".");
        std::io::stderr().flush().unwrap();

        let problems = generate(
            &mut seed,
            parallel,
            best_problems.get(0).cloned(),
            &start_time,
        );

        for problem in problems {
            if best_problems.is_empty() || best_problems[0].step < problem.step {
                best_problems.clear();

                println!(
                    "{} {} ({:.1?})",
                    problem.step,
                    problem.position.sfen_url(),
                    start_time.elapsed()
                );
                if problem.step >= goal.unwrap_or(usize::MAX) {
                    return Ok(());
                }

                best_problems.push(problem);
            } else if best_problems[0].step == problem.step {
                best_problems.push(problem);
            }
        }
    }
}

const SEARCH_DEPTH: usize = 8;
const SEARCH_ITER_MULT: usize = 15000;
const USE_MULT: usize = 4;
const MAX_PRODUCE: usize = 2;

fn insert(all_problems: &mut Vec<Vec<Problem>>, mut position: Position, min_step: usize) {
    let mut movements = vec![];
    let step = one_way_mate_steps(&position, &mut movements).unwrap();

    if step >= all_problems.len() {
        all_problems.resize(step + 1, vec![]);
    }
    all_problems[step].push(Problem::new(position.clone(), step));
    for (i, movement) in movements.iter().enumerate() {
        if step - 1 - i < min_step {
            break;
        }
        position.do_move(movement);
        all_problems[step - 1 - i].push(Problem::new(position.clone(), step - 1 - i));
    }
}

fn generate(
    seed: &mut u64,
    parallel: usize,
    prev_best: Option<Problem>,
    start_time: &Instant,
) -> Vec<Problem> {
    let mut all_problems: Vec<Vec<Problem>> = vec![];

    if let Some(best) = prev_best.as_ref() {
        insert(&mut all_problems, best.position.clone(), 0);
    }

    let initial_cands = random_one_way_mate_positions(*seed, parallel);
    *seed += parallel as u64;

    for cand in initial_cands.into_iter() {
        insert(&mut all_problems, cand.position, 0);
    }

    for step in 0.. {
        if step >= all_problems.len() {
            break;
        }

        all_problems[step].shuffle(&mut SmallRng::seed_from_u64(*seed));
        *seed += 1;
        all_problems[step].truncate(parallel);

        if step >= prev_best.as_ref().map(|p| p.step + 1).unwrap_or(0) {
            info!(
                "step = {} #problems = {} best = {} elapsed={:.1?}",
                step,
                all_problems[step].len(),
                all_problems.len() - 1,
                start_time.elapsed()
            );
        }
        if all_problems[step].is_empty() {
            continue;
        }

        if !all_problems[step].is_empty() {
            let mut i = 0;
            while all_problems[step].len() < parallel {
                let p = all_problems[step][i].clone();
                all_problems[step].push(p);
                i += 1;
            }
        }

        let base_seed = *seed;
        let new_positions = all_problems[step]
            .par_iter_mut()
            .enumerate()
            .map(|(i, problem)| {
                assert_eq!(step, problem.step);

                let mut rng = SmallRng::seed_from_u64(base_seed + i as u64);

                let num_use = (USE_MULT as f64 * ((step as f64 + 1.).log10() + 1.)).ceil() as usize;

                let mut count = 0;
                let mut new_positions = vec![];

                for _ in 0..num_use {
                    if problem.produced >= MAX_PRODUCE {
                        break;
                    }
                    if let Some(new_problem) = compute_better_problem(&mut rng, problem) {
                        count += new_problem.step - problem.step;

                        new_positions.push(new_problem.position);
                        problem.produced += 1;
                    }

                    if count >= MAX_PRODUCE {
                        break;
                    }
                }

                new_positions
            })
            .collect::<Vec<_>>()
            .concat();

        *seed += parallel as u64;

        for new_position in new_positions {
            insert(&mut all_problems, new_position, step + 1);
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
        (SEARCH_ITER_MULT as f64 * ((problem.step as f64 + 1.).log10() + 1.)).ceil() as usize;
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
                debug!("generate_one_way_mate_positions: {}", i + 1);
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
