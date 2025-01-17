use crate::{
    memo::MemoTrait,
    piece::KindEffect,
    position::{
        bitboard::{
            king_then_king_or_night_power, lance_reachable,
            magic::{bishop_reachable, rook_reachable},
            reachable_cont,
        },
        checked,
        controller::PositionController,
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
    controller: &'a mut PositionController,
    memo: &'a mut M,
    next_step: u16,
    should_return_check: bool,
    options: &'a AdvanceOptions,
    attacker_hint: Option<Attacker>,
    result: &'a mut Vec<Movement>,
) -> Result</* is legal mate */ bool> {
    let mut ctx = Context::new(
        controller,
        memo,
        next_step,
        should_return_check,
        options,
        attacker_hint,
        result,
    )?;
    ctx.advance()?;
    Ok(ctx.is_mate && !controller.pawn_drop())
}

struct Context<'a, M: MemoTrait> {
    controller: &'a mut PositionController,
    occupied_without_king: BitBoard,
    pinned: Pinned,
    attacker: Attacker,
    next_step: u16,
    should_return_check: bool,
    orig_result_len: usize,
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
        controller: &'a mut PositionController,
        memo: &'a mut M,
        next_step: u16,
        should_return_check: bool,
        options: &'a AdvanceOptions,
        attacker_hint: Option<Attacker>,
        result: &'a mut Vec<Movement>,
    ) -> anyhow::Result<Self> {
        let turn = controller.turn();
        let attacker = match attacker_hint {
            Some(attacker) => attacker,
            None => attacker(controller, turn, false)
                .ok_or_else(|| anyhow::anyhow!("No attacker found"))?,
        };
        let pinned = pinned(controller, turn);

        let mut occupied_without_king = controller.occupied_bb();
        occupied_without_king.unset(controller.must_king_pos(turn));

        Ok(Self {
            controller,
            occupied_without_king,
            pinned,
            attacker,
            next_step,
            should_return_check,
            orig_result_len: result.len(),
            memo,
            result,
            is_mate: true,
            num_branches_without_pawn_drop: 0,
            options,
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
        if attacker_kind.is_slider() {
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
        let king_color = self.controller.turn();
        let attacker_color = king_color.opposite();
        let attacker_color_bb = self.controller.capturable_by(king_color);

        let mut king_reachable = king_power(self.controller.must_turn_king_pos())
            .and_not(self.controller.color_bb_and_stone(king_color));
        if king_reachable.is_empty() {
            return Ok(());
        }
        let mut seen_cands = BitBoard::default();
        let non_line_cands =
            king_then_king_or_night_power(king_color, self.controller.must_turn_king_pos())
                & attacker_color_bb;
        for attacker_pos in non_line_cands {
            let attacker_kind = self.controller.must_get_kind(attacker_pos);
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

        let lances = self.controller.bitboard(attacker_color, Kind::Lance);
        let bishipish = self.controller.bishopish() & attacker_color_bb;
        let rookish = self.controller.rookish() & attacker_color_bb;

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

            let capture_kind = self.controller.get_kind(dest);
            let king_pos = self.controller.must_turn_king_pos();
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
            for kind in self.controller.hands().kinds(self.controller.turn()) {
                let pawn_mask = (kind == Kind::Pawn)
                    .then(|| self.controller.pawn_mask(self.controller.turn()))
                    .unwrap_or(0);
                if is_legal_drop(self.controller.turn(), dest, kind, pawn_mask) {
                    self.maybe_add_move(Movement::Drop(dest, kind), kind)?;
                }
            }
        }

        let capture_kind = if include_drop {
            None
        } else {
            self.controller.get_kind(dest)
        };

        // Move
        let around_dest = king_power(dest)
            & self
                .controller
                .capturable_by(self.controller.turn().opposite());
        for source_pos in around_dest {
            let source_kind = self.controller.must_get_kind(source_pos);
            if source_kind == Kind::King {
                continue;
            }
            let source_power = self.pinned.pinned_area(source_pos).unwrap_or_else(|| {
                bitboard::power(self.controller.turn(), source_pos, source_kind)
            });
            if source_power.contains(dest) {
                for promote in [false, true] {
                    if promote && source_kind.promote().is_none() {
                        continue;
                    }
                    if !is_legal_move(
                        self.controller.turn(),
                        source_pos,
                        dest,
                        source_kind,
                        promote,
                    ) {
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
                let raw_pieces = self.controller.bitboard(self.controller.turn(), leap_kind);
                let promoted_kind = leap_kind.promote().unwrap();
                if promoted_kind.is_slider() {
                    raw_pieces
                        | self
                            .controller
                            .bitboard(self.controller.turn(), promoted_kind)
                } else {
                    raw_pieces
                }
            }
            .and_not(around_dest);
            if on_board.is_empty() {
                continue;
            }
            let sources = reachable_cont(
                self.controller,
                self.controller.turn().opposite(),
                dest,
                leap_kind,
                false,
            ) & on_board;
            for source_pos in sources {
                if self.pinned.is_unpin_move(source_pos, dest) {
                    continue;
                }
                let source_kind = self.controller.must_get_kind(source_pos);
                for promote in [false, true] {
                    if promote && !source_kind.can_promote() {
                        continue;
                    }
                    if !is_legal_move(
                        self.controller.turn(),
                        source_pos,
                        dest,
                        source_kind,
                        promote,
                    ) {
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
impl<M: MemoTrait> Context<'_, M> {
    // #[inline(never)]
    fn is_return_check(&mut self, movement: &Movement) -> bool {
        self.controller.push();
        self.controller.do_move(movement);
        let res = checked(self.controller, self.controller.turn());
        self.controller.pop();
        res
    }

    fn maybe_add_move(&mut self, movement: Movement, kind: Kind) -> Result<()> {
        let is_king_move = kind == Kind::King;

        // TODO: check the second attacker
        if !is_king_move && self.attacker.double_check.is_some() {
            self.controller.push();
            self.controller.do_move(&movement);
            let checked = checked(self.controller, self.controller.turn().opposite());
            self.controller.pop();
            if checked {
                return Ok(());
            }
        }

        if self.should_return_check && !self.is_return_check(&movement) {
            return Ok(());
        }

        self.is_mate = false;
        if !movement.is_pawn_drop() {
            self.num_branches_without_pawn_drop += 1;
            self.options
                .check_allowed_branches(self.num_branches_without_pawn_drop)?;
        }

        debug_assert!(
            {
                self.controller.push();
                self.controller.do_move(&movement);
                let res = !common::checked(self.controller, self.controller.turn().opposite());
                self.controller.pop();
                res
            },
            "{:?} king checked: posision={:?} movement={:?}",
            self.controller.turn(),
            self.controller,
            movement,
        );

        if !self.options.no_memo {
            let digest = self.controller.moved_digest(&movement);

            if self.options.no_insertion {
                if self.memo.contains_key(&digest) {
                    return Ok(());
                }
            } else if self.memo.contains_or_insert(digest, self.next_step) {
                // Already seen during search on other branches.
                return Ok(());
            }
        }

        debug_assert!(
            !self.result[self.orig_result_len..].contains(&movement),
            "{:?}",
            movement
        );
        self.result.push(movement);

        Ok(())
    }

    fn blockable_squares(&mut self, attacker_pos: Square, attacker_kind: Kind) -> BitBoard {
        let king_pos = self.controller.must_turn_king_pos();
        if king_power(king_pos).contains(attacker_pos) {
            return BitBoard::default();
        }
        reachable_cont(
            self.controller,
            self.controller.turn(),
            king_pos,
            attacker_kind.maybe_unpromote(),
            false,
        ) & reachable_cont(
            self.controller,
            self.controller.turn().opposite(),
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
    controller: &mut PositionController,
    king_color: Color,
    early_return: bool,
) -> Option<Attacker> {
    let mut attacker: Option<Attacker> = None;

    let mut opponent_bb = controller.capturable_by(king_color);

    let king_power_area = (controller.king_attack_squares(king_color, KindEffect::King)
        | controller.king_attack_squares(king_color, KindEffect::Knight))
        & opponent_bb;

    for pos in king_power_area {
        let kind = controller.must_get_kind(pos);
        if controller
            .king_attack_squares(king_color, kind.effect())
            .contains(pos)
            && update_attacker(&mut attacker, pos, kind, early_return)
        {
            return attacker;
        }
    }
    opponent_bb = opponent_bb.and_not(king_power_area);

    // Lance
    let attacking_lances =
        controller.king_attack_squares(king_color, KindEffect::Lance) & opponent_bb;
    if !attacking_lances.is_empty() {
        let attacker_pos = attacking_lances.singleton();
        let kind = controller.must_get_kind(attacker_pos);
        if matches!(kind, Kind::Lance | Kind::Rook | Kind::ProRook)
            && update_attacker(&mut attacker, attacker_pos, kind, early_return)
        {
            return attacker;
        }

        opponent_bb.unset(attacker_pos);
    }

    for bishop in [false, true] {
        let mut attacker_cands = if bishop {
            controller.bishopish()
        } else {
            controller.rookish()
        } & opponent_bb;

        if attacker_cands.is_empty() {
            continue;
        }
        attacker_cands &= if bishop {
            controller.king_attack_squares(king_color, KindEffect::Bishop)
        } else {
            controller.king_attack_squares(king_color, KindEffect::Rook)
        };
        if attacker_cands.is_empty() {
            continue;
        }
        for attacker_pos in attacker_cands {
            if update_attacker(
                &mut attacker,
                attacker_pos,
                controller.must_get_kind(attacker_pos),
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
