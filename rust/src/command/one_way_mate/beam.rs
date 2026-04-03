use std::{
    collections::{BTreeMap, HashMap},
    env,
    hash::{Hash as _, Hasher as _},
    time::Instant,
};

use fmrs_core::{
    piece::{Color, Kind},
    position::{position::PositionAux, Movement},
};
use log::{debug, info};
use rand::{rngs::SmallRng, seq::SliceRandom, Rng, SeedableRng};
use rayon::prelude::*;
use rustc_hash::FxHasher;

use fmrs_core::solve::one_way::one_way_mate_steps;
use super::action::Action;

#[derive(Debug, Clone, Copy)]
struct BeamConfig {
    weight_exponent: f64,
    seen_cap: Option<usize>,
    use_mult: usize,
}

impl BeamConfig {
    fn from_env() -> anyhow::Result<Self> {
        let weight_exponent = parse_f64_env("FMRS_BEAM_WEIGHT_EXPONENT")?.unwrap_or(0.50);
        let seen_cap = parse_usize_env("FMRS_BEAM_SEEN_CAP")?;
        let use_mult = parse_usize_env("FMRS_BEAM_USE_MULT")?.unwrap_or(USE_MULT).max(1);
        Ok(Self {
            weight_exponent,
            seen_cap,
            use_mult,
        })
    }
}

fn parse_usize_env(name: &str) -> anyhow::Result<Option<usize>> {
    env::var(name)
        .ok()
        .map(|s| {
            s.parse::<usize>()
                .map_err(|e| anyhow::anyhow!("invalid {}: {} ({})", name, s, e))
        })
        .transpose()
}

fn parse_f64_env(name: &str) -> anyhow::Result<Option<f64>> {
    env::var(name)
        .ok()
        .map(|s| {
            s.parse::<f64>()
                .map_err(|e| anyhow::anyhow!("invalid {}: {} ({})", name, s, e))
        })
        .transpose()
}

pub(super) fn generate_one_way_mate_with_beam(
    mut seed: u64,
    parallel: usize,
    goal: Option<usize>,
) -> anyhow::Result<()> {
    let config = BeamConfig::from_env()?;
    info!(
        "generate_one_way_mate_with_beam: seed={} parallel={} config={:?}",
        seed, parallel, config,
    );

    let start_time = Instant::now();

    let mut seen_stats: HashMap<u64, usize> = Default::default();

    let mut best_problems: Vec<Problem> = vec![];
    for i in 0.. {
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
            best_problems.first().cloned(),
            config,
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
    let step = one_way_mate_steps(&mut problem.position, &mut movements).unwrap_or_else(|e| e);

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

fn compute_key_weights<R: Rng + ?Sized>(
    keys: Vec<u64>,
    seen_stats: &HashMap<u64, usize>,
    weight_exponent: f64,
    seen_cap: Option<usize>,
    rng: &mut R,
) -> Vec<KeyWeight> {
    let mut key_weights = keys
        .into_iter()
        .map(|k| {
            let seen = seen_stats.get(&k).copied().unwrap_or(0);
            let seen = seen_cap.map(|cap| seen.min(cap)).unwrap_or(seen);
            let weight = 1. / (seen as f64 + 1.).powf(weight_exponent);
            let u: f64 = rng.gen();
            KeyWeight {
                key: k,
                weight,
                reservoir: u,
            }
        })
        .collect::<Vec<_>>();
    let sum_weight = key_weights.iter().map(|kw| kw.weight).sum::<f64>();
    for key_weight in key_weights.iter_mut() {
        let p = key_weight.weight / sum_weight;
        key_weight.reservoir = key_weight.reservoir.powf(1.0 / p);
        debug_assert!(
            key_weight.reservoir.is_finite(),
            "{} {}",
            key_weight.weight,
            sum_weight
        );
    }
    key_weights.sort_by(|a, b| b.reservoir.total_cmp(&a.reservoir));
    key_weights
}

#[derive(Debug, Clone, Copy)]
struct KeyWeight {
    key: u64,
    weight: f64,
    reservoir: f64,
}

fn generate(
    seed: &mut u64,
    parallel: usize,
    prev_best: Option<Problem>,
    config: BeamConfig,
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

        let key_weights = compute_key_weights(
            keys,
            seen_stats,
            config.weight_exponent,
            config.seen_cap,
            &mut rng,
        );

        let mut problems = vec![];
        'outer: while !buckets.is_empty() {
            for key_weight in key_weights.iter() {
                let Some(ps) = buckets.get_mut(&key_weight.key) else {
                    continue;
                };
                problems.push(ps.pop().unwrap());
                let empty = ps.is_empty();
                if empty {
                    buckets.remove(&key_weight.key);
                }
                if problems.len() >= parallel {
                    break 'outer;
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

        let mut i = 0;
        while problems.len() < parallel {
            let p = problems[i].clone();
            problems.push(p);
            i += 1;
        }

        let base_seed = *seed;
        *seed += parallel as u64;

        for problem in problems.iter() {
            *seen_stats
                .entry(problem.white_movements_digest)
                .or_default() += 1;
        }

        let new_problems = problems
            .par_iter_mut()
            .enumerate()
            .map(|(i, problem)| {
                debug_assert_eq!(step, problem.step);

                let mut rng = SmallRng::seed_from_u64(base_seed + i as u64);

                let num_use =
                    (config.use_mult as f64 * ((step as f64 + 1.).log10() + 1.)).ceil() as usize;

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
                .bitboard(Color::WHITE, Kind::King)
                .u128()
                .count_ones(),
            1
        );
        debug_assert_eq!(
            position
                .bitboard(Color::BLACK, Kind::King)
                .u128()
                .count_ones(),
            1
        );

        movements.clear();
        let step = one_way_mate_steps(&mut position, &mut movements);

        if step.is_err() || *step.as_ref().unwrap() < problem.step {
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
    let initial_position = PositionAux::from_sfen("4k4/9/9/9/9/9/9/9/4K4 b 2r2b4g4s4n4l18p 1")
        .unwrap()
        .clone();

    let base_seed = *seed;
    *seed += count as u64;

    (0..count)
        .into_par_iter()
        .map(|i| {
            let mut rng = SmallRng::seed_from_u64(base_seed + i as u64);
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
                if let Ok(step) = one_way_mate_steps(&mut position, &mut movements) {
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
            .iter()
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

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use rand::rngs::mock::StepRng;

    use super::compute_key_weights;

    #[test]
    fn compute_key_weights_prefers_less_seen_bucket_when_uniforms_match() {
        let keys = vec![1, 2];
        let seen_stats = HashMap::from([(2, 8)]);
        let mut rng = StepRng::new(1 << 63, 0);

        let key_weights = compute_key_weights(keys, &seen_stats, 0.5, None, &mut rng);

        assert_eq!(key_weights[0].key, 1);
        assert!(key_weights[0].reservoir > key_weights[1].reservoir);
    }

    #[test]
    fn compute_key_weights_seen_cap_saturates_penalty() {
        let keys = vec![1, 2];
        let seen_stats = HashMap::from([(1, 4), (2, 100)]);
        let mut rng = StepRng::new(1 << 63, 0);

        let key_weights = compute_key_weights(keys, &seen_stats, 0.5, Some(4), &mut rng);

        assert_eq!(key_weights[0].weight, key_weights[1].weight);
    }
}
