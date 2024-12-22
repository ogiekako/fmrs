use crate::memo::Memo;
use crate::position::bitboard::{king_power, lion_king_power, power};
use crate::position::position::PositionAux;
use crate::position::rule::{is_legal_drop, is_legal_move};
use anyhow::{bail, Result};

use crate::piece::{Color, Kind};

use crate::position::{
    bitboard::{self, BitBoard},
    Movement, Position, Square,
};

use super::attack_prevent::{attack_preventing_movements, attacker, Attacker};
use super::pinned::{pinned, Pinned};
use super::{common, AdvanceOptions};

pub(super) fn advance<'a>(
    position: &'a mut PositionAux,
    memo: &mut Memo,
    next_step: u32,
    options: &AdvanceOptions,
    res: &mut Vec<Movement>,
) -> anyhow::Result<()> {
    debug_assert_eq!(position.turn(), Color::BLACK);
    let mut ctx = Context::new(position, memo, next_step, options, res)?;
    ctx.advance()?;
    Ok(())
}

struct Context<'a> {
    // Immutable fields
    position: &'a mut PositionAux,
    next_step: u32,
    white_king_pos: Square,
    black_king_pos: Option<Square>,
    attacker: Option<Attacker>,
    pinned: Pinned,
    pawn_mask: usize,
    options: &'a AdvanceOptions,

    // Mutable fields
    memo: &'a mut Memo,
    result: &'a mut Vec<Movement>,
    num_branches_without_pawn_drop: usize,
}

impl<'a> Context<'a> {
    fn new(
        position: &'a mut PositionAux,
        memo: &'a mut Memo,
        next_step: u32,
        options: &'a AdvanceOptions,
        result: &'a mut Vec<Movement>,
    ) -> anyhow::Result<Self> {
        let kings = position.kind_bb(Kind::King);

        let white_king_pos = if let Some(p) = (kings & position.white_bb()).next() {
            p
        } else {
            bail!("No white king");
        };
        let black_king_pos = (kings & position.black_bb()).next();
        let attacker = black_king_pos.and_then(|pos| attacker(position, Color::BLACK, pos, false));

        let pinned = black_king_pos
            .map(|pos| pinned(position, Color::BLACK, pos, Color::BLACK))
            .unwrap_or_else(|| Pinned::empty());

        let pawn_mask = {
            let mut mask = Default::default();
            for pos in position.bitboard(Color::BLACK, Kind::Pawn) {
                mask |= 1 << pos.col()
            }
            mask
        };

        Ok(Self {
            position,
            memo,
            next_step,
            white_king_pos,
            black_king_pos,
            attacker,
            pinned,
            pawn_mask,
            result,
            num_branches_without_pawn_drop: 0,
            options,
        })
    }

    fn advance(&mut self) -> Result<()> {
        if let Some(attacker) = &self.attacker {
            attack_preventing_movements(
                &mut self.position,
                self.memo,
                self.next_step,
                self.black_king_pos.unwrap(),
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

    // #[inline(never)]
    fn drops(&mut self) -> Result<()> {
        for kind in self.position.hands().kinds(Color::BLACK) {
            let check_needed = matches!(kind, Kind::Pawn);

            let empty_attack_squares = self
                .attack_squares(kind)
                .and_not(self.position.color_bb(Color::WHITE));
            for pos in empty_attack_squares {
                if check_needed && !is_legal_drop(Color::BLACK, pos, kind, self.pawn_mask) {
                    continue;
                }

                self.maybe_add_move(Movement::Drop(pos, kind), kind)?;
            }
        }
        Ok(())
    }

    // #[inline(never)]
    fn direct_attack_moves(&mut self) -> Result<()> {
        self.non_leap_piece_direct_attack()?;
        self.leap_piece_direct_attack()?;

        Ok(())
    }

    // #[inline(never)]
    fn non_leap_piece_direct_attack(&mut self) -> Result<()> {
        let lion_king_range = lion_king_power(self.white_king_pos);
        let king_range = king_power(self.white_king_pos).and_not(self.position.black_bb());

        let attacker_cands =
            self.position.pawn_silver_goldish() & lion_king_range & self.position.black_bb();

        for attacker_pos in attacker_cands {
            let attacker_source_kind = self.position.must_get_kind(attacker_pos);

            let attacker_range = self.pinned.pinned_area(attacker_pos).unwrap_or_else(|| {
                bitboard::power(Color::BLACK, attacker_pos, attacker_source_kind)
            }) & king_range;
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

                let mut attack_squares =
                    power(Color::WHITE, self.white_king_pos, attacker_dest_kind);
                if promote && !BitBoard::BLACK_PROMOTABLE.get(attacker_pos) {
                    attack_squares &= BitBoard::BLACK_PROMOTABLE;
                }

                for dest in attacker_range & attack_squares {
                    let capture_kind = self.position.get_kind(dest);
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

    // #[inline(never)]
    fn leap_piece_direct_attack(&mut self) -> Result<()> {
        for attacker_source_kind in [
            Kind::Lance,
            Kind::Knight,
            Kind::Bishop,
            Kind::Rook,
            Kind::ProBishop,
            Kind::ProRook,
        ] {
            let attackers = self.position.bitboard(Color::BLACK, attacker_source_kind);
            if attackers.is_empty() {
                continue;
            }

            for attacker_pos in attackers {
                let attacker_reachable =
                    self.pinned.pinned_area(attacker_pos).unwrap_or_else(|| {
                        bitboard::reachable(
                            &mut self.position,
                            Color::BLACK,
                            attacker_pos,
                            attacker_source_kind,
                            false,
                        )
                    });

                for promote in [false, true] {
                    let attacker_dest_kind = if promote {
                        let Some(k) = attacker_source_kind.promote() else {
                            continue;
                        };
                        k
                    } else {
                        attacker_source_kind
                    };

                    let mut attack_squares = self.attack_squares(attacker_dest_kind);
                    if promote && !BitBoard::BLACK_PROMOTABLE.get(attacker_pos) {
                        attack_squares &= BitBoard::BLACK_PROMOTABLE;
                    }

                    for dest in attacker_reachable & attack_squares {
                        let capture_kind = self.position.get_kind(dest);
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
        }
        Ok(())
    }

    // #[inline(never)]
    fn discovered_attack_moves(&mut self) -> Result<()> {
        let blockers = pinned(
            &mut self.position,
            Color::WHITE,
            self.white_king_pos,
            Color::BLACK,
        );
        for &(blocker_pos, blocker_pinned_area) in blockers.iter() {
            let blocker_kind = self.position.must_get_kind(blocker_pos);

            let mut blocker_dest_cands = bitboard::reachable(
                &mut self.position,
                Color::BLACK,
                blocker_pos,
                blocker_kind,
                false,
            )
            .and_not(blocker_pinned_area);

            if let Some(area) = self.pinned.pinned_area(blocker_pos) {
                blocker_dest_cands &= area;
            }

            let maybe_promotable = blocker_kind.is_promotable();
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

                    let capture_kind = self.position.get_kind(blocker_dest);
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
impl<'a> Context<'a> {
    fn update<'b>(
        &self,
        new_position: &'b mut Option<Position>,
        movement: &'b Movement,
    ) -> &'b Position {
        new_position.get_or_insert_with(|| self.position.moved(movement))
    }

    fn maybe_add_move(&mut self, movement: Movement, kind: Kind) -> Result<()> {
        let mut new_position = None;

        if kind == Kind::King
            && common::checked(
                &mut PositionAux::new(self.update(&mut new_position, &movement).clone()),
                Color::BLACK,
                movement.dest().into(),
            )
        {
            return Ok(());
        }

        debug_assert!(
            !common::checked(
                &mut PositionAux::new(self.update(&mut new_position, &movement).clone()),
                Color::BLACK,
                self.black_king_pos,
            ),
            "Black king checked: {:?}",
            new_position.as_ref().unwrap()
        );

        if !movement.is_pawn_drop() {
            self.num_branches_without_pawn_drop += 1;
            self.options
                .check_allowed_branches(self.num_branches_without_pawn_drop)?;
        }

        if !self.options.no_memo {
            let digest = self.update(&mut new_position, &movement).digest();

            let mut contains = true;
            self.memo.entry(digest).or_insert_with(|| {
                contains = false;
                self.next_step
            });

            if contains {
                return Ok(());
            }
        }

        self.result.push(movement);

        Ok(())
    }

    // Squares moving to which produces a check.
    fn attack_squares(&mut self, kind: Kind) -> BitBoard {
        bitboard::reachable(
            &mut self.position,
            Color::WHITE,
            self.white_king_pos,
            kind,
            true,
        )
    }
}
