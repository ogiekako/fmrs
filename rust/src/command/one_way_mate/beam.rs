use std::collections::VecDeque;

use fmrs_core::{piece::Color, position::Position, sfen};
use log::info;
use rand::{rngs::SmallRng, Rng, SeedableRng};
use rayon::prelude::*;

use super::{action::Action, solve::one_way_mate_steps};

pub(super) fn generate_one_way_mate_with_beam(
    seed: u64,
    start: usize,
    bucket: usize,
) -> anyhow::Result<()> {
    info!(
        "generate_one_way_mate_with_beam: seed={} start={} bucket={}",
        seed, start, bucket
    );

    let parallel = 16;

    assert!(std::thread::available_parallelism().unwrap().get() >= parallel as usize);

    let problems: Vec<Problem> = (seed..seed + parallel)
        .into_par_iter()
        .map(|seed| {
            let mut g = Generator::new(seed, start, bucket);
            let problem = g.generate();
            problem
        })
        .collect();

    for problem in problems {
        println!(
            "generated problem (step = {}): {}",
            problem.step,
            &sfen::encode_position(&problem.position)
        );
    }

    Ok(())
}

struct Generator {
    problems: VecDeque<Problem>,
    bucket: usize,

    head: usize,

    rng: SmallRng,

    best_problem: Problem,
}

const SEARCH_DEPTH: usize = 6;
const SEARCH_ITER_MULT: usize = 20000;
const USE_MULT: usize = 5;
const MAX_PRODUCE: usize = 2;

impl Generator {
    fn new(seed: u64, start: usize, bucket: usize) -> Self {
        let mut rng = SmallRng::seed_from_u64(seed);
        let problems: VecDeque<_> = random_one_way_mate_positions(&mut rng, start)
            .into_iter()
            .map(|(position, step)| Problem::new(position, step))
            .collect();
        let mut best_problem = problems[0].clone();
        problems.iter().for_each(|problem| {
            if problem.step > best_problem.step {
                best_problem = problem.clone();
            }
        });

        Self {
            problems,
            bucket,

            head: 0,

            rng,

            best_problem,
        }
    }

    fn generate(&mut self) -> Problem {
        while !self.problems.is_empty() {
            if self.head >= self.problems.len() {
                self.head = 0;
            }
            if self.head == 0 && self.problems.len() >= self.bucket {
                self.problems.pop_front().unwrap();
                continue;
            }
            let problem = {
                let problem = &mut self.problems[self.head];
                if problem.used
                    >= (problem.step.next_power_of_two().trailing_zeros() + 1) as usize * USE_MULT
                    || problem.produced >= MAX_PRODUCE
                {
                    self.problems.remove(self.head);
                    continue;
                }
                problem.used += 1;
                self.head += 1;
                problem
            };

            let mut position = problem.position.clone();
            let mut undo_to_solvable = vec![];

            for _ in 0..(problem.step.next_power_of_two().trailing_zeros() + 1) as usize
                * SEARCH_ITER_MULT
            {
                let action = random_action(&mut self.rng, true);
                let Ok(undo_action) = action.try_apply(&mut position) else {
                    continue;
                };
                let step = one_way_mate_steps(&position).unwrap_or(0);

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
                    problem.produced += 1;

                    let new_problem = Problem::new(position.clone(), step);
                    if self.best_problem.step < step {
                        self.best_problem = new_problem.clone();

                        info!(
                            "best={} {} len={}",
                            self.best_problem.step,
                            sfen::sfen_to_image_url(&sfen::encode_position(
                                &self.best_problem.position
                            )),
                            self.problems.len()
                        );
                    }
                    self.problems.push_back(new_problem);
                    break;
                }
            }
        }
        self.best_problem.clone()
    }
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
                if let Some(step) = one_way_mate_steps(&position) {
                    return (position, step);
                }
            }
        })
        .collect()
}

#[derive(Clone, Debug)]
struct Problem {
    position: Position,
    step: usize,

    used: usize,
    produced: usize,
}

impl Problem {
    fn new(position: Position, step: usize) -> Self {
        Self {
            position,
            step,
            used: 0,
            produced: 0,
        }
    }
}
