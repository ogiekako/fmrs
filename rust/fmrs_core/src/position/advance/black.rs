use crate::nohash::NoHashMap;
use crate::position::rule::{is_legal_drop, is_legal_move};
use anyhow::{bail, Result};

use crate::piece::{Color, Kind};

use crate::position::{
    bitboard::{self, BitBoard},
    Movement, Position, PositionExt, Square,
};

use super::attack_prevent::attack_preventing_movements;
use super::pinned::{pinned, Pinned};
use super::{common, AdvanceOptions};

pub(super) fn advance(
    position: &Position,
    memo: &mut NoHashMap<u32>,
    next_step: u32,
    options: &AdvanceOptions,
) -> anyhow::Result<Vec<Position>> {
    debug_assert_eq!(position.turn(), Color::BLACK);
    let mut ctx = Context::new(position, memo, next_step, options)?;
    ctx.advance()?;
    Ok(ctx.result)
}

pub(super) fn advance_old(position: &Position) -> anyhow::Result<Vec<Position>> {
    advance(
        position,
        &mut NoHashMap::default(),
        0,
        &AdvanceOptions::default(),
    )
}

struct Context<'a> {
    // Immutable fields
    position: &'a Position,
    next_step: u32,
    white_king_pos: Square,
    black_king_checked: bool,
    pinned: Pinned,
    pawn_mask: usize,
    options: &'a AdvanceOptions,

    // Mutable fields
    memo: &'a mut NoHashMap<u32>,
    result: Vec<Position>,
}

impl<'a> Context<'a> {
    fn new(
        position: &'a Position,
        memo: &'a mut NoHashMap<u32>,
        next_step: u32,
        options: &'a AdvanceOptions,
    ) -> anyhow::Result<Self> {
        let white_king_pos = if let Some(p) = position.bitboard(Color::WHITE, Kind::King).next() {
            p
        } else {
            bail!("No white king");
        };
        let black_king_checked = common::checked(position, Color::BLACK);

        let pinned = position
            .bitboard(Color::BLACK, Kind::King)
            .next()
            .map(|king_pos| pinned(position, Color::BLACK, king_pos, Color::BLACK))
            .unwrap_or_else(Pinned::empty);

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
            black_king_checked,
            pinned,
            pawn_mask,
            result: vec![],
            options,
        })
    }

    fn advance(&mut self) -> Result<()> {
        if self.black_king_checked {
            let black_king_pos = self
                .position
                .bitboard(Color::BLACK, Kind::King)
                .next()
                .unwrap();
            self.result = attack_preventing_movements(
                self.position,
                self.memo,
                self.next_step,
                black_king_pos,
                true,
                self.options,
            )?
            .0;
            return Ok(());
        }

        self.drops()?;
        self.direct_attack_moves()?;
        self.discovered_attack_moves()?;

        Ok(())
    }

    fn drops(&mut self) -> Result<()> {
        for kind in self.position.hands().kinds(Color::BLACK) {
            let check_needed = matches!(kind, Kind::Pawn | Kind::Lance | Kind::Knight);

            let empty_attack_squares = self
                .attack_squares(kind)
                .and_not(self.position.color_bb().bitboard(Color::WHITE));
            for pos in empty_attack_squares {
                if check_needed && !is_legal_drop(Color::BLACK, pos, kind, self.pawn_mask) {
                    continue;
                }

                self.maybe_add_move(&Movement::Drop(pos, kind), kind)?;
            }
        }
        Ok(())
    }

    fn direct_attack_moves(&mut self) -> Result<()> {
        self.non_leap_piece_direct_attack()?;
        self.leap_piece_direct_attack()?;

        Ok(())
    }

    // #[inline(never)]
    fn non_leap_piece_direct_attack(&mut self) -> Result<()> {
        let lion_king_range = lion_king_power(self.white_king_pos);
        // Non line or leap pieces
        for attacker_pos in lion_king_range & self.position.color_bb().bitboard(Color::BLACK) {
            let attacker_source_kind = self.position.get(attacker_pos).unwrap().1;
            if attacker_source_kind == Kind::King
                || attacker_source_kind == Kind::Knight
                || attacker_source_kind.is_line_piece()
            {
                continue;
            }
            let attacker_power = if self.pinned.is_pinned(attacker_pos) {
                self.pinned.pinned_area(attacker_pos)
            } else {
                bitboard::power(Color::BLACK, attacker_pos, attacker_source_kind)
            };
            for promote in [false, true] {
                if promote && attacker_source_kind.promote().is_none() {
                    continue;
                }

                let attacker_dest_kind = if promote {
                    attacker_source_kind.promote().unwrap()
                } else {
                    attacker_source_kind
                };
                let attack_squares = self.attack_squares(attacker_dest_kind);
                for dest in attacker_power & attack_squares {
                    if !is_legal_move(
                        Color::BLACK,
                        attacker_pos,
                        dest,
                        attacker_source_kind,
                        promote,
                    ) {
                        continue;
                    }

                    let capture_kind = self.position.get_kind(dest);
                    self.maybe_add_move(
                        &Movement::move_with_hint(
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

            for promote in [false, true] {
                let attacker_dest_kind = if promote {
                    let Some(k) = attacker_source_kind.promote() else {
                        continue;
                    };
                    k
                } else {
                    attacker_source_kind
                };

                let attack_squares = self.attack_squares(attacker_dest_kind);

                for attacker_pos in attackers {
                    let attacker_reachable = if self.pinned.is_pinned(attacker_pos) {
                        self.pinned.pinned_area(attacker_pos)
                    } else {
                        bitboard::reachable(
                            self.position.color_bb(),
                            Color::BLACK,
                            attacker_pos,
                            attacker_source_kind,
                            false,
                        )
                    };

                    for dest in attacker_reachable & attack_squares {
                        if !is_legal_move(
                            Color::BLACK,
                            attacker_pos,
                            dest,
                            attacker_source_kind,
                            promote,
                        ) {
                            continue;
                        }

                        let capture_kind = self.position.get_kind(dest);
                        self.maybe_add_move(
                            &Movement::move_with_hint(
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
            self.position,
            Color::WHITE,
            self.white_king_pos,
            Color::BLACK,
        );
        for (blocker_pos, blocker_pinned_area) in blockers.iter() {
            let (blocker_pos, blocker_pinned_area) = (*blocker_pos, *blocker_pinned_area);
            let blocker_kind = self.position.get(blocker_pos).unwrap().1;
            let mut blocker_dest_cands = bitboard::reachable(
                self.position.color_bb(),
                Color::BLACK,
                blocker_pos,
                blocker_kind,
                false,
            )
            .and_not(blocker_pinned_area);
            if self.pinned.is_pinned(blocker_pos) {
                blocker_dest_cands &= self.pinned.pinned_area(blocker_pos);
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
                        &Movement::move_with_hint(
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
    fn maybe_add_move(&mut self, movement: &Movement, kind: Kind) -> Result<()> {
        let mut next_position = self.position.clone();
        next_position.do_move(movement);

        if kind == Kind::King && common::checked(&next_position, Color::BLACK) {
            return Ok(());
        }

        debug_assert!(
            !common::checked(&next_position, Color::BLACK),
            "Black king checked: {:?}",
            next_position
        );

        let digest = next_position.digest();
        if self.memo.contains_key(&digest) {
            return Ok(());
        }

        self.options.check_allowed_branches(self.result.len() + 1)?;

        self.memo.insert(digest, self.next_step);
        self.result.push(next_position);

        Ok(())
    }

    // Squares moving to which produces a check.
    fn attack_squares(&self, kind: Kind) -> BitBoard {
        bitboard::reachable(
            self.position.color_bb(),
            Color::WHITE,
            self.white_king_pos,
            kind,
            true,
        )
    }
}

fn lion_king_power(pos: Square) -> BitBoard {
    let mut res = bitboard::power(Color::BLACK, pos, Kind::King);
    for i in [-1, 1] {
        for j in [-1, 1] {
            let col = pos.col() as isize + i;
            let row = pos.row() as isize + j;
            if (0..9).contains(&col) && (0..9).contains(&row) {
                res |= bitboard::power(
                    Color::BLACK,
                    Square::new(col as usize, row as usize),
                    Kind::King,
                );
            }
        }
    }
    res
}
