use crate::{
    memo::Memo,
    position::{
        bitboard::{
            king_then_king_or_night_power, lance_reachable,
            magic::{bishop_reachable, rook_reachable},
        },
        checked,
        position::PositionAux,
        rule::{is_legal_drop, is_legal_move},
    },
};
use anyhow::Result;

use crate::{
    piece::{Color, Kind},
    position::{
        bitboard::{self, power, reachable, rule::king_power, BitBoard},
        Movement, Position, Square,
    },
};

use super::{
    common,
    pinned::{pinned, Pinned},
    AdvanceOptions,
};

// #[inline(never)]
pub(super) fn attack_preventing_movements<'p, 'a>(
    position: &'a mut PositionAux<'p>,
    memo: &'a mut Memo,
    next_step: u32,
    king_pos: Square,
    should_return_check: bool,
    options: &'a AdvanceOptions,
    attacker_hint: Option<Attacker>,
    result: &'a mut Vec<Movement>,
) -> Result</* is legal mate */ bool> {
    let mut ctx = Context::new(
        position,
        memo,
        next_step,
        king_pos,
        should_return_check,
        options,
        attacker_hint,
        result,
    )?;
    ctx.advance()?;
    Ok(ctx.is_mate && !position.pawn_drop())
}

struct Context<'p, 'a> {
    position: &'a mut PositionAux<'p>,
    occupied_without_king: BitBoard,
    king_pos: Square,
    pinned: Pinned,
    attacker: Attacker,
    pawn_mask: Option<usize>,
    next_step: u32,
    should_return_check: bool,
    // Mutable fields
    memo: &'a mut Memo,
    result: &'a mut Vec<Movement>,
    is_mate: bool,
    num_branches_without_pawn_drop: usize,

    options: &'a AdvanceOptions,
}

impl<'p, 'a> Context<'p, 'a> {
    // #[inline(never)]
    fn new(
        position: &'a mut PositionAux<'p>,
        memo: &'a mut Memo,
        next_step: u32,
        king_pos: Square,
        should_return_check: bool,
        options: &'a AdvanceOptions,
        attacker_hint: Option<Attacker>,
        result: &'a mut Vec<Movement>,
    ) -> anyhow::Result<Self> {
        let turn = position.turn();
        let color_bb = position.color_bitboard();
        let attacker = match attacker_hint {
            Some(attacker) => attacker,
            None => attacker(position, turn, king_pos, false)
                .ok_or_else(|| anyhow::anyhow!("No attacker found"))?,
        };
        let pinned = pinned(position, turn, king_pos, turn);

        let mut occupied_without_king = color_bb.both();
        occupied_without_king.unset(king_pos);

        Ok(Self {
            position,
            occupied_without_king,
            king_pos,
            pinned,
            attacker,
            pawn_mask: None,
            next_step,
            should_return_check,
            memo,
            result,
            is_mate: true,
            num_branches_without_pawn_drop: 0,
            options,
        })
    }

    fn pawn_mask(&mut self) -> usize {
        *self.pawn_mask.get_or_insert_with(|| {
            let mut mask = Default::default();
            for pos in self.position.bitboard(self.position.turn(), Kind::Pawn) {
                mask |= 1 << pos.col()
            }
            mask
        })
    }

    // #[inline(never)]
    fn advance(&mut self) -> Result<()> {
        self.king_move()?;

        if self.attacker.double_check.is_none() {
            self.capture(self.attacker.pos)?;
            self.block(self.attacker.pos, self.attacker.kind)?;
        }

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
        let king_color = self.position.turn();
        let attacker_color = king_color.opposite();
        let attacker_color_bb = self.position.color_bb(attacker_color);

        let mut king_reachable =
            king_power(self.king_pos).and_not(self.position.color_bb(king_color));
        if king_reachable.is_empty() {
            return Ok(());
        }
        let mut seen_cands = BitBoard::default();
        let non_line_cands =
            king_then_king_or_night_power(king_color, self.king_pos) & attacker_color_bb;
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
        let bishipish = self.position.bishopish() & attacker_color_bb;
        let rookish = self.position.rookish() & attacker_color_bb;

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
                Movement::move_with_hint(self.king_pos, Kind::King, dest, false, capture_kind),
                Kind::King,
            )?;
        }
        Ok(())
    }

    fn add_movements_to(&mut self, dest: Square, include_drop: bool) -> Result<()> {
        // Drop
        if include_drop {
            for kind in self.position.hands().kinds(self.position.turn()) {
                let pawn_mask = (kind == Kind::Pawn).then(|| self.pawn_mask()).unwrap_or(0);
                if is_legal_drop(self.position.turn(), dest, kind, pawn_mask) {
                    self.maybe_add_move(Movement::Drop(dest, kind), kind)?;
                }
            }
        }

        let capture_kind = if include_drop {
            None
        } else {
            self.position.get_kind(dest)
        };

        // Move
        let around_dest = king_power(dest) & self.position.color_bb(self.position.turn());
        for source_pos in around_dest {
            let source_kind = self.position.must_get_kind(source_pos);
            if source_kind == Kind::King {
                continue;
            }
            let source_power = self
                .pinned
                .pinned_area(source_pos)
                .unwrap_or_else(|| bitboard::power(self.position.turn(), source_pos, source_kind));
            if source_power.get(dest) {
                for promote in [false, true] {
                    if promote && source_kind.promote().is_none() {
                        continue;
                    }
                    if !is_legal_move(self.position.turn(), source_pos, dest, source_kind, promote)
                    {
                        continue;
                    }
                    self.maybe_add_move(
                        Movement::move_with_hint(
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
                let raw_pieces = self.position.bitboard(self.position.turn(), leap_kind);
                let promoted_kind = leap_kind.promote().unwrap();
                if promoted_kind.is_line_piece() {
                    raw_pieces | self.position.bitboard(self.position.turn(), promoted_kind)
                } else {
                    raw_pieces
                }
            };
            if on_board.is_empty() {
                continue;
            }
            let sources = bitboard::reachable(
                self.position,
                self.position.turn().opposite(),
                dest,
                leap_kind,
                false,
            ) & on_board;
            for source_pos in sources {
                if self.pinned.is_unpin_move(source_pos, dest) {
                    continue;
                }
                let source_kind = self.position.must_get_kind(source_pos);
                for promote in [false, true] {
                    if promote && !source_kind.is_promotable() {
                        continue;
                    }
                    if !is_legal_move(self.position.turn(), source_pos, dest, source_kind, promote)
                    {
                        continue;
                    }
                    self.maybe_add_move(
                        Movement::move_with_hint(
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
impl<'p, 'a> Context<'p, 'a> {
    fn update<'b>(
        &self,
        new_position: &'b mut Option<Position>,
        movement: &Movement,
    ) -> &'b Position {
        if new_position.is_none() {
            *new_position = self.position.moved(movement).into();
        }
        new_position.as_ref().unwrap()
    }

    fn maybe_add_move<'b>(&mut self, movement: Movement, kind: Kind) -> Result<()> {
        let is_king_move = kind == Kind::King;

        let mut new_position = None;

        // TODO: check the second attacker
        if !is_king_move && self.attacker.double_check.is_some() {
            let mut np = PositionAux::new(self.update(&mut new_position, &movement));
            if checked(&mut np, self.position.turn(), self.king_pos.into()) {
                return Ok(());
            }
        }

        if self.should_return_check {
            let mut np = PositionAux::new(self.update(&mut new_position, &movement));
            if !checked(&mut np, self.position.turn().opposite(), None) {
                return Ok(());
            }
        }

        self.is_mate = false;
        if !movement.is_pawn_drop() {
            self.num_branches_without_pawn_drop += 1;
            self.options
                .check_allowed_branches(self.num_branches_without_pawn_drop)?;
        }

        debug_assert!(
            !common::checked(
                &mut PositionAux::new(self.update(&mut new_position, &movement)),
                self.position.turn(),
                None,
            ),
            "{:?} king checked: posision={:?} movement={:?} next={:?}",
            self.position.turn(),
            self.position,
            movement,
            new_position.as_ref().unwrap()
        );

        if !self.options.no_memo {
            self.update(&mut new_position, &movement);
            let digest = new_position.as_ref().unwrap().digest();

            let mut contains = true;
            self.memo.entry(digest).or_insert_with(|| {
                contains = false;
                self.next_step
            });

            if contains {
                // Already seen during search on other branches.
                return Ok(());
            }
        }

        self.result.push(movement);

        Ok(())
    }

    fn blockable_squares(&mut self, attacker_pos: Square, attacker_kind: Kind) -> BitBoard {
        if king_power(self.king_pos).get(attacker_pos) {
            return BitBoard::default();
        }
        bitboard::reachable(
            self.position,
            self.position.turn(),
            self.king_pos,
            attacker_kind.maybe_unpromote(),
            false,
        ) & bitboard::reachable(
            self.position,
            self.position.turn().opposite(),
            attacker_pos,
            attacker_kind.maybe_unpromote(),
            true,
        )
    }
}

#[derive(Clone, Debug)]
pub struct Attacker {
    pub pos: Square,
    pub kind: Kind,
    pub double_check: Option<(Square, Kind)>,
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

pub fn attacker(
    position: &mut PositionAux<'_>,
    king_color: Color,
    king_pos: Square,
    early_return: bool,
) -> Option<Attacker> {
    let opponent_bb = position.color_bb(king_color.opposite());

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
            position.goldish()
        } else {
            position.kind_bb(attacker_kind)
        } & opponent_bb;

        if attacker_cands.is_empty() {
            continue;
        }
        attacker_cands &= power(king_color, king_pos, attacker_kind);
        if attacker_cands.is_empty() {
            continue;
        }
        if attacker_kind.is_line_piece() {
            attacker_cands &= reachable(position, king_color, king_pos, attacker_kind, false);
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
            if early_return {
                return attacker;
            }
        }
    }
    attacker
}
