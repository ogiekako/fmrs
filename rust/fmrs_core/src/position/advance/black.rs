use crate::memo::MemoTrait;
use crate::position::bitboard::reachable_cont;
use crate::position::bitboard::{king_power, lion_king_power, power, reachable_cont_sub};
use crate::position::controller::PositionController;
use crate::position::rule::is_legal_move;
use anyhow::Result;

use crate::piece::{Color, Kind, Kindish};

use crate::position::{bitboard::BitBoard, Movement};

use super::attack_prevent::{attack_preventing_movements, attacker, Attacker};
use super::pinned::{pinned, Pinned};
use super::{common, AdvanceOptions};

pub(super) fn advance<M: MemoTrait>(
    controller: &mut PositionController,
    memo: &mut M,
    next_step: u16,
    options: &AdvanceOptions,
    res: &mut Vec<Movement>,
) -> anyhow::Result<()> {
    debug_assert_eq!(controller.turn(), Color::BLACK);
    let mut ctx = Context::new(controller, memo, next_step, options, res)?;
    ctx.advance()?;
    Ok(())
}

struct Context<'a, M: MemoTrait> {
    // Immutable fields
    controller: &'a mut PositionController,
    next_step: u16,
    attacker: Option<Attacker>,
    black_king_pinned: Pinned,
    white_king_pinned: Pinned,
    pawn_mask: usize,
    options: &'a AdvanceOptions,
    orig_result_len: usize,

    // Mutable fields
    memo: &'a mut M,
    result: &'a mut Vec<Movement>,
    num_branches_without_pawn_drop: usize,
}

impl<'a, M: MemoTrait> Context<'a, M> {
    fn new(
        controller: &'a mut PositionController,
        memo: &'a mut M,
        next_step: u16,
        options: &'a AdvanceOptions,
        result: &'a mut Vec<Movement>,
    ) -> anyhow::Result<Self> {
        let attacker = controller
            .black_king_pos()
            .is_some()
            .then(|| attacker(controller, Color::BLACK, false))
            .flatten();
        let black_king_pinned = controller
            .black_king_pos()
            .is_some()
            .then(|| pinned(controller, Color::BLACK))
            .unwrap_or_else(Pinned::default);
        let white_king_pinned = pinned(controller, Color::WHITE);

        let pawn_mask = {
            let mut mask = Default::default();
            for pos in controller.bitboard(Color::BLACK, Kind::Pawn) {
                mask |= 1 << pos.col()
            }
            mask
        };

        Ok(Self {
            controller,
            next_step,
            attacker,
            black_king_pinned,
            white_king_pinned,
            pawn_mask,
            options,
            orig_result_len: result.len(),

            memo,
            result,
            num_branches_without_pawn_drop: 0,
        })
    }

    fn advance(&mut self) -> Result<()> {
        if let Some(attacker) = &self.attacker {
            attack_preventing_movements(
                self.controller,
                self.memo,
                self.next_step,
                true,
                self.options,
                attacker.clone().into(),
                self.result,
            )?;
            return Ok(());
        }

        self.drops()?;
        self.direct_attack_moves()?;
        self.discovered_attack_moves()?;

        Ok(())
    }

    #[inline(never)]
    fn drops(&mut self) -> Result<()> {
        let white_king_pos = self.controller.white_king_pos();
        for kind in self.controller.hands().kinds(Color::BLACK) {
            if kind == Kind::Pawn && self.pawn_mask >> white_king_pos.col() & 1 != 0 {
                continue;
            }

            let empty_attack_squares = self
                .controller
                .white_king_attack_empty_squares(kind.effect());

            for pos in empty_attack_squares {
                self.maybe_add_move(Movement::Drop(pos, kind), kind)?;
            }
        }
        Ok(())
    }

    #[inline(never)]
    fn direct_attack_moves(&mut self) -> Result<()> {
        self.non_leap_piece_direct_attack()?;
        self.leap_piece_direct_attack()?;

        Ok(())
    }

    #[inline(never)]
    fn non_leap_piece_direct_attack(&mut self) -> Result<()> {
        let lion_king_range = lion_king_power(self.controller.white_king_pos());
        let king_range = king_power(self.controller.white_king_pos())
            .and_not(self.controller.color_bb_and_stone(Color::BLACK));

        let attacker_cands =
            self.controller.pawn_silver_goldish() & lion_king_range & self.controller.black_bb();

        for attacker_pos in attacker_cands {
            let attacker_source_kind = self.controller.must_get_kind(attacker_pos);

            let mut attacker_range =
                power(Color::BLACK, attacker_pos, attacker_source_kind) & king_range;
            if let Some(pinned_area) = self.black_king_pinned.pinned_area(attacker_pos) {
                attacker_range &= pinned_area;
            }
            if let Some(pinned_area) = self.white_king_pinned.pinned_area(attacker_pos) {
                attacker_range &= pinned_area;
            }
            if attacker_range.is_empty() {
                continue;
            }

            for promote in [false, true] {
                if promote && attacker_source_kind.promote().is_none() {
                    continue;
                }

                let attacker_dest_kind = if promote {
                    attacker_source_kind.promote().unwrap()
                } else {
                    attacker_source_kind
                };

                let mut attack_squares = power(
                    Color::WHITE,
                    self.controller.white_king_pos(),
                    attacker_dest_kind,
                );
                if promote && !BitBoard::BLACK_PROMOTABLE.contains(attacker_pos) {
                    attack_squares &= BitBoard::BLACK_PROMOTABLE;
                }

                for dest in attacker_range & attack_squares {
                    let capture_kind = self.controller.get_kind(dest);
                    self.maybe_add_move(
                        Movement::move_with_hint(
                            attacker_pos,
                            attacker_source_kind,
                            dest,
                            promote,
                            capture_kind,
                        ),
                        attacker_source_kind,
                    )?;
                }
            }
        }
        Ok(())
    }

    #[inline(never)]
    fn leap_piece_direct_attack(&mut self) -> Result<()> {
        for kindish in [
            Kindish::Pawn,
            Kindish::Lance,
            Kindish::Knight,
            Kindish::Bishop,
            Kindish::Rook,
        ] {
            let attackable = self.controller.attackable(kindish);
            for source in attackable {
                let kind = self.controller.must_get_kind(source);
                debug_assert_eq!(kind.ish(), kindish);

                let mut dests = reachable_cont_sub(self.controller, Color::BLACK, source, kind);
                if let Some(pinned_area) = self.black_king_pinned.pinned_area(source) {
                    dests &= pinned_area;
                    if dests.is_empty() {
                        continue;
                    }
                }
                if let Some(pinned_area) = self.white_king_pinned.pinned_area(source) {
                    dests &= pinned_area;
                    if dests.is_empty() {
                        continue;
                    }
                }

                let raw_dests = dests
                    & self
                        .controller
                        .white_king_empty_or_white_attack_squares(kind.effect());
                let pro_dests = kind.promote().map(|k| {
                    let res = dests
                        & self
                            .controller
                            .white_king_empty_or_white_attack_squares(k.effect());
                    if BitBoard::BLACK_PROMOTABLE.contains(source) {
                        res
                    } else {
                        res & BitBoard::BLACK_PROMOTABLE
                    }
                });

                for dest in raw_dests {
                    let capture_kind = self.controller.get_kind(dest);
                    self.maybe_add_move(
                        Movement::move_with_hint(source, kind, dest, false, capture_kind),
                        kind,
                    )?;
                }
                if let Some(pro_dests) = pro_dests {
                    for dest in pro_dests {
                        let capture_kind = self.controller.get_kind(dest);
                        self.maybe_add_move(
                            Movement::move_with_hint(source, kind, dest, true, capture_kind),
                            kind,
                        )?;
                    }
                }
            }
        }
        Ok(())
    }

    #[inline(never)]
    fn discovered_attack_moves(&mut self) -> Result<()> {
        for blocker_pinned_area in self
            .white_king_pinned
            .exclusive_pinned_areas()
            .collect::<Vec<_>>()
        {
            let Some(blocker_pos) = (blocker_pinned_area & self.controller.black_bb()).next()
            else {
                continue;
            };
            let blocker_kind = self.controller.must_get_kind(blocker_pos);

            let mut blocker_dest_cands = reachable_cont(
                self.controller,
                Color::BLACK,
                blocker_pos,
                blocker_kind,
                false,
            )
            .and_not(blocker_pinned_area);

            if let Some(area) = self.black_king_pinned.pinned_area(blocker_pos) {
                blocker_dest_cands &= area;
            }

            let maybe_promotable = blocker_kind.can_promote();
            for blocker_dest in blocker_dest_cands {
                for promote in [false, true] {
                    if promote && !maybe_promotable {
                        continue;
                    }
                    if !is_legal_move(
                        Color::BLACK,
                        blocker_pos,
                        blocker_dest,
                        blocker_kind,
                        promote,
                    ) {
                        continue;
                    }

                    let capture_kind = self.controller.get_kind(blocker_dest);
                    self.maybe_add_move(
                        Movement::move_with_hint(
                            blocker_pos,
                            blocker_kind,
                            blocker_dest,
                            promote,
                            capture_kind,
                        ),
                        blocker_kind,
                    )?;
                }
            }
        }
        Ok(())
    }
}

// Helper
impl<M: MemoTrait> Context<'_, M> {
    fn maybe_add_move(&mut self, movement: Movement, kind: Kind) -> Result<()> {
        debug_assert!(
            !self.result[self.orig_result_len..].contains(&movement),
            "{:?} {:?}",
            self.controller,
            movement
        );

        if kind == Kind::King {
            self.controller.push();
            self.controller.do_move(&movement);
            let checked = common::checked(self.controller, Color::BLACK);
            self.controller.pop();
            if checked {
                return Ok(());
            }
        }

        debug_assert!(
            {
                self.controller.push();
                self.controller.do_move(&movement);
                let res = !common::checked(self.controller, Color::BLACK);
                self.controller.pop();
                res
            },
            "Black king checked: {}",
            {
                self.controller.do_move(&movement);
                let attacker = attacker(self.controller, Color::BLACK, false);
                format!("{:?} {:?} {:?}", self.controller, movement, attacker)
            }
        );

        if !movement.is_pawn_drop() {
            self.num_branches_without_pawn_drop += 1;
            self.options
                .check_allowed_branches(self.num_branches_without_pawn_drop)?;
        }

        if !self.options.no_memo {
            let digest = self.controller.moved_digest(&movement);

            if self.options.no_insertion {
                if self.memo.contains_key(&digest) {
                    return Ok(());
                }
            } else if self.memo.contains_or_insert(digest, self.next_step) {
                return Ok(());
            }
        }

        self.result.push(movement);

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        memo::MemoStub,
        position::{
            controller::PositionController, position::PositionAux, AdvanceOptions, Movement, Square,
        },
    };

    #[test]
    fn test_black_advance() {
        for (sfen, mut want) in [
            (
                "4l1+P2/3+P1n3/S3p1+L2/1SG1kp2G/2SL4S/1N1p1l1B1/B4NR2/3g1K1p1/PNP1P3P b Prg7p 1",
                vec![Movement::move_without_hint(Square::S74, Square::S64, false)],
            ),
            (
                "9/G2s4G/LLpNGNpPP/4L4/1sN2kN2/1g1bpb1ss/1pL3P2/P8/1PPPPP2K b 2r5p 1",
                vec![],
            ),
            (
                "1k1G5/g4g1P1/1b1K1+L3/1S4+s2/p2l+p2p1/2N2p2P/5R3/7P1/6B1r b g2s3n2l11p 1",
                vec![Movement::move_without_hint(Square::S61, Square::S71, false)],
            ),
            (
                "6R2/4k4/3L5/4B4/9/9/9/9/9 b rb4g4s4n3l18p 1",
                vec![
                    Movement::move_without_hint(Square::S31, Square::S32, false),
                    Movement::move_without_hint(Square::S31, Square::S32, true),
                    Movement::move_without_hint(Square::S31, Square::S41, true),
                    Movement::move_without_hint(Square::S31, Square::S51, false),
                    Movement::move_without_hint(Square::S31, Square::S51, true),
                    Movement::move_without_hint(Square::S31, Square::S61, true),
                    Movement::move_without_hint(Square::S54, Square::S43, false),
                    Movement::move_without_hint(Square::S54, Square::S43, true),
                    Movement::move_without_hint(Square::S63, Square::S62, true),
                ],
            ),
            (
                "5k3/9/9/9/9/9/9/9/4R1+R2 b 2b4g4s4n4l18p 1",
                vec![
                    Movement::move_without_hint(Square::S39, Square::S31, false),
                    Movement::move_without_hint(Square::S39, Square::S32, false),
                    Movement::move_without_hint(Square::S39, Square::S48, false),
                    Movement::move_without_hint(Square::S39, Square::S49, false),
                    Movement::move_without_hint(Square::S59, Square::S51, false),
                    Movement::move_without_hint(Square::S59, Square::S51, true),
                    Movement::move_without_hint(Square::S59, Square::S52, true),
                    Movement::move_without_hint(Square::S59, Square::S49, false),
                ],
            ),
            (
                "5kO2/5O3/9/9/9/9/9/9/6+R2 b r2b4g4s4n4l18p 1",
                vec![Movement::move_without_hint(Square::S39, Square::S32, false)],
            ),
        ] {
            let position = PositionAux::from_sfen(sfen).unwrap();
            let mut controller =
                PositionController::new(position.core().clone(), *position.stone());
            let mut res = vec![];
            super::advance(
                &mut controller,
                &mut MemoStub,
                1,
                &AdvanceOptions {
                    no_memo: true,
                    ..Default::default()
                },
                &mut res,
            )
            .unwrap();

            res.sort();
            want.sort();

            assert_eq!(res, want);
        }
    }
}
