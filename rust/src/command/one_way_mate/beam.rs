use std::{
    collections::BTreeMap,
    hash::{Hash as _, Hasher as _},
    io::Write as _,
    time::Instant,
    usize,
};

use fmrs_core::{
    piece::Color,
    position::{Movement, Position, PositionExt},
};
use log::{debug, info};
use rand::{rngs::SmallRng, seq::SliceRandom, Rng, SeedableRng};
use rayon::prelude::*;
use rustc_hash::FxHasher;

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
    for i in 0.. {
        eprint!(".");
        std::io::stderr().flush().unwrap();

        if (i + 1) % 10 == 0 {
            info!(
                "best = {} (one iter in {:.1?})",
                best_problems[0].step,
                start_time.elapsed() / (i + 1),
            );
        }

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
    unreachable!()
}

const SEARCH_DEPTH: usize = 8;
const SEARCH_ITER_MULT: usize = 10000;
const USE_MULT: usize = 1;
const MAX_PRODUCE: usize = 1;

fn insert(all_problems: &mut Vec<Vec<Problem>>, problem: Problem, min_step: usize) {
    let mut movements = vec![];
    let step = one_way_mate_steps(&problem.position, &mut movements).unwrap();

    if step >= all_problems.len() {
        all_problems.resize(step + 1, vec![]);
    }
    all_problems[step].push(problem.clone());

    let mut position = problem.position.clone();
    for (i, movement) in movements.iter().enumerate() {
        if step - 1 - i < min_step {
            break;
        }
        position.do_move(movement);
        all_problems[step - 1 - i].push(Problem::new(
            position.clone(),
            step - 1 - i,
            &movements[i + 1..],
        ));
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
        insert(&mut all_problems, best.clone(), 0);
    }

    let initial_cands = random_one_way_mate_positions(*seed, parallel);
    *seed += parallel as u64;

    for cand in initial_cands.into_iter() {
        insert(&mut all_problems, cand, 0);
    }

    let mut rng = SmallRng::seed_from_u64(*seed);
    *seed += 1;

    for step in 0.. {
        if step >= all_problems.len() {
            break;
        }

        all_problems[step].shuffle(&mut rng);

        let mut buckets = BTreeMap::new();
        for problem in all_problems[step].iter() {
            buckets
                .entry(problem.white_movements.clone())
                .or_insert_with(Vec::new)
                .push(problem);
        }
        let mut keys = buckets.keys().cloned().collect::<Vec<_>>();
        keys.shuffle(&mut rng);
        let mut problems = vec![];
        'outer: while !buckets.is_empty() {
            for key in keys.iter() {
                let Some(ps) = buckets.get_mut(key) else {
                    continue;
                };
                problems.push(ps.pop().unwrap());
                if problems.len() >= parallel {
                    break 'outer;
                }
                if ps.is_empty() {
                    buckets.remove(key);
                }
            }
        }

        if step >= prev_best.as_ref().map(|p| p.step + 1).unwrap_or(0) {
            info!(
                "step = {} #problems = {} best = {} elapsed={:.1?}",
                step,
                problems.len(),
                all_problems.len() - 1,
                start_time.elapsed()
            );
        }
        if problems.is_empty() {
            continue;
        }

        let mut i = 0;
        while problems.len() < parallel {
            let p = problems[i];
            problems.push(p);
            i += 1;
        }

        let base_seed = *seed;
        *seed += parallel as u64;

        let new_problems = problems
            .into_par_iter()
            .enumerate()
            .map(|(i, problem)| {
                let mut problem = problem.clone();

                assert_eq!(step, problem.step);

                let mut rng = SmallRng::seed_from_u64(base_seed + i as u64);

                let num_use = (USE_MULT as f64 * ((step as f64 + 1.).log10() + 1.)).ceil() as usize;

                let mut count = 0;
                let mut new_problems = vec![];

                for _ in 0..num_use {
                    match compute_better_problem(&mut rng, &problem) {
                        Ok(new_problem) => {
                            count += new_problem.step - problem.step;

                            new_problems.push(new_problem);
                        }
                        Err(modified_problem) => problem = modified_problem,
                    }

                    if count >= MAX_PRODUCE {
                        break;
                    }
                }

                new_problems
            })
            .collect::<Vec<_>>()
            .concat();

        for new_problem in new_problems {
            insert(&mut all_problems, new_problem, step + 1);
        }
    }

    all_problems.remove(all_problems.len() - 1)
}

fn compute_better_problem(rng: &mut SmallRng, problem: &Problem) -> Result<Problem, Problem> {
    let mut position = problem.position.clone();
    let mut solvable_position = position.clone();
    let mut inferior_count = 0;

    let mut movements = vec![];

    let iteration =
        (SEARCH_ITER_MULT as f64 * ((problem.step as f64 + 1.).log10() + 1.)).ceil() as usize;
    for _ in 0..iteration {
        if random_action(rng, true).try_apply(&mut position).is_err() {
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
        inferior_count = 0;
        let step = step.unwrap();

        if step > problem.step {
            return Ok(Problem::new(position, step, &movements));
        }

        solvable_position = position.clone();
    }
    movements.clear();
    one_way_mate_steps(&solvable_position, &mut movements).unwrap();
    Err(Problem::new(solvable_position, problem.step, &movements))
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
            60..=61 => return Action::HandToHand(rng.gen(), rng.gen()),
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

            let mut movements = vec![];
            loop {
                let action = random_action(&mut rng, false);
                if action.try_apply(&mut position).is_err() {
                    continue;
                }
                movements.clear();
                if let Some(step) = one_way_mate_steps(&position, &mut movements) {
                    return Problem::new(position, step, &movements);
                }
            }
        })
        .collect::<Vec<_>>()
}

#[derive(Clone, Debug)]
struct Problem {
    position: Position,
    step: usize,
    white_movements: u64,
}

impl Problem {
    fn new(position: Position, step: usize, movements: &[Movement]) -> Self {
        assert_eq!(step, movements.len());

        let mut hasher = FxHasher::default();
        movements
            .into_iter()
            .enumerate()
            .filter(|(i, _)| i % 2 == step % 2)
            .for_each(|(_, m)| m.hash(&mut hasher));
        Self {
            position,
            step,
            white_movements: hasher.finish(),
        }
    }
}
