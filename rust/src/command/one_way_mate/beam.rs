use fmrs_core::{
    piece::{Color, KINDS, NUM_HAND_KIND},
    position::{Movement, Position},
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
    seen_movements: HashMap<Vec<Movement>, usize>,

    rng: SmallRng,

    best_problems: Vec<Problem>,
}

const SEARCH_DEPTH_LOG_BASE: f64 = 1.8;
const SEARCH_DEPTH_MULT: f64 = 7.;
const SEARCH_ITER_LOG_BASE: f64 = 1.8;
const SEARCH_ITER_MULT: f64 = 15000.;
const USE_LOG_BASE: f64 = 1.8;
const USE_MULT: f64 = 2.;
const MAX_PRODUCE: usize = 2;

impl Generator {
    fn new(seed: u64, start: usize, _bucket: usize) -> Self {
        let mut rng = SmallRng::seed_from_u64(seed);

        let mut problems = BTreeSet::new();
        let mut metadata = HashMap::new();

        for (problem, movements) in random_one_way_mate_positions(&mut rng, start) {
            metadata.insert(problem.position.digest(), ProblemMetadata::new(movements));
            problems.insert(problem);
        }

        let mut best_problems: Vec<Problem> = vec![];
        for problem in problems.iter() {
            if best_problems.is_empty() || best_problems[0].step < problem.step {
                best_problems.clear();
                best_problems.push(problem.clone());
            } else if best_problems[0].step == problem.step {
                best_problems.push(problem.clone());
            }
        }

        Self {
            problems,
            metadata,
            seen_movements: HashMap::new(),

            rng,

            best_problems,
        }
    }

    fn iteration(step: usize, seen: usize) -> usize {
        let log = (step as f64).log(SEARCH_ITER_LOG_BASE) + 1.;
        (log * SEARCH_ITER_MULT / seen as f64) as usize
    }

    fn max_use(step: usize, seen: usize) -> usize {
        let log = (step as f64).log(USE_LOG_BASE) + 1.;
        (log * USE_MULT / seen as f64) as usize
    }

    fn search_depth(step: usize, seen: usize) -> usize {
        let log = (step as f64).log(SEARCH_DEPTH_LOG_BASE) + 1.;
        (log * SEARCH_DEPTH_MULT / seen as f64) as usize
    }

    fn generate(&mut self) -> Vec<Problem> {
        let mut undo_to_solvable = vec![];
        while !self.problems.is_empty() {
            let problem = self.problems.first().unwrap();

            let metadata = self.metadata.get_mut(&problem.position.digest()).unwrap();

            let seen = self
                .seen_movements
                .entry(metadata.movements.clone())
                .or_default();
            *seen += 1;

            if metadata.used >= Self::max_use(problem.step, *seen)
                || metadata.produced >= MAX_PRODUCE
            {
                self.problems.pop_first().unwrap();
                continue;
            }
            metadata.used += 1;

            let mut position = problem.position.clone();
            undo_to_solvable.clear();

            for _ in 0..Self::iteration(problem.step, *seen) {
                let action = random_action(&mut self.rng, true);
                let Ok(undo_action) = action.try_apply(&mut position) else {
                    continue;
                };

                let mut movements = vec![];
                let step = one_way_mate_steps(&position, &mut movements).unwrap_or(0);

                if step == 0 {
                    undo_to_solvable.push(undo_action);

                    if undo_to_solvable.len() >= Self::search_depth(problem.step, *seen) {
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
                        .or_insert_with(|| ProblemMetadata::new(movements));

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
            40..=42 => return Action::Shift(rng.gen()),
            50..=52 if allow_black_capture => return Action::ChangeTurn,
            60..=62 if allow_black_capture => {
                return Action::HandToHand(rng.gen(), KINDS[rng.gen_range(0..NUM_HAND_KIND)])
            }
            _ => (),
        }
    }
}

fn random_one_way_mate_positions(
    rng: &mut SmallRng,
    count: usize,
) -> Vec<(Problem, Vec<Movement>)> {
    let mut position = Position::from_sfen("4k4/9/9/9/9/9/9/9/4K4 b 2r2b4g4s4n4l18p 1").unwrap();
    let mut solution = vec![];
    (0..count)
        .map(|i| {
            if (i + 1) % 100 == 0 {
                info!("generate_one_way_mate_positions: {}", i + 1);
            }

            for j in 0.. {
                let action = random_action(rng, false);
                if action.try_apply(&mut position).is_err() {
                    continue;
                }
                solution.clear();
                if let Some(step) = one_way_mate_steps(&position, &mut solution) {
                    if step > 0 && j > 100 {
                        return (Problem::new(position.clone(), step), solution.clone());
                    }
                }
            }
            unreachable!()
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

impl Problem {
    fn new(position: Position, step: usize) -> Self {
        Self { position, step }
    }
}

#[derive(Clone, Debug)]
struct ProblemMetadata {
    movements: Vec<Movement>,
    used: usize,
    produced: usize,
}

impl ProblemMetadata {
    fn new(movements: Vec<Movement>) -> Self {
        Self {
            movements,
            used: 0,
            produced: 0,
        }
    }
}
