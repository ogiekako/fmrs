use crate::{
    memo::MemoTrait,
    position::{
        bitboard::{
            king_then_king_or_night_power, knight_power, lance_reachable,
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
        bitboard::{self, power, rule::king_power, BitBoard},
        Movement, Square,
    },
};

use super::{
    common,
    pinned::{pinned, Pinned},
    AdvanceOptions,
};

// #[inline(never)]
pub(super) fn attack_preventing_movements<'a, M: MemoTrait>(
    position: &'a mut PositionAux,
    memo: &'a mut M,
    next_step: u16,
    should_return_check: bool,
    options: &'a AdvanceOptions,
    attacker_hint: Option<Attacker>,
    result: &'a mut Vec<Movement>,
) -> Result</* is legal mate */ bool> {
    let mut ctx = Context::new(
        position,
        memo,
        next_step,
        should_return_check,
        options,
        attacker_hint,
        result,
    )?;
    ctx.advance()?;
    Ok(ctx.is_mate && !position.pawn_drop())
}

struct Context<'a, M: MemoTrait> {
    position: &'a mut PositionAux,
    occupied_without_king: BitBoard,
    pinned: Pinned,
    attacker: Attacker,
    pawn_mask: Option<usize>,
    next_step: u16,
    should_return_check: bool,
    // Mutable fields
    memo: &'a mut M,
    result: &'a mut Vec<Movement>,
    is_mate: bool,
    num_branches_without_pawn_drop: usize,

    options: &'a AdvanceOptions,
}

impl<'a, M: MemoTrait> Context<'a, M> {
    // #[inline(never)]
    fn new(
        position: &'a mut PositionAux,
        memo: &'a mut M,
        next_step: u16,
        should_return_check: bool,
        options: &'a AdvanceOptions,
        attacker_hint: Option<Attacker>,
        result: &'a mut Vec<Movement>,
    ) -> anyhow::Result<Self> {
        let turn = position.turn();
        let attacker = match attacker_hint {
            Some(attacker) => attacker,
            None => attacker(position, turn, false)
                .ok_or_else(|| anyhow::anyhow!("No attacker found"))?,
        };
        let pinned = pinned(position, turn, turn);

        let mut occupied_without_king = position.occupied_bb();
        occupied_without_king.unset(position.must_king_pos(turn));

        Ok(Self {
            position,
            occupied_without_king,
            pinned,
            attacker,
            pawn_mask: None, // TODO: move to PositionAux
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

        let mut king_reachable = king_power(self.position.must_turn_king_pos())
            .and_not(self.position.color_bb(king_color));
        if king_reachable.is_empty() {
            return Ok(());
        }
        let mut seen_cands = BitBoard::default();
        let non_line_cands =
            king_then_king_or_night_power(king_color, self.position.must_turn_king_pos())
                & attacker_color_bb;
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
            let king_pos = self.position.must_turn_king_pos();
            self.maybe_add_move(
                Movement::move_with_hint(king_pos, Kind::King, dest, false, capture_kind),
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
impl<'a, M: MemoTrait> Context<'a, M> {
    // #[inline(never)]
    fn is_return_check(&self, movement: &Movement) -> bool {
        let mut np = self.position.clone();
        np.do_move(movement);
        checked(&mut np, self.position.turn().opposite())
    }

    fn maybe_add_move<'b>(&mut self, movement: Movement, kind: Kind) -> Result<()> {
        let is_king_move = kind == Kind::King;

        // TODO: check the second attacker
        if !is_king_move && self.attacker.double_check.is_some() {
            let mut np = self.position.clone();
            np.do_move(&movement);
            if checked(&mut np, self.position.turn()) {
                return Ok(());
            }
        }

        if self.should_return_check {
            if !self.is_return_check(&movement) {
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
            {
                let mut np = self.position.clone();
                np.do_move(&movement);
                !common::checked(&mut np, self.position.turn())
            },
            "{:?} king checked: posision={:?} movement={:?} next={:?}",
            self.position.turn(),
            self.position,
            movement,
            {
                let mut np = self.position.clone();
                np.do_move(&movement);
                np
            }
        );

        if !self.options.no_memo {
            let digest = self.position.moved_digest(&movement);

            if self.options.no_insertion {
                if self.memo.contains_key(&digest) {
                    return Ok(());
                }
            } else if self.memo.contains_or_insert(digest, self.next_step) {
                // Already seen during search on other branches.
                return Ok(());
            }
        }

        self.result.push(movement);

        Ok(())
    }

    fn blockable_squares(&mut self, attacker_pos: Square, attacker_kind: Kind) -> BitBoard {
        let king_pos = self.position.must_turn_king_pos();
        if king_power(king_pos).get(attacker_pos) {
            return BitBoard::default();
        }
        bitboard::reachable(
            self.position,
            self.position.turn(),
            king_pos,
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
    position: &mut PositionAux,
    king_color: Color,
    early_return: bool,
) -> Option<Attacker> {
    let mut attacker: Option<Attacker> = None;

    let mut opponent_bb = position.color_bb(king_color.opposite());
    let king_pos = position.must_king_pos(king_color);

    let king_power_area = (king_power(king_pos) | knight_power(king_color, king_pos)) & opponent_bb;

    for pos in king_power_area {
        let kind = position.must_get_kind(pos);
        if power(king_color, king_pos, kind).get(pos) {
            if update_attacker(&mut attacker, pos, kind, early_return) {
                return attacker;
            }
        }
    }
    opponent_bb = opponent_bb.and_not(king_power_area);

    // Lance
    let attacking_lances =
        lance_reachable(position.occupied_bb(), king_color, king_pos) & opponent_bb;
    if !attacking_lances.is_empty() {
        let attacker_pos = attacking_lances.singleton();
        let kind = position.must_get_kind(attacker_pos);
        if matches!(kind, Kind::Lance | Kind::Rook | Kind::ProRook) {
            if update_attacker(&mut attacker, attacker_pos, kind, early_return) {
                return attacker;
            }
        }

        opponent_bb.unset(attacker_pos);
    }

    for bishop in [false, true] {
        let mut attacker_cands = if bishop {
            position.bishopish()
        } else {
            position.rookish()
        } & opponent_bb;

        if attacker_cands.is_empty() {
            continue;
        }
        attacker_cands &= if bishop {
            bishop_reachable(position.occupied_bb(), king_pos)
        } else {
            rook_reachable(position.occupied_bb(), king_pos)
        };
        if attacker_cands.is_empty() {
            continue;
        }
        for attacker_pos in attacker_cands {
            if update_attacker(
                &mut attacker,
                attacker_pos,
                position.must_get_kind(attacker_pos),
                early_return,
            ) {
                return attacker;
            }
        }
    }

    attacker
}

fn update_attacker(
    attacker: &mut Option<Attacker>,
    pos: Square,
    kind: Kind,
    early_return: bool,
) -> bool {
    if let Some(a) = attacker {
        a.double_check = (pos, kind).into();
        true
    } else {
        *attacker = Some(Attacker::new(pos, kind, None));
        early_return
    }
}
