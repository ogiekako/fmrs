use std::{
    cmp::Reverse,
    collections::{BTreeMap, HashMap},
    env,
    hash::{Hash as _, Hasher as _},
    time::Instant,
};

use fmrs_core::{
    piece::{Color, Kind},
    position::{advance::advance::advance_aux, position::PositionAux, AdvanceOptions, Movement},
};
use log::{debug, info};
use rand::{rngs::SmallRng, seq::SliceRandom, Rng, SeedableRng};
use rayon::prelude::*;
use rustc_hash::FxHasher;

use crate::command::parse_to_sfen;

use super::action::Action;
use fmrs_core::nohash::NoHashSet64;
use fmrs_core::solve::one_way::one_way_mate_steps;

#[derive(Debug, Clone, Copy)]
struct BeamConfig {
    weight_exponent: f64,
    seen_cap: Option<usize>,
    use_mult: usize,
    require_legal: bool,
    // Keep only black-to-move (therefore odd-length) problems.  Long white-to-
    // move ties are useless for the one-way composition target but otherwise
    // consume roughly half of the beam near a plateau.
    require_black: bool,
    // Preserve one of the first-k forced movement sequences from the initial
    // seeds.  This keeps a seeded gadget from being replaced by an unrelated
    // (but longer) forced line while its tail is co-evolved.
    required_prefix: Option<usize>,
    // Print tying (same-step) problems whose initial position is legal, once
    // per distinct position, when step >= this threshold. Used to walk a
    // fixed-step ridge (e.g. an illegal record) hunting for legal variants.
    legal_ties_min: usize,
    // How many consecutive inferior mutations are tolerated before reverting
    // to the last solvable position (valley-crossing depth).
    search_depth: usize,
    // Per-kind caps on the number of black checking moves in an accepted
    // solution, indexed by unpromoted Kind (promoted pieces count toward
    // their base kind). Used to steer the search away from known mechanism
    // attractors (e.g. rook-lockstep marches, edge pawn push-ups).
    kind_caps: Option<[usize; 16]>,
}

impl BeamConfig {
    fn from_env() -> anyhow::Result<Self> {
        let weight_exponent = parse_f64_env("FMRS_BEAM_WEIGHT_EXPONENT")?.unwrap_or(0.50);
        let seen_cap = parse_usize_env("FMRS_BEAM_SEEN_CAP")?;
        let use_mult = parse_usize_env("FMRS_BEAM_USE_MULT")?
            .unwrap_or(USE_MULT)
            .max(1);
        let require_legal = parse_bool_env("FMRS_BEAM_REQUIRE_LEGAL")?.unwrap_or(false);
        let require_black = parse_bool_env("FMRS_BEAM_REQUIRE_BLACK")?.unwrap_or(false);
        let required_prefix = parse_usize_env("FMRS_BEAM_REQUIRED_PREFIX")?;
        initialize_required_prefixes(required_prefix)?;
        let legal_ties_min = parse_usize_env("FMRS_BEAM_LEGAL_TIES_MIN")?.unwrap_or(usize::MAX);
        let search_depth = parse_usize_env("FMRS_BEAM_SEARCH_DEPTH")?
            .unwrap_or(SEARCH_DEPTH)
            .max(1);
        // e.g. FMRS_BEAM_KIND_CAPS="R:6,P:8" caps rook checks at 6 and pawn
        // checks at 8 in any accepted solution.
        let kind_caps = parse_kind_caps_env("FMRS_BEAM_KIND_CAPS")?;
        let soft_kind_caps = parse_kind_caps_env("FMRS_BEAM_SOFT_KIND_CAPS")?;
        let soft_kind_penalty = parse_usize_env("FMRS_BEAM_SOFT_KIND_PENALTY")?
            .unwrap_or(2)
            .max(1);
        SOFT_KIND_CONFIG
            .set(SoftKindConfig {
                caps: soft_kind_caps,
                penalty: soft_kind_penalty,
            })
            .map_err(|_| anyhow::anyhow!("soft kind configuration was already initialized"))?;
        let score_break_branches =
            parse_bool_env("FMRS_BEAM_SCORE_BREAK_BRANCHES")?.unwrap_or(false);
        SCORE_BREAK_BRANCHES
            .set(score_break_branches)
            .map_err(|_| anyhow::anyhow!("break-branch scoring was already initialized"))?;
        Ok(Self {
            weight_exponent,
            seen_cap,
            use_mult,
            require_legal,
            require_black,
            required_prefix,
            legal_ties_min,
            search_depth,
            kind_caps,
        })
    }
}

#[derive(Clone, Copy, Debug)]
struct SoftKindConfig {
    caps: Option<[usize; 16]>,
    penalty: usize,
}

static SOFT_KIND_CONFIG: std::sync::OnceLock<SoftKindConfig> = std::sync::OnceLock::new();
static SCORE_BREAK_BRANCHES: std::sync::OnceLock<bool> = std::sync::OnceLock::new();

fn soft_kind_config() -> SoftKindConfig {
    SOFT_KIND_CONFIG.get().copied().unwrap_or(SoftKindConfig {
        caps: None,
        penalty: 1,
    })
}

#[derive(Debug)]
struct RequiredPrefix {
    digest: u64,
    movements: Vec<Movement>,
}

static REQUIRED_PREFIXES: std::sync::OnceLock<Vec<RequiredPrefix>> = std::sync::OnceLock::new();

fn movements_digest(movements: &[Movement]) -> u64 {
    let mut hasher = FxHasher::default();
    for movement in movements {
        movement.hash(&mut hasher);
    }
    hasher.finish()
}

fn initialize_required_prefixes(required_prefix: Option<usize>) -> anyhow::Result<()> {
    let Some(len) = required_prefix else {
        return Ok(());
    };
    let sfen_like = env::var("FMRS_BEAM_INITIAL_SFEN").map_err(|_| {
        anyhow::anyhow!("FMRS_BEAM_REQUIRED_PREFIX requires FMRS_BEAM_INITIAL_SFEN")
    })?;
    let mut prefixes = Vec::new();
    for line in sfen_like
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
    {
        let sfen = parse_to_sfen(line)?;
        let mut position = PositionAux::from_sfen(&sfen)?;
        let mut movements = Vec::new();
        let _ = one_way_mate_steps(&mut position, &mut movements);
        if movements.len() < len {
            anyhow::bail!(
                "FMRS_BEAM_INITIAL_SFEN has only {} forced moves, shorter than FMRS_BEAM_REQUIRED_PREFIX={}: {}",
                movements.len(),
                len,
                line,
            );
        }
        let movements = movements[..len].to_vec();
        prefixes.push(RequiredPrefix {
            digest: movements_digest(&movements),
            movements,
        });
    }
    if prefixes.is_empty() {
        anyhow::bail!("FMRS_BEAM_REQUIRED_PREFIX requires at least one initial seed");
    }
    REQUIRED_PREFIXES
        .set(prefixes)
        .map_err(|_| anyhow::anyhow!("required prefixes were already initialized"))?;
    Ok(())
}

fn required_prefix_ok(required_prefix: Option<usize>, movements: &[Movement]) -> bool {
    let Some(len) = required_prefix else {
        return true;
    };
    if movements.len() < len {
        return false;
    }
    let candidate = &movements[..len];
    let digest = movements_digest(candidate);
    REQUIRED_PREFIXES
        .get()
        .expect("required prefixes initialized with BeamConfig")
        .iter()
        .any(|prefix| prefix.digest == digest && prefix.movements.as_slice() == candidate)
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

fn parse_bool_env(name: &str) -> anyhow::Result<Option<bool>> {
    env::var(name)
        .ok()
        .map(|s| match s.as_str() {
            "1" | "true" | "TRUE" | "yes" | "YES" => Ok(true),
            "0" | "false" | "FALSE" | "no" | "NO" => Ok(false),
            _ => Err(anyhow::anyhow!("invalid {}: {}", name, s)),
        })
        .transpose()
}

fn parse_kind_caps_env(name: &str) -> anyhow::Result<Option<[usize; 16]>> {
    let Some(value) = env::var(name).ok() else {
        return Ok(None);
    };
    let mut caps = [usize::MAX; 16];
    for part in value.split(',') {
        let (kind, cap) = part
            .split_once(':')
            .ok_or_else(|| anyhow::anyhow!("invalid {}: {}", name, value))?;
        let kind = match kind.trim() {
            "P" => Kind::Pawn,
            "L" => Kind::Lance,
            "N" => Kind::Knight,
            "S" => Kind::Silver,
            "G" => Kind::Gold,
            "B" => Kind::Bishop,
            "R" => Kind::Rook,
            "K" => Kind::King,
            _ => anyhow::bail!("invalid kind in {}: {}", name, kind),
        };
        caps[kind as usize] = cap.trim().parse()?;
    }
    Ok(Some(caps))
}

pub(super) fn generate_one_way_mate_with_beam(
    mut seed: u64,
    parallel: usize,
    goal: Option<usize>,
) -> anyhow::Result<()> {
    let config = BeamConfig::from_env()?;
    info!(
        "generate_one_way_mate_with_beam: seed={} parallel={} config={:?} soft_kind_config={:?}",
        seed,
        parallel,
        config,
        soft_kind_config(),
    );

    let start_time = Instant::now();

    let mut seen_stats: HashMap<u64, usize> = Default::default();

    let initial_problems = initial_problems_from_env()?;
    for problem in initial_problems.iter() {
        println!(
            "{} {:?} ({:.1?})",
            problem.step,
            problem.position,
            start_time.elapsed()
        );
    }
    if initial_problems
        .iter()
        .any(|problem| problem_reaches_goal(problem, goal, config))
    {
        return Ok(());
    }
    let mut best_problems: Vec<Problem> = initial_problems.clone();
    if let Some(best_score) = best_problems.iter().map(Problem::score).max() {
        best_problems.retain(|problem| problem.score() == best_score);
    }
    let mut printed_legal_ties: NoHashSet64 = Default::default();
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
            &best_problems,
            &initial_problems,
            config,
            &start_time,
            &mut seen_stats,
        );

        for problem in problems {
            // A steering bonus can deliberately keep a useful incomplete
            // ridge above a newly completed problem in score order.  Goal
            // detection must therefore inspect every produced problem, not
            // only strict score improvements.
            if problem_reaches_goal(&problem, goal, config) {
                println!(
                    "{} {:?} ({:.1?})",
                    problem.step,
                    problem.position,
                    start_time.elapsed()
                );
                return Ok(());
            }
            if best_problems.is_empty() || best_problems[0].score() < problem.score() {
                best_problems.clear();

                println!(
                    "{} {:?} ({:.1?})",
                    problem.step,
                    problem.position,
                    start_time.elapsed()
                );

                best_problems.push(problem);
            } else if best_problems[0].score() == problem.score() {
                if problem.step >= config.legal_ties_min {
                    let problem = problem.clone();
                    if !problem.position.is_illegal_initial_position()
                        && printed_legal_ties.insert(problem.position.digest())
                    {
                        println!(
                            "{} LEGAL-TIE {:?} ({:.1?})",
                            problem.step,
                            problem.position,
                            start_time.elapsed()
                        );
                    }
                }
                best_problems.push(problem);
            }
        }

        // `best_problems` accumulates one tie-push per improving `problem` on every
        // iteration where the best doesn't strictly advance. Left unbounded, each entry
        // gets fully re-decomposed into `all_problems` (down to step 0) on every future
        // `generate()` call, so both memory and per-iteration time grow without bound
        // over a long stagnant run (observed: iteration time 30s -> 78s -> 1000s+,
        // RSS climbing into tens of GB and eventually OOM-killed). Cap it.
        if best_problems.len() > MAX_BEST_PROBLEMS {
            let mut rng = SmallRng::seed_from_u64(seed);
            seed += 1;
            best_problems.shuffle(&mut rng);
            best_problems.truncate(MAX_BEST_PROBLEMS);
        }
    }
    unreachable!()
}

fn problem_reaches_goal(problem: &Problem, goal: Option<usize>, config: BeamConfig) -> bool {
    if !problem.score().reaches_goal(goal) {
        return false;
    }
    let mut position = problem.position.clone();
    if config.require_legal && position.is_illegal_initial_position() {
        return false;
    }
    if config.require_black && !problem.position.turn().is_black() {
        return false;
    }

    // Re-evaluate the complete line before terminating. This also applies the
    // hard kind cap to an initial seed (mutated candidates have already passed
    // it in `compute_better_problem`).
    let mut movements = Vec::new();
    let (score, _) = one_way_score(&mut position, &mut movements);
    score.reaches_goal(goal)
        && required_prefix_ok(config.required_prefix, &movements)
        && kind_cap_ok(&config.kind_caps, &position, &movements)
}

fn initial_problems_from_env() -> anyhow::Result<Vec<Problem>> {
    let Some(sfen_like) = env::var("FMRS_BEAM_INITIAL_SFEN").ok() else {
        return Ok(vec![]);
    };
    sfen_like
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(|line| {
            let sfen = parse_to_sfen(line)?;
            let mut position = PositionAux::from_sfen(&sfen)?;
            let mut movements = Vec::new();
            // Invalid seeds are accepted as mutation starting points: score them
            // by the step they fail at (their forced prefix length).
            let (score, failed_step) = one_way_score(&mut position, &mut movements);
            if let Some(failed_step) = failed_step {
                    log::info!(
                        "FMRS_BEAM_INITIAL_SFEN entry is not one-way (fails at step {}), using as mutation seed: {}",
                        failed_step,
                        line
                    );
            }
            Ok(Problem::new(position, score, &movements))
        })
        .collect()
}

const SEARCH_DEPTH: usize = 7;
const SEARCH_ITER_MULT: usize = 10000;
const USE_MULT: usize = 1;
const MAX_PRODUCE: [usize; 2] = [1, 1];
const MAX_BEST_PROBLEMS: usize = 200;

fn insert(all_problems: &mut Vec<Vec<Problem>>, mut problem: Problem, min_step: usize) {
    let mut movements = vec![];
    // On Err (invalid one-way, e.g. a mutation seed), use the forced-prefix
    // length so that step == movements.len() (Problem::new invariant).
    let (score, _) = one_way_score(&mut problem.position, &mut movements);
    let step = score.step;
    problem = Problem::new(problem.position, score, &movements);

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
        let remaining_movements = &movements[i + 1..];
        let remaining_score = score_line(
            &position,
            remaining_movements,
            step - 1 - i,
            score.complete,
            None,
        );
        all_problems[step - 1 - i].push(Problem::new(
            position.clone(),
            remaining_score,
            remaining_movements,
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
    prev_bests: &[Problem],
    anchors: &[Problem],
    config: BeamConfig,
    start_time: &Instant,
    seen_stats: &mut HashMap<u64, usize>,
) -> Vec<Problem> {
    let mut all_problems: Vec<Vec<Problem>> = vec![];

    for anchor in anchors {
        insert(&mut all_problems, anchor.clone(), 0);
    }
    for best in prev_bests {
        insert(&mut all_problems, best.clone(), 0);
    }

    // With explicit anchors this is a mutation search around those mechanisms.
    // Injecting unrelated random works can dominate a seeded soft-cap run by
    // raw length and silently replace the requested family.
    let initial_cands = if anchors.is_empty() {
        random_one_way_mate_positions(seed, parallel, config.require_legal, config.require_black)
    } else {
        Vec::new()
    };

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

        let prev_best_step = prev_bests.first().map(|p| p.step).unwrap_or(0);
        if step >= prev_best_step + usize::from(!prev_bests.is_empty()) {
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

                    if let Some(new_problem) = compute_better_problem(
                        &mut rng,
                        problem,
                        config.search_depth,
                        must_step_parity,
                        config.require_legal,
                        config.require_black,
                        config.required_prefix,
                        &config.kind_caps,
                    ) {
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

/// Counts black checking moves by unpromoted kind and rejects solutions
/// exceeding the configured caps (mechanism-diversity steering).
fn checking_kind_counts(initial: &PositionAux, movements: &[Movement]) -> [usize; 16] {
    let mut counts = [0usize; 16];
    let mut pos = initial.clone();
    for m in movements {
        if pos.turn().is_black() {
            let base = match m {
                Movement::Drop(_, kind) => kind.maybe_unpromote(),
                Movement::Move { source, .. } => match pos.get(*source) {
                    Some((_, kind)) => kind.maybe_unpromote(),
                    None => Kind::King,
                },
            };
            counts[base as usize] += 1;
        }
        pos.do_move(m);
    }
    counts
}

fn kind_cap_ok(caps: &Option<[usize; 16]>, initial: &PositionAux, movements: &[Movement]) -> bool {
    let Some(caps) = caps else {
        return true;
    };
    checking_kind_counts(initial, movements)
        .into_iter()
        .zip(caps)
        .all(|(count, cap)| count <= *cap)
}

fn soft_kind_excess(initial: &PositionAux, movements: &[Movement]) -> usize {
    let Some(caps) = soft_kind_config().caps else {
        return 0;
    };
    checking_kind_counts(initial, movements)
        .into_iter()
        .zip(caps)
        .map(|(count, cap)| count.saturating_sub(cap))
        .sum()
}

fn compute_better_problem(
    rng: &mut SmallRng,
    problem: &mut Problem,
    search_depth: usize,
    must_step_parity: Option<usize>,
    require_legal: bool,
    require_black: bool,
    required_prefix: Option<usize>,
    kind_caps: &Option<[usize; 16]>,
) -> Option<Problem> {
    let mut position = problem.position.clone();
    let mut solvable_position = problem.position.clone();
    let mut solvable_position_movements = None;

    let mut inferior_count = 0;

    let mut movements = vec![];

    // OnceLock: reading env vars per call would serialize parallel workers on
    // the env lock (see the FMRS_FEAT_HEAVY incident).
    static ITER_MULT: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    let iter_mult = *ITER_MULT.get_or_init(|| {
        std::env::var("FMRS_BEAM_SEARCH_ITER_MULT")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(SEARCH_ITER_MULT)
    });
    let iteration = (iter_mult as f64 * ((problem.step as f64 + 1.).log2() + 1.)).ceil() as usize;

    for _ in 0..iteration {
        if random_action(rng, true).try_apply(&mut position).is_err() {
            continue;
        }
        if require_legal && position.is_illegal_initial_position() {
            continue;
        }
        if require_black && !position.turn().is_black() {
            continue;
        }
        if !has_exactly_one_king_each(&position) {
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
        // Keep incomplete positions on the ridge and score them by the
        // length of their uniquely forced prefix.  Previously every `Err`
        // mutation was discarded here, so an incomplete anchor could only
        // improve if a *single* random edit closed its entire tail.  That
        // defeated the intended dynamic co-evolution of incomplete seeds.
        let (score, _) = one_way_score(&mut position, &mut movements);
        let step = score.step;

        if score < problem.score()
            || !required_prefix_ok(required_prefix, &movements)
            || !kind_cap_ok(kind_caps, &position, &movements)
        {
            inferior_count += 1;
            if inferior_count >= search_depth {
                position = solvable_position.clone();
            }
            continue;
        }
        inferior_count = 0;
        solvable_position = position.clone();
        solvable_position_movements = Some((movements.clone(), score.complete));

        if score == problem.score() {
            continue;
        }

        if step == problem.step
            || must_step_parity.is_none()
            || step >= problem.step + 2
            || Some(step % 2) == must_step_parity
        {
            return Some(Problem::new(position, score, &movements));
        }
    }
    if let Some((mut movements, complete)) = solvable_position_movements {
        while movements.len() > problem.step {
            solvable_position.do_move(&movements.remove(0));
        }
        let score = score_line(
            &solvable_position,
            &movements,
            movements.len(),
            complete,
            None,
        );
        *problem = Problem::new(solvable_position, score, &movements);
    }
    None
}

// Optional mutation-region restriction (FMRS_BEAM_REGION="minrow,maxrow" with
// 0-indexed rows; row 0 = rank a). When set, all randomly generated squares
// are drawn from the region, concentrating the mutation budget (e.g. on the
// head gadget of a mate chain) instead of the whole board.
fn region_rows() -> Option<(usize, usize)> {
    static REGION: std::sync::OnceLock<Option<(usize, usize)>> = std::sync::OnceLock::new();
    *REGION.get_or_init(|| {
        let s = env::var("FMRS_BEAM_REGION").ok()?;
        let (a, b) = s.split_once(',')?;
        Some((a.trim().parse().ok()?, b.trim().parse().ok()?))
    })
}

fn random_square(rng: &mut SmallRng) -> fmrs_core::position::Square {
    if let Some((min_row, max_row)) = region_rows() {
        let row = rng.gen_range(min_row..=max_row);
        let col = rng.gen_range(0..9);
        return fmrs_core::position::Square::new(col, row);
    }
    rng.gen()
}

fn random_action(rng: &mut SmallRng, allow_black_capture: bool) -> Action {
    loop {
        match rng.gen_range(0..100) {
            0..=9 => return Action::Move(random_square(rng), random_square(rng)),
            10..=19 => return Action::Swap(random_square(rng), random_square(rng)),
            20..=27 => {
                return Action::FromHand(rng.gen(), random_square(rng), rng.gen(), rng.gen())
            }
            30..=39 => {
                return Action::ToHand(
                    random_square(rng),
                    if allow_black_capture {
                        rng.gen()
                    } else {
                        Color::WHITE
                    },
                )
            }
            40..=47 => {
                if region_rows().is_some() {
                    continue; // shifts move the whole board out of the frozen-tail frame
                }
                return Action::Shift(rng.gen());
            }
            50..=54 => return Action::ChangeTurn,
            60..=60 => return Action::HandToHand(rng.gen(), rng.gen()),
            _ => (),
        }
    }
}

fn random_one_way_mate_positions(
    seed: &mut u64,
    count: usize,
    require_legal: bool,
    require_black: bool,
) -> Vec<Problem> {
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
                if !has_exactly_one_king_each(&position) {
                    continue;
                }
                movements.clear();
                let (score, _) = one_way_score(&mut position, &mut movements);
                if score.complete
                    && (!require_legal || !position.is_illegal_initial_position())
                    && (!require_black || position.turn().is_black())
                {
                    return Problem::new(position, score, &movements);
                }
            }
        })
        .collect::<Vec<_>>()
}

fn has_exactly_one_king_each(position: &PositionAux) -> bool {
    position
        .bitboard(Color::WHITE, Kind::King)
        .u128()
        .count_ones()
        == 1
        && position
            .bitboard(Color::BLACK, Kind::King)
            .u128()
            .count_ones()
            == 1
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct ProblemScore {
    objective: usize,
    soft_kind_excess: Reverse<usize>,
    step: usize,
    complete: bool,
    break_branch_distance: Reverse<usize>,
}

impl ProblemScore {
    fn reaches_goal(self, goal: Option<usize>) -> bool {
        match goal {
            Some(goal) => self.complete && self.step >= goal && self.soft_kind_excess == Reverse(0),
            None => false,
        }
    }
}

fn score_line(
    position: &PositionAux,
    movements: &[Movement],
    step: usize,
    complete: bool,
    break_prefix_len: Option<usize>,
) -> ProblemScore {
    debug_assert_eq!(step, movements.len());
    let soft_kind_excess = soft_kind_excess(position, movements);
    let objective = step
        .saturating_sub(soft_kind_config().penalty.saturating_mul(soft_kind_excess))
        .saturating_add(board_progress_bonus(position, movements))
        .saturating_add(tail_pawn_drop_bonus(position, movements, break_prefix_len));
    ProblemScore {
        objective,
        soft_kind_excess: Reverse(soft_kind_excess),
        step,
        complete,
        break_branch_distance: Reverse(first_break_branch_distance(
            position,
            movements,
            break_prefix_len,
        )),
    }
}

/// Optional steering away from hand-count-only loops.  The one-way pawn pump
/// can have a long forced line while revisiting exactly the same four board
/// states; its only progress is that one pawn changes hands per lap.  A finite
/// pump needs an additional board counter, so reward distinct board states
/// along the forced prefix when explicitly requested.
fn board_progress_bonus(initial: &PositionAux, movements: &[Movement]) -> usize {
    static WEIGHT: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    static CAP: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    static START: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    let weight = *WEIGHT.get_or_init(|| {
        std::env::var("FMRS_BEAM_BOARD_PROGRESS_BONUS")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(0)
    });
    if weight == 0 {
        return 0;
    }
    let cap = *CAP.get_or_init(|| {
        std::env::var("FMRS_BEAM_BOARD_PROGRESS_CAP")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(usize::MAX)
    });
    let start = *START.get_or_init(|| {
        std::env::var("FMRS_BEAM_BOARD_PROGRESS_START")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(0)
    });

    let mut position = initial.clone();
    for movement in movements.iter().take(start) {
        position.do_move(movement);
    }
    if movements.len() < start {
        return 0;
    }
    let mut seen: NoHashSet64 = Default::default();
    seen.insert(position.board_digest());
    for movement in movements.iter().skip(start) {
        position.do_move(movement);
        seen.insert(position.board_digest());
    }
    weight.saturating_mul(seen.len().min(cap))
}

/// Optional ridge steering for pawn-income gadgets.  A position earns the
/// configured bonus only when, at the end of its fully forced prefix, Black
/// owns a pawn and the square immediately behind the white king is both empty
/// and free of nifu.  This is the exact local prerequisite for the next
/// checking pawn drop; it does not assert that the drop is legal with respect
/// to pawn-drop mate, which remains the authoritative solver's job.
fn tail_pawn_drop_bonus(
    initial: &PositionAux,
    movements: &[Movement],
    break_prefix_len: Option<usize>,
) -> usize {
    static BONUS: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    let bonus = *BONUS.get_or_init(|| {
        std::env::var("FMRS_BEAM_TAIL_PAWN_DROP_BONUS")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(0)
    });
    if bonus == 0 {
        return 0;
    }

    let mut position = initial.clone();
    for movement in movements
        .iter()
        .take(break_prefix_len.unwrap_or(movements.len()))
    {
        position.do_move(movement);
    }
    if !has_exactly_one_king_each(&position)
        || position.hands().count(Color::BLACK, Kind::Pawn) == 0
    {
        return 0;
    }

    let Some(king) = position
        .bitboard(Color::WHITE, Kind::King)
        .into_iter()
        .next()
    else {
        return 0;
    };
    let target_row = king.row() + 1;
    if target_row >= 9 {
        return 0;
    }
    let target = fmrs_core::position::Square::new(king.col(), target_row);
    if position.get(target).is_some()
        || position
            .bitboard(Color::BLACK, Kind::Pawn)
            .into_iter()
            .any(|pawn| pawn.col() == king.col())
    {
        return 0;
    }
    bonus
}

fn first_break_branch_distance(
    initial: &PositionAux,
    movements: &[Movement],
    break_prefix_len: Option<usize>,
) -> usize {
    let Some(break_prefix_len) = break_prefix_len else {
        return 0;
    };
    if !SCORE_BREAK_BRANCHES.get().copied().unwrap_or(false) {
        return 0;
    }
    branch_distance_at_prefix(initial, movements, break_prefix_len)
}

fn branch_distance_at_prefix(
    initial: &PositionAux,
    movements: &[Movement],
    break_prefix_len: usize,
) -> usize {
    let legality_probe = initial.clone();
    if legality_probe.is_illegal_initial_position() {
        return usize::MAX / 4;
    }
    let mut position = initial.clone();
    // A capped advance may leave its first speculative branch in `movements`.
    // Replaying only the fully forced prefix lands on the actual break.
    for movement in movements.iter().take(break_prefix_len) {
        position.do_move(movement);
    }
    // Some failed speculative lines can contain a king-capturing pseudo move.
    // `advance_aux` uses Square::INVALID (128) as the missing-king sentinel,
    // so never feed such a replay into its bitboard lookup tables.
    if !has_exactly_one_king_each(&position) {
        return usize::MAX / 4;
    }
    let options = AdvanceOptions {
        // Exact counts above five are immaterial for the local repair
        // objective; the useful gradient is 3 -> 2 -> 1 at the first break.
        max_allowed_branches: Some(4),
        // Every replayed black-to-move node follows a legal white response,
        // hence Black is known not to be in check.  Besides saving the
        // attacker scan, this keeps the diagnostic tie-break robust when a
        // capped speculative branch has left the auxiliary king cache stale.
        assume_not_in_check: position.turn().is_black(),
        ..Default::default()
    };
    let mut branches = Vec::new();
    let _ = advance_aux(&mut position, &options, &mut branches);
    branches.len().abs_diff(1)
}

fn one_way_score(
    position: &mut PositionAux,
    movements: &mut Vec<Movement>,
) -> (ProblemScore, Option<usize>) {
    let (step, complete, failed_step) = match one_way_mate_steps(position, movements) {
        Ok(step) => (step, true, None),
        Err(failed_step) => (movements.len(), false, Some(failed_step)),
    };
    let break_prefix_len =
        failed_step.map(|failed_step| failed_step.saturating_sub(1).min(movements.len()));
    (
        score_line(position, movements, step, complete, break_prefix_len),
        failed_step,
    )
}

#[derive(Clone, Debug)]
struct Problem {
    position: PositionAux,
    objective: usize,
    soft_kind_excess: Reverse<usize>,
    step: usize,
    complete: bool,
    break_branch_distance: Reverse<usize>,
    white_movements_digest: u64,
}

impl Problem {
    fn new(position: PositionAux, score: ProblemScore, movements: &[Movement]) -> Self {
        assert_eq!(score.step, movements.len());

        let mut hasher = FxHasher::default();
        movements
            .iter()
            .enumerate()
            .filter(|(i, _)| i % 2 == score.step % 2)
            .for_each(|(_, m)| m.hash(&mut hasher));
        Self {
            position,
            objective: score.objective,
            soft_kind_excess: score.soft_kind_excess,
            step: score.step,
            complete: score.complete,
            break_branch_distance: score.break_branch_distance,
            white_movements_digest: hasher.finish(),
        }
    }

    fn score(&self) -> ProblemScore {
        ProblemScore {
            objective: self.objective,
            soft_kind_excess: self.soft_kind_excess,
            step: self.step,
            complete: self.complete,
            break_branch_distance: self.break_branch_distance,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{cmp::Reverse, collections::HashMap};

    use fmrs_core::position::position::PositionAux;
    use rand::rngs::mock::StepRng;

    use super::{
        branch_distance_at_prefix, compute_key_weights, insert, one_way_score, Problem,
        ProblemScore,
    };

    #[test]
    fn branch_distance_is_measured_before_the_speculative_failed_move() {
        let mut position = PositionAux::from_sfen(
            "5gg+LR/1n3b1+PG/7L1/3G3Nk/pP1S2SP1/2NN1+P1L1/P4S1l1/K1pRB1Pp1/2s1+P+P2+p b 6p",
        )
        .unwrap();
        let initial = position.clone();
        let mut movements = Vec::new();
        let result = fmrs_core::solve::one_way::one_way_mate_steps(&mut position, &mut movements);

        assert_eq!(result, Err(36));
        assert_eq!(movements.len(), 36);
        assert_eq!(branch_distance_at_prefix(&initial, &movements, 35), 2);
    }

    #[test]
    fn complete_status_breaks_same_step_ties_and_gates_goal() {
        let incomplete = ProblemScore {
            objective: 45,
            soft_kind_excess: Reverse(0),
            step: 45,
            complete: false,
            break_branch_distance: Reverse(0),
        };
        let complete = ProblemScore {
            objective: 45,
            soft_kind_excess: Reverse(0),
            step: 45,
            complete: true,
            break_branch_distance: Reverse(0),
        };

        assert!(complete > incomplete);
        assert!(!incomplete.reaches_goal(Some(45)));
        assert!(complete.reaches_goal(Some(45)));
        assert!(!complete.reaches_goal(Some(47)));
        assert!(!complete.reaches_goal(None));

        let over_cap = ProblemScore {
            objective: 41,
            soft_kind_excess: Reverse(2),
            step: 45,
            complete: true,
            break_branch_distance: Reverse(0),
        };
        assert!(!over_cap.reaches_goal(Some(45)));

        // With penalty=2 these two lines have the same scalar objective;
        // the lower-excess replacement is deliberately preferred even though
        // it is two plies shorter, allowing station-by-station conversion.
        let fewer_known_family_checks = ProblemScore {
            objective: 41,
            soft_kind_excess: Reverse(1),
            step: 43,
            complete: true,
            break_branch_distance: Reverse(0),
        };
        assert!(fewer_known_family_checks > over_cap);

        let three_way_break = ProblemScore {
            objective: 36,
            soft_kind_excess: Reverse(0),
            step: 36,
            complete: false,
            break_branch_distance: Reverse(2),
        };
        let two_way_break = ProblemScore {
            break_branch_distance: Reverse(1),
            ..three_way_break
        };
        assert!(two_way_break > three_way_break);
    }

    #[test]
    fn one_way_score_and_insert_preserve_completion_status() {
        let mut complete_position = PositionAux::from_sfen(
            "9/1P1Kn4/2p6/2+lG+P2PG/k2S+p4/1pB4+p1/sB1RP4/2+SPR1p2/1L1+P3+P1 b 3NL2gsl6p",
        )
        .unwrap();
        let mut complete_movements = Vec::new();
        let (complete_score, failed_step) =
            one_way_score(&mut complete_position, &mut complete_movements);
        assert_eq!(
            complete_score,
            ProblemScore {
                objective: 21,
                soft_kind_excess: Reverse(0),
                step: 21,
                complete: true,
                break_branch_distance: Reverse(0),
            }
        );
        assert_eq!(failed_step, None);

        let complete_problem = Problem::new(complete_position, complete_score, &complete_movements);
        let mut buckets = Vec::new();
        insert(&mut buckets, complete_problem, 0);
        assert!(buckets[21].iter().all(|problem| problem.complete));

        let mut incomplete_position = PositionAux::from_sfen(
            "9/5+P3/1L+P3S2/1L3p3/1p3+pG1G/p1NNN1B+P1/L1sS1R3/PP1kBGK2/RLs2G3 b N9p",
        )
        .unwrap();
        let mut incomplete_movements = Vec::new();
        let (incomplete_score, failed_step) =
            one_way_score(&mut incomplete_position, &mut incomplete_movements);
        assert!(!incomplete_score.complete);
        assert_eq!(incomplete_score.step, incomplete_movements.len());
        assert_eq!(failed_step, Some(35));

        let incomplete_problem =
            Problem::new(incomplete_position, incomplete_score, &incomplete_movements);
        let mut buckets = Vec::new();
        insert(&mut buckets, incomplete_problem, 0);
        assert!(buckets[incomplete_score.step]
            .iter()
            .all(|problem| !problem.complete));
    }

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
