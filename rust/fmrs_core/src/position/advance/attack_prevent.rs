use anyhow::Result;
use rustc_hash::FxHashMap;

use crate::{
    piece::{Color, Kind},
    position::{
        bitboard::{self, rule::king_power, BitBoard},
        Digest, Movement, Position, PositionExt, Square,
    },
};

use super::{
    common,
    pinned::{pinned, Pinned},
    AdvanceOptions,
};

pub(super) fn attack_preventing_movements(
    position: &Position,
    memo: &mut FxHashMap<Digest, u32>,
    next_step: u32,
    king_pos: Square,
    should_return_check: bool,
    options: &AdvanceOptions,
) -> Result<(Vec<Position>, /* is mate */ bool)> {
    let mut ctx = Context::new(
        position,
        memo,
        next_step,
        king_pos,
        should_return_check,
        options,
    );
    ctx.advance()?;
    Ok((ctx.result, ctx.is_mate))
}

struct Context<'a> {
    position: &'a Position,
    turn: Color,
    king_pos: Square,
    pinned: Pinned,
    attacker: Attacker,
    pawn_mask: usize,
    next_step: u32,
    should_return_check: bool,
    // Mutable fields
    memo: &'a mut FxHashMap<Digest, u32>,
    result: Vec<Position>,
    is_mate: bool,

    options: &'a AdvanceOptions,
}

impl<'a> Context<'a> {
    #[inline(never)]
    fn new(
        position: &'a Position,
        memo: &'a mut FxHashMap<Digest, u32>,
        next_step: u32,
        king_pos: Square,
        should_return_check: bool,
        options: &'a AdvanceOptions,
    ) -> Self {
        let turn = position.turn();
        let attacker = attacker(position, king_pos).expect("no attacker");
        let pinned = pinned(position, turn, king_pos, turn);
        let pawn_mask = {
            let mut mask = Default::default();
            for pos in position.bitboard(turn.into(), Kind::Pawn.into()) {
                mask |= 1 << pos.col()
            }
            mask
        };
        Self {
            position,
            turn,
            king_pos,
            pinned,
            attacker,
            pawn_mask,
            next_step,
            should_return_check,
            memo,
            result: vec![],
            is_mate: true,
            options,
        }
    }

    #[inline(never)]
    fn advance(&mut self) -> Result<()> {
        if !self.attacker.double_check {
            self.block(self.attacker.pos, self.attacker.kind)?;
            self.capture(self.attacker.pos)?;
        }
        self.king_move()?;

        Ok(())
    }

    #[inline(never)]
    fn block(&mut self, attacker_pos: Square, attacker_kind: Kind) -> Result<()> {
        if attacker_kind.is_line_piece() {
            let blockable = self.blockable_squares(attacker_pos, attacker_kind);
            for dest in blockable {
                self.add_movements_to(dest, true)?;
            }
        }
        Ok(())
    }

    #[inline(never)]
    fn capture(&mut self, attacker_pos: Square) -> Result<()> {
        self.add_movements_to(attacker_pos, false)?;

        Ok(())
    }

    #[inline(never)]
    fn king_move(&mut self) -> Result<()> {
        let king_reachable =
            king_power(self.king_pos).and_not(self.position.color_bb().bitboard(self.turn));

        let mut under_attack = BitBoard::empty();
        for attacker_kind in Kind::iter() {
            for attacker_pos in self
                .position
                .bitboard(self.turn.opposite().into(), attacker_kind.into())
            {
                let attacker_power =
                    bitboard::power(self.turn.opposite(), attacker_pos, attacker_kind);
                if (attacker_power & king_reachable).is_empty() {
                    continue;
                }
                if !attacker_kind.is_line_piece() {
                    under_attack |= attacker_power;
                    continue;
                }
                let attacker_reachable = bitboard::reachable(
                    self.position.color_bb(),
                    self.turn.opposite(),
                    attacker_pos,
                    attacker_kind,
                    true,
                );
                under_attack |= attacker_reachable;

                // Hidden by king
                if attacker_pos == self.attacker.pos {
                    if let Some(hidden_pos) = hidden_square(attacker_pos, self.king_pos) {
                        if attacker_power.get(hidden_pos) {
                            under_attack.set(hidden_pos);
                        }
                    }
                }
            }
        }

        for dest in king_reachable.and_not(under_attack) {
            self.maybe_add_move(
                &Movement::Move {
                    source: self.king_pos,
                    dest,
                    promote: false,
                },
                Kind::King,
            )?;
        }
        Ok(())
    }

    fn add_movements_to(&mut self, dest: Square, include_drop: bool) -> Result<()> {
        // Drop
        if include_drop {
            for kind in self.position.hands().kinds(self.turn) {
                self.maybe_add_move(&Movement::Drop(dest, kind), kind)?;
            }
        }

        // Move
        let around_dest = king_power(dest) & self.position.color_bb().bitboard(self.turn);
        for source_pos in around_dest {
            let source_kind = self.position.get(source_pos).unwrap().1;
            if source_kind == Kind::King {
                continue;
            }
            let source_power = if self.pinned.is_pinned(source_pos) {
                self.pinned.pinned_area(source_pos)
            } else {
                bitboard::power(self.turn, source_pos, source_kind)
            };
            if source_power.get(dest) {
                for promote in [false, true] {
                    if promote && source_kind.promote().is_none() {
                        continue;
                    }
                    self.maybe_add_move(
                        &Movement::Move {
                            source: source_pos,
                            dest,
                            promote,
                        },
                        source_kind,
                    )?;
                }
            }
        }

        for leap_kind in [Kind::Lance, Kind::Knight, Kind::Bishop, Kind::Rook] {
            let on_board = {
                let raw_pieces = self.position.bitboard(self.turn.into(), leap_kind.into());
                let promoted_kind = leap_kind.promote().unwrap();
                if promoted_kind.is_line_piece() {
                    raw_pieces
                        | self
                            .position
                            .bitboard(self.turn.into(), promoted_kind.into())
                } else {
                    raw_pieces
                }
            };
            if on_board.is_empty() {
                continue;
            }
            let sources = bitboard::reachable(
                self.position.color_bb(),
                self.turn.opposite(),
                dest,
                leap_kind,
                false,
            ) & on_board;
            for source_pos in sources {
                if self.pinned.is_unpin_move(source_pos, dest) {
                    continue;
                }
                let source_kind = self.position.get(source_pos).unwrap().1;
                for promote in [false, true] {
                    if promote && source_kind.promote().is_none() {
                        continue;
                    }
                    self.maybe_add_move(
                        &Movement::Move {
                            source: source_pos,
                            dest,
                            promote,
                        },
                        source_kind,
                    )?;
                }
            }
        }
        Ok(())
    }
}

// Helper methods
impl<'a> Context<'a> {
    fn maybe_add_move(&mut self, movement: &Movement, kind: Kind) -> Result<()> {
        if !common::maybe_legal_movement(self.turn, movement, kind, self.pawn_mask) {
            return Ok(());
        }

        let mut next_position = self.position.clone();
        next_position.do_move(movement);

        if self.attacker.double_check && common::checked(&next_position, self.turn) {
            return Ok(());
        }

        if self.should_return_check && !common::checked(&next_position, self.turn.opposite()) {
            return Ok(());
        }

        debug_assert!(
            !common::checked(&next_position, self.turn),
            "{:?} king checked: posision={:?} movement={:?} next={:?}",
            self.turn,
            self.position,
            movement,
            next_position
        );

        self.is_mate = false;
        let digest = next_position.digest();
        if self.memo.contains_key(&digest) {
            return Ok(());
        }

        self.options.check_allowed_branches(self.result.len() + 1)?;

        self.memo.insert(digest, self.next_step);
        self.result.push(next_position);

        Ok(())
    }

    fn blockable_squares(&self, attacker_pos: Square, attacker_kind: Kind) -> BitBoard {
        if king_power(self.king_pos).get(attacker_pos) {
            return BitBoard::empty();
        }
        bitboard::reachable(
            self.position.color_bb(),
            self.turn,
            self.king_pos,
            attacker_kind.maybe_unpromote(),
            false,
        ) & bitboard::reachable(
            self.position.color_bb(),
            self.turn.opposite(),
            attacker_pos,
            attacker_kind.maybe_unpromote(),
            true,
        )
    }
}

struct Attacker {
    pos: Square,
    kind: Kind,
    double_check: bool,
}

impl Attacker {
    fn new(pos: Square, kind: Kind, double_check: bool) -> Self {
        Self {
            pos,
            kind,
            double_check,
        }
    }
}

fn attacker(position: &Position, king_pos: Square) -> Option<Attacker> {
    let king_color = position.turn();
    let mut attacker: Option<Attacker> = None;
    for attacker_kind in Kind::iter() {
        let existing = position.bitboard(king_color.opposite().into(), attacker_kind.into());
        if existing.is_empty() {
            continue;
        }
        // TODO: consider checking power first.
        let attacking = bitboard::reachable(
            position.color_bb(),
            king_color,
            king_pos,
            attacker_kind,
            false,
        ) & existing;
        if attacking.is_empty() {
            continue;
        }
        for attacker_pos in attacking {
            if let Some(mut attacker) = attacker.take() {
                attacker.double_check = true;
                return Some(attacker);
            }
            attacker = Some(Attacker::new(attacker_pos, attacker_kind, false));
        }
    }
    attacker
}

// Potentially attacked position which is currently hidden by the king.
fn hidden_square(attacker_pos: Square, king_pos: Square) -> Option<Square> {
    let (kc, kr) = (king_pos.col() as isize, king_pos.row() as isize);
    let (ac, ar) = (attacker_pos.col() as isize, attacker_pos.row() as isize);

    let (dc, dr) = (kc - ac, kr - ar);
    let d = dc.abs().max(dr.abs());
    let (rc, rr) = (kc + dc / d, kr + dr / d);
    if (0..9).contains(&rc) && (0..9).contains(&rr) {
        Some(Square::new(rc as usize, rr as usize))
    } else {
        None
    }
}