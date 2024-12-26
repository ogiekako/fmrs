use std::{
    collections::{BTreeMap, HashMap},
    hash::{Hash as _, Hasher as _},
    io::Write as _,
    time::Instant,
    usize,
};

use fmrs_core::{
    piece::{Color, KIND_KING},
    position::{position::PositionAux, Movement, Position},
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

    let mut seen_stats: HashMap<u64, usize> = Default::default();

    let mut best_problems: Vec<Problem> = vec![];
    for i in 0.. {
        eprint!(".");
        std::io::stderr().flush().unwrap();

        if (i + 1) % 10 == 0 {
            info!(
                "best = {} #seen = {} iter = {} ({:.1?} iter/{:.1?})",
                best_problems[0].step,
                seen_stats.len(),
                i + 1,
                start_time.elapsed(),
                start_time.elapsed() / (i + 1),
            );
        }

        let problems = generate(
            &mut seed,
            parallel,
            best_problems.get(0).cloned(),
            &start_time,
            &mut seen_stats,
        );

        for problem in problems {
            if best_problems.is_empty() || best_problems[0].step < problem.step {
                best_problems.clear();

                println!(
                    "{} {:?} ({:.1?})",
                    problem.step,
                    problem.position,
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

const SEARCH_DEPTH: usize = 7;
const SEARCH_ITER_MULT: usize = 10000;
const USE_MULT: usize = 1;
const MAX_PRODUCE: [usize; 2] = [1, 1];

fn insert(all_problems: &mut Vec<Vec<Problem>>, mut problem: Problem, min_step: usize) {
    let mut movements = vec![];
    let step = one_way_mate_steps(&mut problem.position, &mut movements).unwrap();

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
    seen_stats: &mut HashMap<u64, usize>,
) -> Vec<Problem> {
    let mut all_problems: Vec<Vec<Problem>> = vec![];

    if let Some(best) = prev_best.as_ref() {
        insert(&mut all_problems, best.clone(), 0);
    }

    let initial_cands = random_one_way_mate_positions(seed, parallel);

    for cand in initial_cands.into_iter() {
        insert(&mut all_problems, cand, 0);
    }

    let mut rng = SmallRng::seed_from_u64(*seed);
    *seed += 1;

    for step in 0.. {
        if step >= all_problems.len() {
            break;
        }
        if all_problems[step].is_empty() {
            continue;
        }

        all_problems[step].shuffle(&mut rng);

        let mut buckets = BTreeMap::new();
        for problem in all_problems[step].iter() {
            buckets
                .entry(problem.white_movements_digest)
                .or_insert_with(Vec::new)
                .push(problem.clone());
        }

        let mut keys = buckets.keys().copied().collect::<Vec<_>>();
        keys.shuffle(&mut rng);

        let weights = keys
            .iter()
            .map(|k| 1. / (seen_stats.get(k).copied().unwrap_or(0) as f64 + 1.).sqrt())
            .collect::<Vec<_>>();
        let sum_weight = weights.iter().sum::<f64>();
        let mut key_weights = keys
            .into_iter()
            .zip(weights.into_iter())
            .map(|(k, weight)| {
                let p = weight / sum_weight;
                let u: f64 = rng.gen();
                let reservoir = u.powf(1.0 / p);
                assert!(reservoir.is_finite(), "{} {}", weight, sum_weight);
                (k, reservoir)
            })
            .collect::<Vec<_>>();
        key_weights.sort_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap());

        let mut problems = vec![];
        'outer: while !buckets.is_empty() {
            for (key, _) in key_weights.iter() {
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
                "step = {} #problems = {} veriety = {} best = {} elapsed={:.1?}",
                step,
                problems.len(),
                key_weights.len(),
                all_problems.len() - 1,
                start_time.elapsed()
            );
        }

        for problem in problems.iter() {
            *seen_stats
                .entry(problem.white_movements_digest)
                .or_default() += 1;
        }

        let mut i = 0;
        while problems.len() < parallel {
            let p = problems[i].clone();
            problems.push(p);
            i += 1;
        }

        let base_seed = *seed;
        *seed += parallel as u64;

        let new_problems = problems
            .par_iter_mut()
            .enumerate()
            .map(|(i, problem)| {
                assert_eq!(step, problem.step);

                let mut rng = SmallRng::seed_from_u64(base_seed + i as u64);

                let num_use = (USE_MULT as f64 * ((step as f64 + 1.).log10() + 1.)).ceil() as usize;

                let mut count = [0, 0];
                let mut new_problems = vec![];

                for _ in 0..num_use {
                    let must_step_parity = if count[0] >= MAX_PRODUCE[0] {
                        Some(1)
                    } else if count[1] >= MAX_PRODUCE[1] {
                        Some(0)
                    } else {
                        None
                    };

                    if let Some(new_problem) =
                        compute_better_problem(&mut rng, problem, SEARCH_DEPTH, must_step_parity)
                    {
                        for s in problem.step + 1..=new_problem.step {
                            count[s % 2] += 1;
                        }

                        new_problems.push(new_problem);

                        if count[0] >= MAX_PRODUCE[0] && count[1] >= MAX_PRODUCE[1] {
                            break;
                        }
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

fn compute_better_problem(
    rng: &mut SmallRng,
    problem: &mut Problem,
    search_depth: usize,
    must_step_parity: Option<usize>,
) -> Option<Problem> {
    let mut position = problem.position.clone();
    let mut solvable_position = problem.position.clone();
    let mut solvable_position_movements = None;

    let mut inferior_count = 0;

    let mut movements = vec![];

    let iteration =
        (SEARCH_ITER_MULT as f64 * ((problem.step as f64 + 1.).log2() + 1.)).ceil() as usize;

    for _ in 0..iteration {
        if random_action(rng, true).try_apply(&mut position).is_err() {
            continue;
        }
        debug_assert_eq!(
            position
                .bitboard::<KIND_KING>(Color::WHITE)
                .u128()
                .count_ones(),
            1
        );
        debug_assert_eq!(
            position
                .bitboard::<KIND_KING>(Color::BLACK)
                .u128()
                .count_ones(),
            1
        );

        movements.clear();
        let step = one_way_mate_steps(&mut position, &mut movements);

        if step.is_none() || step.unwrap() < problem.step {
            inferior_count += 1;
            if inferior_count >= search_depth {
                position = solvable_position.clone();
            }
            continue;
        }
        inferior_count = 0;
        solvable_position = position.clone();
        solvable_position_movements = Some(movements.clone());

        let step = step.unwrap();

        if step == problem.step {
            continue;
        }

        if must_step_parity.is_none()
            || step >= problem.step + 2
            || Some(step % 2) == must_step_parity
        {
            return Some(Problem::new(position, step, &movements));
        }
    }
    if let Some(mut movements) = solvable_position_movements {
        while movements.len() > problem.step {
            solvable_position.do_move(&movements.remove(0));
        }
        *problem = Problem::new(solvable_position, movements.len(), &movements);
    }
    None
}

fn random_action(rng: &mut SmallRng, allow_black_capture: bool) -> Action {
    loop {
        match rng.gen_range(0..100) {
            0..=9 => return Action::Move(rng.gen(), rng.gen()),
            10..=19 => return Action::Swap(rng.gen(), rng.gen()),
            20..=27 => return Action::FromHand(rng.gen(), rng.gen(), rng.gen(), rng.gen()),
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
            40..=47 => return Action::Shift(rng.gen()),
            50..=54 => return Action::ChangeTurn,
            60..=60 => return Action::HandToHand(rng.gen(), rng.gen()),
            _ => (),
        }
    }
}

fn random_one_way_mate_positions(seed: &mut u64, count: usize) -> Vec<Problem> {
    // TOOD: Use more random positions
    let initial_position =
        Position::from_sfen("4k4/9/9/9/9/9/9/9/4K4 b 2r2b4g4s4n4l18p 1").unwrap();

    let base_seed = *seed;
    *seed += count as u64;

    (0..count)
        .into_par_iter()
        .map(|i| {
            let mut rng = SmallRng::seed_from_u64(base_seed + i as u64);
            if (i + 1) % 1000 == 0 {
                debug!("generate_one_way_mate_positions: {}", i + 1);
            }
            let mut position = PositionAux::new(initial_position.clone());

            let mut movements = vec![];
            loop {
                let action = random_action(&mut rng, false);
                if action.try_apply(&mut position).is_err() {
                    continue;
                }
                movements.clear();
                if let Some(step) = one_way_mate_steps(&mut position, &mut movements) {
                    return Problem::new(position, step, &movements);
                }
            }
        })
        .collect::<Vec<_>>()
}

#[derive(Clone, Debug)]
struct Problem {
    position: PositionAux,
    step: usize,
    white_movements_digest: u64,
}

impl Problem {
    fn new(position: PositionAux, step: usize, movements: &[Movement]) -> Self {
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
            white_movements_digest: hasher.finish(),
        }
    }
}
