use fmrs_core::{
    piece::{Color, KINDS, NUM_HAND_KIND},
    position::Position,
    sfen,
};
use log::info;
use rand::{rngs::SmallRng, Rng, SeedableRng};
use rayon::prelude::*;
use std::collections::{BTreeSet, HashMap};

use super::{action::Action, solve::one_way_mate_steps};

pub(super) fn generate_one_way_mate_with_beam(
    seed: u64,
    start: usize,
    bucket: usize,
    parallel: usize,
) -> anyhow::Result<()> {
    info!(
        "generate_one_way_mate_with_beam: seed={} start={} bucket={}",
        seed, start, bucket
    );

    assert!(std::thread::available_parallelism().unwrap().get() >= parallel);

    let mut problems: Vec<_> = (seed..seed + parallel as u64)
        .into_par_iter()
        .map(|seed| {
            let mut g = Generator::new(seed, start, bucket);
            let problem = g.generate();
            problem
        })
        .collect::<Vec<_>>()
        .into_iter()
        .flatten()
        .collect();

    problems.sort_by_key(|problem| problem.step);

    for problem in problems {
        println!(
            "generated problem (step = {}): {}",
            problem.step,
            problem.position.sfen_url()
        );
    }

    Ok(())
}

struct Generator {
    problems: BTreeSet<Problem>,
    metadata: HashMap<u64, ProblemMetadata>,

    rng: SmallRng,

    best_problems: Vec<Problem>,
}

const SEARCH_DEPTH: usize = 6;
const SEARCH_ITER_LOG_BASE: f64 = 1.8;
const SEARCH_ITER_MULT: usize = 10000;
const USE_LOG_BASE: f64 = 1.8;
const USE_MULT: usize = 4;
const MAX_PRODUCE: usize = 2;

impl Generator {
    fn new(seed: u64, start: usize, _bucket: usize) -> Self {
        let mut rng = SmallRng::seed_from_u64(seed);
        let problems: BTreeSet<_> = random_one_way_mate_positions(&mut rng, start)
            .into_iter()
            .map(|(position, step)| Problem::new(position, step))
            .collect();

        let mut best_problems: Vec<Problem> = vec![];
        for problem in problems.iter() {
            if best_problems.is_empty() || best_problems[0].step < problem.step {
                best_problems.clear();
                best_problems.push(problem.clone());
            } else if best_problems[0].step == problem.step {
                best_problems.push(problem.clone());
            }
        }

        let metadata = problems
            .iter()
            .map(|problem| (problem.position.digest(), ProblemMetadata::default()))
            .collect();

        Self {
            problems,
            metadata,

            rng,

            best_problems,
        }
    }

    fn iteration(step: usize) -> usize {
        let log = (step as f64).log(SEARCH_ITER_LOG_BASE) as usize + 1;
        log * SEARCH_ITER_MULT
    }

    fn max_use(step: usize) -> usize {
        let log = (step as f64).log(USE_LOG_BASE) as usize + 1;
        log * USE_MULT
    }

    fn generate(&mut self) -> Vec<Problem> {
        while !self.problems.is_empty() {
            let problem = self.problems.first().unwrap();

            let metadata = self.metadata.get_mut(&problem.position.digest()).unwrap();
            if metadata.used >= Self::max_use(problem.step) || metadata.produced >= MAX_PRODUCE {
                self.problems.pop_first().unwrap();
                continue;
            }
            metadata.used += 1;

            let mut position = problem.position.clone();
            let mut undo_to_solvable = vec![];

            for _ in 0..Self::iteration(problem.step) {
                let action = random_action(&mut self.rng, true);
                let Ok(undo_action) = action.try_apply(&mut position) else {
                    continue;
                };
                let step = one_way_mate_steps(&position, &mut vec![]).unwrap_or(0);

                if step < problem.step {
                    undo_to_solvable.push(undo_action);

                    if undo_to_solvable.len() >= SEARCH_DEPTH {
                        for undo_action in undo_to_solvable.iter().rev() {
                            undo_action.clone().try_apply(&mut position).unwrap();
                        }
                        undo_to_solvable.clear();
                    }
                    continue;
                }

                undo_to_solvable.clear();

                if step > problem.step {
                    let new_problem = Problem::new(position.clone(), step);

                    let mut is_best = false;
                    if self.best_problems[0].step < step {
                        self.best_problems.clear();
                        is_best = true;
                    } else if self.best_problems[0].step == step {
                        is_best = true;
                    }

                    if is_best && !self.best_problems.contains(&new_problem) {
                        self.best_problems.push(new_problem.clone());

                        info!(
                            "best={} {} |best|={} |problems|={}",
                            self.best_problems[0].step,
                            sfen::sfen_to_image_url(&sfen::encode_position(&new_problem.position)),
                            self.best_problems.len(),
                            self.problems.len()
                        );
                    }

                    metadata.produced += 1;

                    self.metadata
                        .entry(new_problem.position.digest())
                        .or_default();

                    self.problems.insert(new_problem);

                    break;
                }
            }
        }
        self.best_problems.clone()
    }
}

fn random_action(rng: &mut SmallRng, allow_black_capture: bool) -> Action {
    loop {
        match rng.gen_range(0..100) {
            0..=9 => return Action::Move(rng.gen(), rng.gen()),
            10..=19 => return Action::Swap(rng.gen(), rng.gen()),
            20..=29 => return Action::FromHand(rng.gen(), rng.gen(), rng.gen(), rng.gen()),
            30..=34 => return Action::ToHand(rng.gen(), Color::WHITE),
            35..=39 if allow_black_capture => return Action::ToHand(rng.gen(), Color::BLACK),
            40..=44 => return Action::Shift(rng.gen()),
            50..=54 => return Action::ChangeTurn,
            60..=64 if allow_black_capture => {
                return Action::HandToHand(rng.gen(), KINDS[rng.gen_range(0..NUM_HAND_KIND)])
            }
            _ => (),
        }
    }
}

fn random_one_way_mate_positions(rng: &mut SmallRng, count: usize) -> Vec<(Position, usize)> {
    let initial_position =
        Position::from_sfen("4k4/9/9/9/9/9/9/9/4K4 b 2r2b4g4s4n4l18p 1").unwrap();

    (0..count)
        .map(|i| {
            if (i + 1) % 100 == 0 {
                info!("generate_one_way_mate_positions: {}", i + 1);
            }

            let mut position = initial_position.clone();

            loop {
                let action = random_action(rng, false);
                if action.try_apply(&mut position).is_err() {
                    continue;
                }
                if let Some(step) = one_way_mate_steps(&position, &mut vec![]) {
                    return (position, step);
                }
            }
        })
        .collect()
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Problem {
    step: usize,
    position: Position,
}

impl PartialOrd for Problem {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

// Sort by step in descending order.
impl Ord for Problem {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        other
            .step
            .cmp(&self.step)
            .then_with(|| self.position.cmp(&other.position))
    }
}

#[derive(Clone, Debug, Default)]
struct ProblemMetadata {
    used: usize,
    produced: usize,
}

impl Problem {
    fn new(position: Position, step: usize) -> Self {
        Self { position, step }
    }
}
