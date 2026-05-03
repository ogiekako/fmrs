use std::{ops::Range, sync::Arc};

use anyhow::bail;
use log::{debug, info};
use rayon::prelude::*;

use crate::{
    nohash::NoHashMap64,
    piece::Color,
    position::{
        advance::advance::advance_aux, position::PositionAux, previous, BitBoard, Movement,
        Position,
    },
    solve::standard_solve::standard_solve,
};

const MAX_BACKWARD_PARALLEL: usize = 32;

pub fn backward_initial_variants(initial_position: &PositionAux) -> Vec<PositionAux> {
    let mut variants = Vec::with_capacity(2);
    for pawn_drop in [false, true] {
        let mut position = initial_position.clone();
        position.set_pawn_drop(pawn_drop);
        if variants
            .iter()
            .all(|existing: &PositionAux| existing.digest() != position.digest())
        {
            variants.push(position);
        }
    }
    variants
}

pub fn backward_search(
    initial_position: &PositionAux,
    black_position: bool,
    forward: usize,
    one_way: bool,
) -> anyhow::Result<(u16, Vec<PositionAux>)> {
    backward_search_with_progress(
        initial_position,
        black_position,
        forward,
        one_way,
        |_s, _c, _u| {},
    )
}

pub fn backward_search_with_progress(
    initial_position: &PositionAux,
    black_position: bool,
    forward: usize,
    one_way: bool,
    progress: impl FnMut(u16, usize, String),
) -> anyhow::Result<(u16, Vec<PositionAux>)> {
    backward_search_with_progress_and_parallel(
        initial_position,
        black_position,
        forward,
        1,
        one_way,
        progress,
    )
}

pub fn backward_search_with_progress_and_parallel(
    initial_position: &PositionAux,
    black_position: bool,
    forward: usize,
    parallel: usize,
    one_way: bool,
    mut progress: impl FnMut(u16, usize, String),
) -> anyhow::Result<(u16, Vec<PositionAux>)> {
    let mut best = (0, NoHashMap64::default());
    let mut last_error = None;

    for variant in backward_initial_variants(initial_position) {
        match backward_search_single(
            &variant,
            black_position,
            forward,
            parallel,
            one_way,
            &mut progress,
        ) {
            Ok((step, positions)) => merge_backward_results(&mut best, step, positions),
            Err(err) if last_error.is_none() => last_error = Some(err),
            Err(_) => {}
        }
    }

    if best.1.is_empty() {
        return Err(last_error.unwrap_or_else(|| anyhow::anyhow!("No backward search result")));
    }

    let mut positions = best.1.into_values().collect::<Vec<_>>();
    positions.sort_by_key(|p| p.sfen());
    Ok((best.0, positions))
}

fn backward_search_single(
    initial_position: &PositionAux,
    black_position: bool,
    forward: usize,
    parallel: usize,
    one_way: bool,
    progress: &mut impl FnMut(u16, usize, String),
) -> anyhow::Result<(u16, Vec<PositionAux>)> {
    let mut search = BackwardSearch::new_with_parallel(initial_position, one_way, parallel)?;

    let initial_step = search.solution.len() as u16;
    let mut last_logged_step = search.step;

    let mut best = (0, NoHashMap64::default());

    for i in 0..=forward {
        if i > 0 {
            search.forward();
            info!("forward to {} ({}/{})", search.step, i, forward);
        }
        loop {
            if !search.advance()? {
                break;
            }
            if search.step != last_logged_step {
                last_logged_step = search.step;
                progress(
                    search.step,
                    search.positions.len(),
                    PositionAux::new(search.positions[0].clone(), *initial_position.stone())
                        .sfen_url(),
                );
            }
            if search.step > initial_step && search.step % 40 == 0 {
                info!(
                    "backward step={} count={} {}",
                    search.step,
                    search.positions.len(),
                    PositionAux::new(search.positions[0].clone(), *initial_position.stone())
                        .sfen_url()
                );
            } else if search.step > initial_step {
                debug!(
                    "backward step={} count={} {}",
                    search.step,
                    search.positions.len(),
                    PositionAux::new(search.positions[0].clone(), *initial_position.stone())
                        .sfen_url()
                );
            }
        }

        let step = if search.step > 0 && search.step % 2 == 0 && black_position {
            search.step - 1
        } else {
            search.step
        };
        if step < best.0 {
            continue;
        }
        if step > best.0 {
            best = (step, NoHashMap64::default());

            info!(
                "best={} count={} {}",
                best.0,
                search.positions.len(),
                PositionAux::new(search.positions[0].clone(), *initial_position.stone()).sfen_url()
            );
        }

        let mut positions = search
            .positions
            .iter()
            .filter(|p| !p.pawn_drop())
            .map(|p| PositionAux::new(p.clone(), *initial_position.stone()))
            .collect::<Vec<_>>();

        if !black_position || search.step % 2 == 1 || search.step == 0 {
            for p in positions.iter_mut() {
                best.1.insert(p.digest(), p.clone());
            }
            continue;
        }

        let mut black_positions = vec![];
        for p in positions.iter_mut() {
            debug_assert_eq!(p.turn(), Color::WHITE);
            let mut movements = vec![];
            advance_aux(p, &Default::default(), &mut movements)?;
            for m in movements.iter() {
                let digest = p.moved_digest(m);
                if search
                    .prev_memo
                    .get(&digest)
                    .map_or(false, |x| x.is_uniquely(search.step - 1))
                {
                    let mut np = p.clone();
                    np.do_move(m);
                    black_positions.push(np);
                }
            }
        }
        for p in black_positions.iter_mut() {
            best.1.insert(p.digest(), p.clone());
        }
    }
    let mut positions = best.1.into_values().collect::<Vec<_>>();
    positions.sort_by_key(|p| p.sfen());
    Ok((best.0, positions))
}

fn merge_backward_results(
    best: &mut (u16, NoHashMap64<PositionAux>),
    step: u16,
    positions: Vec<PositionAux>,
) {
    if step < best.0 {
        return;
    }
    if step > best.0 {
        best.0 = step;
        best.1.clear();
    }
    for position in positions {
        best.1.insert(position.digest(), position);
    }
}

pub struct BackwardSearch {
    initial_position: PositionAux,
    solution: Vec<Movement>,
    seen_positions: usize,
    positions: Vec<Position>,
    prev_positions: Vec<Position>,
    memo: NoHashMap64<StepRange>,
    prev_memo: NoHashMap64<StepRange>,
    stone: Option<BitBoard>,
    step: u16,
    one_way: bool,
    parallel: usize,
    pool: Option<rayon::ThreadPool>,
}

impl BackwardSearch {
    pub fn new(initial_position: &PositionAux, one_way: bool) -> anyhow::Result<Self> {
        Self::new_with_parallel(initial_position, one_way, 1)
    }

    pub fn new_with_parallel(
        initial_position: &PositionAux,
        one_way: bool,
        parallel: usize,
    ) -> anyhow::Result<Self> {
        let mut solution = standard_solve(initial_position.clone(), 2, true)?.solutions();
        if solution.len() != 1 {
            bail!("Not unique: {}", solution.len());
        }
        let solution = solution.remove(0);
        let mut p = initial_position.clone();
        for m in solution.iter() {
            p.do_move(m);
        }
        if !p.hands().is_empty(Color::BLACK) {
            bail!("Extra black pieces in checkmate");
        }

        let positions = vec![initial_position.core().clone()];

        let mut memo = NoHashMap64::default();
        let mut prev_memo = NoHashMap64::default();
        let mut p = initial_position.clone();
        memo.insert(p.digest(), StepRange::exact(solution.len() as u16));
        for (i, m) in solution.iter().enumerate() {
            p.do_move(m);
            if i % 2 == 0 {
                prev_memo.insert(
                    p.digest(),
                    StepRange::exact((solution.len() - i - 1) as u16),
                );
            } else {
                memo.insert(
                    p.digest(),
                    StepRange::exact((solution.len() - i - 1) as u16),
                );
            }
        }

        let step = solution.len() as u16;

        let parallel = parallel.clamp(1, MAX_BACKWARD_PARALLEL);
        let pool = if parallel > 1 {
            Some(
                rayon::ThreadPoolBuilder::new()
                    .num_threads(parallel)
                    .build()?,
            )
        } else {
            None
        };

        Ok(BackwardSearch {
            initial_position: initial_position.clone(),
            solution,
            seen_positions: 0,
            positions,
            prev_positions: vec![],
            memo,
            prev_memo,
            stone: *initial_position.stone(),
            step,
            one_way,
            parallel,
            pool,
        })
    }

    pub fn advance(&mut self) -> anyhow::Result<bool> {
        if !self.one_way && self.parallel > 1 && self.seen_positions == 0 {
            return self.advance_parallel();
        }
        self.advance_upto(usize::MAX / 2)
    }

    pub fn advance_upto(&mut self, upto: usize) -> anyhow::Result<bool> {
        self.advance_upto_with_filter(upto, |_, _| true)
    }

    pub fn advance_upto_with_filter(
        &mut self,
        upto: usize,
        mut filter: impl FnMut(&Position, Option<BitBoard>) -> bool,
    ) -> anyhow::Result<bool> {
        let range = self.seen_positions..(self.seen_positions + upto).min(self.positions.len());
        self.seen_positions = range.end;
        let mut undo_moves = vec![];
        let mut solution_scratch = vec![];
        for core in self.positions[range].iter() {
            let mut position = PositionAux::new(core.clone(), self.stone);
            undo_moves.clear();
            previous(&mut position, self.step > 0, &mut undo_moves);

            for m in undo_moves.iter() {
                let mut pp = position.clone();
                pp.undo_move(m);

                if !is_backward_candidate_legal(&mut pp) {
                    continue;
                }

                if !filter(pp.core(), self.stone) {
                    continue;
                }

                if self.one_way {
                    let mut branches = vec![];
                    let options = crate::position::AdvanceOptions {
                        max_allowed_branches: Some(1),
                    };
                    if crate::position::advance::advance::advance_aux(
                        &mut pp,
                        &options,
                        &mut branches,
                    )
                    .is_ok()
                    {
                        // In one-way mate, there must be exactly 1 move (or 1 move + 1 pawn drop which is illegal mate).
                        // If it has >0 moves, we just trust it, because we already know `pp` can reach `position` which is a mate.
                        // Actually, to be strictly one-way, we just check that advance_aux didn't fail (meaning <= 1 non-pawn-drop branch).
                        if !branches.is_empty() {
                            self.prev_positions.push(pp.core().clone());
                            self.prev_memo
                                .insert(pp.digest(), StepRange::exact(self.step + 1));
                        }
                    }
                    continue;
                }

                let pp_digest = pp.digest();
                let ans =
                    if let Some(ans) = self.prev_memo.get(&pp_digest).filter(|ans| {
                        !ans.needs_investigation(self.step + 1)
                    }) {
                        *ans
                    } else {
                        solutions(
                            &mut pp,
                            &mut self.prev_memo,
                            &mut self.memo,
                            self.step + 1,
                            &mut solution_scratch,
                        )
                    };
                if ans.is_uniquely(self.step + 1) {
                    #[cfg(debug_assertions)]
                    {
                        let sol = standard_solve(pp.clone(), 2, true).unwrap().solutions();
                        if sol.len() != 1 {
                            eprintln!("Not unique: {} {}", sol.len(), pp.sfen_url());
                            for sol in sol.iter() {
                                let m = &sol[0];
                                let mut p = pp.clone();
                                p.do_move(m);
                                eprintln!(
                                    "{} {} {:?} {:?}",
                                    self.step,
                                    p.sfen_url(),
                                    m,
                                    self.memo.get(&p.digest())
                                );
                            }
                            debug_assert_eq!(sol.len(), 1);
                        }
                    }

                    self.prev_positions.push(pp.core().clone());
                }
            }
        }

        if self.seen_positions < self.positions.len() {
            return Ok(true);
        }

        if self.prev_positions.is_empty() {
            return Ok(false);
        }

        std::mem::swap(&mut self.positions, &mut self.prev_positions);
        std::mem::swap(&mut self.memo, &mut self.prev_memo);
        self.prev_positions.clear();
        self.seen_positions = 0;

        self.step += 1;

        Ok(true)
    }

    fn advance_parallel(&mut self) -> anyhow::Result<bool> {
        if self.positions.is_empty() {
            return Ok(false);
        }

        let step = self.step;
        let stone = self.stone;
        let position_parallel = self.parallel.min(self.positions.len());
        let position_chunk_size = self
            .positions
            .len()
            .div_ceil(position_parallel * 8)
            .max(1);
        let pool = self.pool.as_ref().expect("parallel pool");
        let candidate_chunks = pool.install(|| {
            self.positions
                .par_chunks(position_chunk_size)
                .map(|chunk| {
                    let mut undo_moves = vec![];
                    let mut candidates = vec![];

                    for core in chunk.iter() {
                        let mut position = PositionAux::new(core.clone(), stone);
                        undo_moves.clear();
                        previous(&mut position, step > 0, &mut undo_moves);

                        for m in undo_moves.iter() {
                            let mut pp = position.clone();
                            pp.undo_move(m);
                            if !is_backward_candidate_legal(&mut pp) {
                                continue;
                            }
                            candidates.push(pp.core().clone());
                        }
                    }

                    candidates
                })
                .collect::<Vec<_>>()
        });
        let candidate_len = candidate_chunks.iter().map(Vec::len).sum::<usize>();
        let mut candidates = Vec::with_capacity(candidate_len);
        for chunk in candidate_chunks {
            candidates.extend(chunk);
        }

        if candidates.is_empty() {
            return Ok(false);
        }

        let parallel = self.parallel.min(candidates.len());
        let chunk_size = candidates.len().div_ceil(parallel * 8).max(1);
        let memo = Arc::new(std::mem::take(&mut self.memo));
        let prev_memo = Arc::new(std::mem::take(&mut self.prev_memo));

        let results = pool.install(|| {
            candidates
                .par_chunks(chunk_size)
                .map(|chunk| {
                    let memo = Arc::clone(&memo);
                    let prev_memo = Arc::clone(&prev_memo);
                    let mut memo_delta = NoHashMap64::default();
                    let mut prev_memo_delta = NoHashMap64::default();
                    let mut prev_positions = vec![];
                    let mut solution_scratch = vec![];

                    for core in chunk.iter() {
                        let mut pp = PositionAux::new(core.clone(), stone);
                        let pp_digest = pp.digest();
                        if let Some(ans) =
                            get_overlay(&prev_memo_delta, &prev_memo, pp_digest)
                                .filter(|ans| !ans.needs_investigation(step + 1))
                        {
                            if ans.is_uniquely(step + 1) {
                                prev_positions.push(core.clone());
                            }
                            continue;
                        }

                        let ans = solutions_overlay(
                            &mut pp,
                            &prev_memo,
                            &mut prev_memo_delta,
                            &memo,
                            &mut memo_delta,
                            step + 1,
                            &mut solution_scratch,
                        );
                        if ans.is_uniquely(step + 1) {
                            prev_positions.push(core.clone());
                        }
                    }

                    (prev_positions, memo_delta, prev_memo_delta)
                })
                .collect::<Vec<_>>()
        });

        let mut next_positions = vec![];
        self.memo = Arc::try_unwrap(memo).ok().unwrap();
        self.prev_memo = Arc::try_unwrap(prev_memo).ok().unwrap();

        for (positions, memo_delta, prev_memo_delta) in results {
            next_positions.extend(positions);
            self.memo.extend(memo_delta);
            self.prev_memo.extend(prev_memo_delta);
        }

        if next_positions.is_empty() {
            return Ok(false);
        }

        self.positions = next_positions;
        self.prev_positions.clear();
        std::mem::swap(&mut self.memo, &mut self.prev_memo);
        self.seen_positions = 0;
        self.step += 1;

        Ok(true)
    }

    pub fn step(&self) -> u16 {
        self.step
    }

    pub fn positions(&self) -> (/* stone */ Option<BitBoard>, &[Position]) {
        (self.stone, &self.positions)
    }

    pub fn forward(&mut self) {
        if self.solution.is_empty() {
            return;
        }
        self.initial_position.do_move(&self.solution.remove(0));
        self.positions = vec![self.initial_position.core().clone()];
        self.prev_positions.clear();
        std::mem::swap(&mut self.memo, &mut self.prev_memo);
        self.seen_positions = 0;
        self.step = self.solution.len() as u16;
    }
}

#[inline(always)]
fn is_backward_candidate_legal(position: &mut PositionAux) -> bool {
    if position.turn().is_white() {
        let Some(att) =
            crate::position::advance::attack_prevent::attacker(position, Color::WHITE, false)
        else {
            return false;
        };
        if position.checked_slow(Color::BLACK) {
            return false;
        }
        if let Some((pos2, kind2)) = att.double_check {
            let king_pos = position.king_pos(Color::WHITE).unwrap();
            let (pos1, kind1) = (att.pos, att.kind);

            let dist = |pos: crate::position::Square| -> usize {
                let dx = (pos.col() as isize - king_pos.col() as isize).abs();
                let dy = (pos.row() as isize - king_pos.row() as isize).abs();
                std::cmp::max(dx, dy) as usize
            };

            let is_slider = |kind: crate::piece::Kind| -> bool {
                matches!(
                    kind,
                    crate::piece::Kind::Lance
                        | crate::piece::Kind::Bishop
                        | crate::piece::Kind::Rook
                        | crate::piece::Kind::ProBishop
                        | crate::piece::Kind::ProRook
                )
            };

            let possible =
                (is_slider(kind1) && dist(pos1) >= 2) || (is_slider(kind2) && dist(pos2) >= 2);
            if !possible {
                return false;
            }
        }
    } else if position.checked_slow(Color::WHITE) {
        return false;
    }
    true
}

const INF_START: u16 = u16::MAX - 2;
const INF_END: u16 = u16::MAX - 1;

fn solutions(
    position: &mut PositionAux,
    memo: &mut NoHashMap64<StepRange>,
    next_memo: &mut NoHashMap64<StepRange>,
    mate_in: u16,
    scratch: &mut Vec<Vec<Movement>>,
) -> StepRange {
    if scratch.len() <= mate_in as usize {
        scratch.resize_with(mate_in as usize + 1, Vec::new);
    }
    solutions_inner(position, memo, next_memo, mate_in, scratch)
}

fn solutions_inner(
    position: &mut PositionAux,
    memo: &mut NoHashMap64<StepRange>,
    next_memo: &mut NoHashMap64<StepRange>,
    mate_in: u16,
    scratch: &mut [Vec<Movement>],
) -> StepRange {
    let mut ans = StepRange::unknown();
    if let Some(a) = memo.get(&position.digest()) {
        if !a.needs_investigation(mate_in) {
            return *a;
        }
        ans = *a;
    }

    if mate_in == 0 {
        let mut movements = std::mem::take(&mut scratch[0]);
        movements.clear();
        let options = crate::position::AdvanceOptions {
            max_allowed_branches: Some(0),
        };
        let advance_result = advance_aux(position, &options, &mut movements);
        let hint = if advance_result.is_err() {
            StepRange::non_zero()
        } else if advance_result.unwrap() {
            StepRange::exact(0)
        } else if movements.is_empty() {
            StepRange::unsolvable()
        } else {
            StepRange::non_zero()
        };
        let ans = ans.intersection(&hint);
        debug_assert!(!ans.needs_investigation(mate_in));
        memo.insert(position.digest(), ans);
        scratch[0] = movements;
        return ans;
    }

    let scratch_index = mate_in as usize;
    let mut movements = std::mem::take(&mut scratch[scratch_index]);
    movements.clear();
    let is_mate = advance_aux(position, &Default::default(), &mut movements).unwrap();

    let mut hint = StepRange::unknown();
    if is_mate {
        hint = StepRange::exact(0);
        debug_assert!(!hint.needs_investigation(mate_in));
    } else if movements.is_empty() {
        hint = StepRange::unsolvable();
        debug_assert!(!hint.needs_investigation(mate_in));
    } else if mate_in == 0 {
        hint = StepRange::non_zero();
    }
    ans = ans.intersection(&hint);
    if !ans.needs_investigation(mate_in) {
        memo.insert(position.digest(), ans);
        scratch[scratch_index] = movements;
        return ans;
    }

    let mut res = StepRange::unsolvable();

    for m in movements.iter() {
        let child_digest = position.moved_digest(m);
        let child = next_memo
            .get(&child_digest)
            .filter(|a| !a.needs_investigation(mate_in - 1))
            .cloned();
        let a = if let Some(child) = child {
            child.inc()
        } else {
            let mut np = position.clone();
            np.do_move(m);
            solutions_inner(&mut np, next_memo, memo, mate_in - 1, scratch).inc()
        };
        debug_assert!(!a.needs_investigation(mate_in));

        res.update_with_child(&a);

        if res.definitely_shorter_or_non_unique(mate_in) {
            res.shortest_start = 1;
            res.next_start = 1;
            break;
        }
    }

    res = res.intersection(&ans);

    debug_assert!(
        !res.needs_investigation(mate_in),
        "{:?} {:?} {:?} {}",
        res,
        hint,
        position,
        mate_in
    );

    memo.insert(position.digest(), res);
    scratch[scratch_index] = movements;
    res
}

#[inline(always)]
fn get_overlay(
    delta: &NoHashMap64<StepRange>,
    base: &NoHashMap64<StepRange>,
    digest: u64,
) -> Option<StepRange> {
    delta.get(&digest).copied().or_else(|| base.get(&digest).copied())
}

fn solutions_overlay(
    position: &mut PositionAux,
    memo_base: &NoHashMap64<StepRange>,
    memo_delta: &mut NoHashMap64<StepRange>,
    next_memo_base: &NoHashMap64<StepRange>,
    next_memo_delta: &mut NoHashMap64<StepRange>,
    mate_in: u16,
    scratch: &mut Vec<Vec<Movement>>,
) -> StepRange {
    if scratch.len() <= mate_in as usize {
        scratch.resize_with(mate_in as usize + 1, Vec::new);
    }
    solutions_overlay_inner(
        position,
        memo_base,
        memo_delta,
        next_memo_base,
        next_memo_delta,
        mate_in,
        scratch,
    )
}

fn solutions_overlay_inner(
    position: &mut PositionAux,
    memo_base: &NoHashMap64<StepRange>,
    memo_delta: &mut NoHashMap64<StepRange>,
    next_memo_base: &NoHashMap64<StepRange>,
    next_memo_delta: &mut NoHashMap64<StepRange>,
    mate_in: u16,
    scratch: &mut [Vec<Movement>],
) -> StepRange {
    let digest = position.digest();
    let mut ans = StepRange::unknown();
    if let Some(a) = get_overlay(memo_delta, memo_base, digest) {
        if !a.needs_investigation(mate_in) {
            return a;
        }
        ans = a;
    }

    if mate_in == 0 {
        let mut movements = std::mem::take(&mut scratch[0]);
        movements.clear();
        let options = crate::position::AdvanceOptions {
            max_allowed_branches: Some(0),
        };
        let advance_result = advance_aux(position, &options, &mut movements);
        let hint = if advance_result.is_err() {
            StepRange::non_zero()
        } else if advance_result.unwrap() {
            StepRange::exact(0)
        } else if movements.is_empty() {
            StepRange::unsolvable()
        } else {
            StepRange::non_zero()
        };
        let ans = ans.intersection(&hint);
        debug_assert!(!ans.needs_investigation(mate_in));
        memo_delta.insert(digest, ans);
        scratch[0] = movements;
        return ans;
    }

    let scratch_index = mate_in as usize;
    let mut movements = std::mem::take(&mut scratch[scratch_index]);
    movements.clear();
    let is_mate = advance_aux(position, &Default::default(), &mut movements).unwrap();

    let mut hint = StepRange::unknown();
    if is_mate {
        hint = StepRange::exact(0);
        debug_assert!(!hint.needs_investigation(mate_in));
    } else if movements.is_empty() {
        hint = StepRange::unsolvable();
        debug_assert!(!hint.needs_investigation(mate_in));
    } else if mate_in == 0 {
        hint = StepRange::non_zero();
    }
    ans = ans.intersection(&hint);
    if !ans.needs_investigation(mate_in) {
        memo_delta.insert(digest, ans);
        scratch[scratch_index] = movements;
        return ans;
    }

    let mut res = StepRange::unsolvable();

    for m in movements.iter() {
        let child_digest = position.moved_digest(m);
        let child = get_overlay(next_memo_delta, next_memo_base, child_digest)
            .filter(|a| !a.needs_investigation(mate_in - 1));
        let a = if let Some(child) = child {
            child.inc()
        } else {
            let mut np = position.clone();
            np.do_move(m);
            solutions_overlay_inner(
                &mut np,
                next_memo_base,
                next_memo_delta,
                memo_base,
                memo_delta,
                mate_in - 1,
                scratch,
            )
            .inc()
        };
        debug_assert!(!a.needs_investigation(mate_in));

        res.update_with_child(&a);

        if res.definitely_shorter_or_non_unique(mate_in) {
            res.shortest_start = 1;
            res.next_start = 1;
            break;
        }
    }

    res = res.intersection(&ans);

    debug_assert!(
        !res.needs_investigation(mate_in),
        "{:?} {:?} {:?} {}",
        res,
        hint,
        position,
        mate_in
    );

    memo_delta.insert(digest, res);
    scratch[scratch_index] = movements;
    res
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct StepRange {
    // Second shortest solution range
    next_start: u16,
    next_end: u16,
    // Shortest solution range
    shortest_start: u16,
    shortest_end: u16,
}

#[inline(always)]
fn intersection_bounds(
    a_start: u16,
    a_end: u16,
    b_start: u16,
    b_end: u16,
) -> (u16, u16) {
    let start = a_start.max(b_start);
    let end = a_end.min(b_end);
    if start >= end {
        (0, 0)
    } else {
        (start, end)
    }
}

#[inline(always)]
fn definitely_shorter(start: u16, end: u16, step: u16) -> bool {
    let (start, end) = intersection_bounds(start, end, step, INF_END);
    start >= end
}

#[inline(always)]
fn definitely_longer(start: u16, end: u16, step: u16) -> bool {
    let (start, end) = intersection_bounds(start, end, 0, step + 1);
    start >= end
}

#[inline(always)]
fn exactly(start: u16, end: u16, step: u16) -> bool {
    start == step && end == step + 1
}

impl StepRange {
    #[inline(always)]
    fn new(mut shortest: Range<u16>, mut next: Range<u16>) -> Self {
        debug_assert!(shortest.start <= next.start);
        debug_assert!(shortest.end <= next.end);

        shortest.start = shortest.start.min(INF_START);
        shortest.end = shortest.end.min(INF_END);
        next.start = next.start.min(INF_START);
        next.end = next.end.min(INF_END);

        StepRange {
            shortest_start: shortest.start,
            shortest_end: shortest.end,
            next_start: next.start,
            next_end: next.end,
        }
    }

    #[inline(always)]
    fn exact(step: u16) -> Self {
        Self::new(step..step + 1, step + 1..INF_END)
    }

    #[inline(always)]
    fn unsolvable() -> Self {
        Self::new(INF_START..INF_END, INF_START..INF_END)
    }

    #[inline(always)]
    fn unknown() -> Self {
        Self::new(0..INF_END, 0..INF_END)
    }

    #[inline(always)]
    fn non_zero() -> Self {
        Self::new(1..INF_END, 1..INF_END)
    }

    #[inline(always)]
    fn inc(&self) -> Self {
        Self::new(
            self.shortest_start + 1..self.shortest_end + 1,
            self.next_start + 1..self.next_end + 1,
        )
    }

    #[inline(always)]
    fn definitely_shorter_or_non_unique(&self, step: u16) -> bool {
        self.shortest_end <= step || self.shortest_end == step + 1 && self.next_end == step + 1
    }

    #[inline(always)]
    fn needs_investigation(&self, mate_in: u16) -> bool {
        if self.definitely_shorter_or_non_unique(mate_in)
            || definitely_longer(self.shortest_start, self.shortest_end, mate_in)
        {
            return false;
        }
        if exactly(self.shortest_start, self.shortest_end, mate_in) {
            debug_assert!(!definitely_shorter(self.next_start, self.next_end, mate_in));
            if definitely_longer(self.next_start, self.next_end, mate_in)
                || exactly(self.next_start, self.next_end, mate_in)
            {
                return false;
            }
        }
        true
    }

    #[inline(always)]
    fn intersection(&self, hint: &StepRange) -> StepRange {
        let (shortest_start, shortest_end) = intersection_bounds(
            self.shortest_start,
            self.shortest_end,
            hint.shortest_start,
            hint.shortest_end,
        );
        let (next_start, next_end) = intersection_bounds(
            self.next_start,
            self.next_end,
            hint.next_start,
            hint.next_end,
        );
        Self::new(
            shortest_start..shortest_end,
            next_start..next_end,
        )
    }

    #[inline(always)]
    fn update_with_child(&mut self, c: &StepRange) {
        for (start, end) in [
            (c.shortest_start, c.shortest_end),
            (c.next_start, c.next_end),
        ] {
            if start < self.shortest_start {
                self.next_start = self.shortest_start;
                self.shortest_start = start;
            } else if start < self.next_start {
                self.next_start = start;
            }

            if end < self.shortest_end {
                self.next_end = self.shortest_end;
                self.shortest_end = end;
            } else if end < self.next_end {
                self.next_end = end;
            }
        }
    }

    #[inline(always)]
    fn is_uniquely(&self, step: u16) -> bool {
        exactly(self.shortest_start, self.shortest_end, step)
            && definitely_longer(self.next_start, self.next_end, step)
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        position::position::PositionAux,
        search::backward::{backward_initial_variants, backward_search},
    };

    #[test]
    fn test_backward_search() {
        for (sfen, (want_step, mut want_sfens)) in [
            (
                "9/9/9/9/9/6OOO/6O1k/6OO+P/8P w - 1",
                (1, vec!["9/9/9/9/9/6OOO/6O1k/6OO1/7+PP b - 1"]),
            ),
            (
                "9/9/9/7OO/7Ok/7OP/7O1/7O1/7OL w - 1",
                (3, vec!["9/9/9/7OO/7O1/7Ok/7O1/7OP/7OL b - 1"]),
            ),
            (
                "9/9/9/7OO/7Ok/7O1/7OP/7O1/7OL b - 1",
                (3, vec!["9/9/9/7OO/7O1/7Ok/7O1/7OP/7OL b - 1"]),
            ),
            (
                "9/9/9/9/9/5OOOO/5OR1k/5O1p1/5O2P w - 1",
                (
                    19,
                    vec![
                        "9/9/9/9/9/5OOOO/5O2+p/5Ok+p1/5O2R b - 1",
                        "9/9/9/9/9/5OOOO/5O2R/5Ok+p1/5O2+p b - 1",
                        "9/9/9/9/9/5OOOO/5O2p/5Ok+p1/5O2R b - 1",
                    ],
                ),
            ),
            (
                "6ppp/6P2/9/9/9/5OOOO/5O2k/5O1PR/5O2P w - 1",
                (0, vec!["6ppp/6P2/9/9/9/5OOOO/5O2k/5O1PR/5O2P w - 1"]),
            ),
        ] {
            let initial_position = PositionAux::from_sfen(sfen).unwrap();
            let (step, mut positions) = backward_search(&initial_position, true, 0, false).unwrap();

            assert_eq!(step, want_step, "{:?}", initial_position);

            want_sfens.sort();
            let want_positions = want_sfens
                .iter()
                .map(|sfen| PositionAux::from_sfen(sfen).unwrap())
                .collect::<Vec<_>>();

            positions.sort_by_key(|a| a.clone().sfen());

            assert_eq!(positions, want_positions)
        }
    }

    #[test]
    fn test_backward_initial_variants() {
        let position = PositionAux::from_sfen("9/9/9/9/9/9/9/9/4k4 b - 1").unwrap();
        let variants = backward_initial_variants(&position);
        assert_eq!(variants.len(), 2);
        assert!(variants.iter().any(|p| !p.pawn_drop()));
        assert!(variants.iter().any(|p| p.pawn_drop()));

        let position = PositionAux::from_sfen("9/9/9/9/9/9/9/9/4k4 b - -1").unwrap();
        let variants = backward_initial_variants(&position);
        assert_eq!(variants.len(), 2);
        assert!(variants.iter().any(|p| !p.pawn_drop()));
        assert!(variants.iter().any(|p| p.pawn_drop()));
    }
}
