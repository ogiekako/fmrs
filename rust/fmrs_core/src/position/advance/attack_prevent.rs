use crate::{
    nohash::NoHashMap,
    position::{
        bitboard::{
            king_then_king_or_night_power, lance_reachable,
            magic::{bishop_reachable, rook_reachable},
        },
        rule::{is_legal_drop, is_legal_move},
    },
};
use anyhow::Result;

use crate::{
    piece::{Color, Kind},
    position::{
        bitboard::{self, power, reachable, rule::king_power, BitBoard},
        Movement, Position, PositionExt, Square,
    },
};

use super::{
    common,
    pinned::{pinned, Pinned},
    AdvanceOptions,
};

pub(super) fn attack_preventing_movements(
    position: &Position,
    memo: &mut NoHashMap<u32>,
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
    occupied_without_king: BitBoard,
    turn: Color,
    king_pos: Square,
    pinned: Pinned,
    attacker: Attacker,
    pawn_mask: usize,
    next_step: u32,
    should_return_check: bool,
    // Mutable fields
    memo: &'a mut NoHashMap<u32>,
    result: Vec<Position>,
    is_mate: bool,

    options: &'a AdvanceOptions,
}

impl<'a> Context<'a> {
    // #[inline(never)]
    fn new(
        position: &'a Position,
        memo: &'a mut NoHashMap<u32>,
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
            for pos in position.bitboard(turn, Kind::Pawn) {
                mask |= 1 << pos.col()
            }
            mask
        };

        let mut occupied_without_king = position.color_bb().both();
        occupied_without_king.unset(king_pos);

        Self {
            position,
            occupied_without_king,
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

    // #[inline(never)]
    fn advance(&mut self) -> Result<()> {
        if self.attacker.double_check.is_none() {
            self.block(self.attacker.pos, self.attacker.kind)?;
            self.capture(self.attacker.pos)?;
        }
        self.king_move()?;

        Ok(())
    }

    // #[inline(never)]
    fn block(&mut self, attacker_pos: Square, attacker_kind: Kind) -> Result<()> {
        if attacker_kind.is_line_piece() {
            let blockable = self.blockable_squares(attacker_pos, attacker_kind);
            for dest in blockable {
                self.add_movements_to(dest, true)?;
            }
        }
        Ok(())
    }

    // #[inline(never)]
    fn capture(&mut self, attacker_pos: Square) -> Result<()> {
        self.add_movements_to(attacker_pos, false)
    }

    // #[inline(never)]
    fn king_move(&mut self) -> Result<()> {
        let color_bb = self.position.color_bb();
        let king_color = self.turn;
        let attacker_color = king_color.opposite();

        let mut king_reachable = king_power(self.king_pos).and_not(color_bb.bitboard(self.turn));
        if king_reachable.is_empty() {
            return Ok(());
        }
        let mut seen_cands = BitBoard::empty();
        let non_line_cands = king_then_king_or_night_power(king_color, self.king_pos)
            & color_bb.bitboard(attacker_color);
        for attacker_pos in non_line_cands {
            let attacker_kind = self.position.must_get_kind(attacker_pos);
            let attacker_reach = match attacker_kind {
                Kind::Lance | Kind::Bishop | Kind::Rook => continue,
                Kind::ProBishop | Kind::ProRook => king_power(attacker_pos),
                _ => {
                    seen_cands.set(attacker_pos);
                    power(attacker_color, attacker_pos, attacker_kind)
                }
            };

            king_reachable = king_reachable.and_not(attacker_reach);
            if king_reachable.is_empty() {
                return Ok(());
            }
        }

        let lances = self.position.bitboard(attacker_color, Kind::Lance);
        let bishipish = self.position.kind_bb().bishopish() & color_bb.bitboard(attacker_color);
        let rookish = self.position.kind_bb().rookish() & color_bb.bitboard(attacker_color);

        for dest in king_reachable {
            if !lances.is_empty() {
                let attacking_lances =
                    lance_reachable(self.occupied_without_king, king_color, dest) & lances;
                if !attacking_lances.is_empty() {
                    continue;
                }
            }
            if !bishipish.is_empty() {
                let attacking_bishops =
                    bishop_reachable(self.occupied_without_king, dest) & bishipish;
                if !attacking_bishops.is_empty() {
                    continue;
                }
            }
            if !rookish.is_empty() {
                let attacking_rooks = rook_reachable(self.occupied_without_king, dest) & rookish;
                if !attacking_rooks.is_empty() {
                    continue;
                }
            }

            let capture_kind = self.position.get_kind(dest);
            self.maybe_add_move(
                &Movement::move_with_hint(self.king_pos, Kind::King, dest, false, capture_kind),
                Kind::King,
            )?;
        }
        Ok(())
    }

    fn add_movements_to(&mut self, dest: Square, include_drop: bool) -> Result<()> {
        // Drop
        if include_drop {
            for kind in self.position.hands().kinds(self.turn) {
                if is_legal_drop(self.turn, dest, kind, self.pawn_mask) {
                    self.maybe_add_move(&Movement::Drop(dest, kind), kind)?;
                }
            }
        }

        let capture_kind = if include_drop {
            None
        } else {
            self.position.get_kind(dest)
        };

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
                    if !is_legal_move(self.turn, source_pos, dest, source_kind, promote) {
                        continue;
                    }
                    self.maybe_add_move(
                        &Movement::move_with_hint(
                            source_pos,
                            source_kind,
                            dest,
                            promote,
                            capture_kind,
                        ),
                        source_kind,
                    )?;
                }
            }
        }

        for leap_kind in [Kind::Lance, Kind::Knight, Kind::Bishop, Kind::Rook] {
            let on_board = {
                let raw_pieces = self.position.bitboard(self.turn, leap_kind);
                let promoted_kind = leap_kind.promote().unwrap();
                if promoted_kind.is_line_piece() {
                    raw_pieces | self.position.bitboard(self.turn, promoted_kind)
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
                    if promote && !source_kind.is_promotable() {
                        continue;
                    }
                    if !is_legal_move(self.turn, source_pos, dest, source_kind, promote) {
                        continue;
                    }
                    self.maybe_add_move(
                        &Movement::move_with_hint(
                            source_pos,
                            source_kind,
                            dest,
                            promote,
                            capture_kind,
                        ),
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
        let is_king_move = kind == Kind::King;

        let mut next_position = self.position.clone();
        next_position.do_move(movement);

        // TODO: check the second attacker
        if !is_king_move
            && self.attacker.double_check.is_some()
            && common::checked(&next_position, self.turn)
        {
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

        // TODO:compute the digest without making a clone.
        let digest = next_position.digest();

        self.options.check_allowed_branches(self.result.len() + 1)?;

        self.memo
            .entry(digest)
            .and_modify(|prev_step| {
                if *prev_step > self.next_step {
                    *prev_step = self.next_step;
                }
            })
            .or_insert(self.next_step);
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
    double_check: Option<(Square, Kind)>,
}

impl Attacker {
    fn new(pos: Square, kind: Kind, double_check: Option<(Square, Kind)>) -> Self {
        Self {
            pos,
            kind,
            double_check,
        }
    }
}

fn attacker(position: &Position, king_pos: Square) -> Option<Attacker> {
    let king_color = position.turn();

    let opponent_bb = position.color_bb().bitboard(king_color.opposite());
    let kind_bb = position.kind_bb();

    let mut attacker: Option<Attacker> = None;

    for attacker_kind in [
        Kind::Pawn,
        Kind::Lance,
        Kind::Knight,
        Kind::Silver,
        Kind::Gold,
        Kind::Bishop,
        Kind::Rook,
        Kind::King,
        Kind::ProBishop,
        Kind::ProRook,
    ] {
        let mut attacker_cands = if attacker_kind == Kind::Gold {
            kind_bb.goldish()
        } else {
            kind_bb.bitboard(attacker_kind)
        } & opponent_bb;

        if attacker_cands.is_empty() {
            continue;
        }
        attacker_cands &= power(king_color, king_pos, attacker_kind);
        if attacker_cands.is_empty() {
            continue;
        }
        if attacker_kind.is_line_piece() {
            attacker_cands &= reachable(
                position.color_bb(),
                king_color,
                king_pos,
                attacker_kind,
                false,
            );
            if attacker_cands.is_empty() {
                continue;
            }
        }
        for attacker_pos in attacker_cands {
            if let Some(mut attacker) = attacker.take() {
                attacker.double_check = (attacker_pos, attacker_kind).into();
                return Some(attacker);
            }
            attacker = Some(Attacker::new(attacker_pos, attacker_kind, None));
        }
    }
    attacker
}
