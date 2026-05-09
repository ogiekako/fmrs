use std::collections::HashSet;

use anyhow::bail;
use log::info;

use crate::memo::{Memo, MemoTrait};
use crate::nohash::NoHashSet64;
use crate::piece::Color;
use crate::position::advance::advance::advance_aux;
use crate::position::bitboard::{bishop_power, power, rook_power};
use crate::position::position::PositionAux;
use crate::position::{AdvanceOptions, BitBoard, Movement, Position, PositionExt, Square};

use super::reconstruct::Reconstructor;
use super::standard_solve::SolverStatus;

/// Conservative test: returns true only when, after applying `m` (a white
/// move) to a position with the given black king square, black is DEFINITELY
/// not in check. False = unknown (full attacker check needed).
///
/// Used to skip the `attacker(BLACK, ...)` call inside `black::Context::new`
/// for white-escape moves that obviously can't put black in check (the common
/// case in tsumeshogi: white pieces are clustered defensively far from black
/// king).
fn black_safe_after_white_move(black_king_pos: Square, m: &Movement) -> bool {
    let (dest, dest_kind, source) = match m {
        Movement::Drop(pos, kind) => (*pos, *kind, None),
        Movement::Move {
            source,
            source_kind_hint,
            dest,
            promote,
            ..
        } => {
            let source_kind = match source_kind_hint {
                Some(k) => *k,
                // Without a hint we can't reason about discovered check
                // cheaply; bail out.
                None => return false,
            };
            let dest_kind = if *promote {
                match source_kind.promote() {
                    Some(k) => k,
                    None => source_kind,
                }
            } else {
                source_kind
            };
            (*dest, dest_kind, Some(*source))
        }
    };

    // Direct check: white piece at dest attacks black king.
    if power(Color::WHITE, dest, dest_kind).contains(black_king_pos) {
        return false;
    }

    // For drops, no source means no discovered-check possibility.
    let Some(source) = source else {
        return true;
    };

    // Discovered: source was on a line through black king with a white line
    // piece behind it. We approximate cheaply: if source is not on any line
    // from black king, no discovered possible.
    let bk_lines = bishop_power(black_king_pos) | rook_power(black_king_pos);
    if !bk_lines.contains(source) {
        return true;
    }

    // Source on king's line: discovered MAY apply, fall back to attacker().
    false
}

/// Build AdvanceOptions for a black-to-move advance call where we may know
/// black is not in check.
fn black_options(black_safe: bool) -> AdvanceOptions {
    AdvanceOptions {
        max_allowed_branches: None,
        assume_not_in_check: black_safe,
    }
}


pub fn low_mem_standard_solve(
    position: PositionAux,
    solutions_upto: usize,
    silent: bool,
) -> anyhow::Result<Reconstructor> {
    low_mem_standard_solve_mult(vec![position], solutions_upto, silent)
}

pub fn low_mem_standard_solve_mult(
    positions: Vec<PositionAux>,
    solutions_upto: usize,
    silent: bool,
) -> anyhow::Result<Reconstructor> {
    let mut solver = LowMemStandardSolver::with_multiple(positions, solutions_upto, silent)?;
    loop {
        let status = solver.advance()?;
        match status {
            SolverStatus::Intermediate(_) => continue,
            SolverStatus::Mate(reconstructor) => return Ok(reconstructor),
            SolverStatus::NoSolution => return Ok(Reconstructor::no_solution()),
        }
    }
}

pub struct LowMemStandardSolver {
    initial_position_digests: NoHashSet64,
    solutions_upto: usize,
    step: u16,
    positions: Vec<Position>,
    tmp_positions: Vec<Position>,
    movements: Vec<Movement>,
    tmp_movements: Vec<Movement>,
    mate_positions: Vec<PositionAux>,
    memo_white_turn: Memo,
    stone: Option<BitBoard>,
    silent: bool,
}

impl LowMemStandardSolver {
    pub fn new(position: PositionAux, solutions_upto: usize, silent: bool) -> anyhow::Result<Self> {
        Self::with_multiple(vec![position], solutions_upto, silent)
    }

    pub fn with_multiple(
        positions: Vec<PositionAux>,
        solutions_upto: usize,
        silent: bool,
    ) -> anyhow::Result<Self> {
        if positions.is_empty() {
            bail!("No initial positions");
        }

        if positions.iter().any(|p| p.is_illegal_initial_position()) {
            bail!("Illegal initial position");
        }

        let initial_position_digests: NoHashSet64 = positions.iter().map(|p| p.digest()).collect();

        let mut memo = Memo::default();
        for digest in initial_position_digests.iter() {
            memo.contains_or_insert(*digest, 0);
        }

        let turns = positions.iter().map(|p| p.turn()).collect::<HashSet<_>>();
        if turns.len() > 1 {
            bail!("Multiple turns");
        }
        let turn = turns.iter().next().copied().unwrap();

        let stones = positions.iter().map(|p| p.stone()).collect::<HashSet<_>>();
        if stones.len() > 1 {
            bail!("Multiple stone formations");
        }
        let stone = stones.iter().next().and_then(|s| **s);

        let mut mate_positions = vec![];
        let mut positions = positions.iter().map(|p| p.core().clone()).collect();
        let mut step = 0;

        if turn.is_black() {
            let mut memo_next = Memo::default();
            next_positions(
                &mut mate_positions,
                &mut memo_next,
                &mut positions,
                step,
                &stone,
            );
            std::mem::swap(&mut memo, &mut memo_next);
            step += 1;
        }

        // Pre-allocate to skip the early Vec::push grow steps. Sizes chosen
        // to cover the typical 15-ply search frontier (~1k positions, ~256
        // moves per advance) without over-committing for shallow searches.
        Ok(Self {
            initial_position_digests,
            solutions_upto,
            step,
            positions,
            tmp_positions: Vec::with_capacity(1024),
            movements: Vec::with_capacity(256),
            tmp_movements: Vec::with_capacity(256),
            mate_positions,
            memo_white_turn: memo,
            stone,
            silent,
        })
    }

    pub fn advance(&mut self) -> anyhow::Result<SolverStatus> {
        if self.positions.is_empty() {
            return Ok(SolverStatus::NoSolution);
        }

        self.next_next_positions();

        if !self.mate_positions.is_empty() {
            if !self.silent {
                info!(
                    "Found {} mates searching {} positions",
                    self.mate_positions.len(),
                    self.memo_white_turn.len(),
                );
            }

            let mate_positions = std::mem::take(&mut self.mate_positions);
            let memo_white_turn = std::mem::take(&mut self.memo_white_turn);

            let reconstructor = Reconstructor::new(
                std::mem::take(&mut self.initial_position_digests),
                mate_positions,
                Box::new(memo_white_turn),
                self.solutions_upto,
            );

            return Ok(SolverStatus::Mate(reconstructor));
        }

        self.step += 2;
        Ok(SolverStatus::Intermediate(self.step as u32))
    }

    fn next_next_positions(&mut self) {
        self.tmp_positions.clear();
        std::mem::swap(&mut self.tmp_positions, &mut self.positions);

        for core in self.tmp_positions.iter() {
            let mut outer = PositionAux::new(core.clone(), self.stone);

            self.movements.clear();
            let is_mate =
                advance_aux(&mut outer, &Default::default(), &mut self.movements).unwrap();

            if is_mate {
                self.mate_positions.push(outer.clone());
            } else if !self.mate_positions.is_empty() {
                continue;
            }

            std::mem::swap(&mut self.tmp_movements, &mut self.movements);
            // Black king pos is stable across white escapes (white's move
            // doesn't move the BLACK king); look up once.
            let black_king_pos = outer.black_king_pos();
            for m in self.tmp_movements.iter() {
                // Clone the outer PositionAux (carries cached king_pos +
                // occupied/white_bb after advance_aux's lazy fill), then apply
                // white's escape via PositionAux::do_move which incrementally
                // updates the caches. Avoids re-running PositionAux::new from
                // a Position clone and lets the next advance_aux skip its lazy
                // king_pos lookup.
                let mut position = outer.clone();
                position.do_move(m);

                // Hint: in tsumeshogi, white pieces are mostly defensive and
                // far from the black king, so most escapes can't possibly
                // check black. Skipping attacker() here is the bulk of this
                // optimization's payoff.
                let black_safe = black_king_pos
                    .map(|bk| black_safe_after_white_move(bk, m))
                    .unwrap_or(true);

                self.movements.clear();
                advance_aux(&mut position, &black_options(black_safe), &mut self.movements)
                    .unwrap();

                for m in self.movements.iter() {
                    let digest = position.moved_digest(m);
                    if self
                        .memo_white_turn
                        .contains_or_insert(digest, self.step + 2)
                    {
                        continue;
                    }

                    let mut np = position.core().clone();
                    np.do_move(m);
                    self.positions.push(np);
                }
            }
        }
    }
}

fn next_positions(
    mate_positions: &mut Vec<PositionAux>,
    memo_next: &mut Memo,
    positions: &mut Vec<Position>,
    step: u16,
    stone: &Option<BitBoard>,
) {
    let mut movements = vec![];
    for core in std::mem::take(positions) {
        let mut position = PositionAux::new(core.clone(), *stone);
        movements.clear();
        let is_mate = advance_aux(&mut position, &Default::default(), &mut movements).unwrap();

        if is_mate {
            mate_positions.push(position.clone());
        }

        for m in movements.iter() {
            let digest = position.moved_digest(m);
            if memo_next.contains_or_insert(digest, step + 1) {
                continue;
            }
            let mut np = core.clone();
            np.do_move(m);
            positions.push(np);
        }
    }
}
