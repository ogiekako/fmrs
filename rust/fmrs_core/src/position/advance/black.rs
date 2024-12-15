use crate::nohash::NoHashMap;
use crate::position::bitboard::{gold_power, king_power, knight_power, power};
use crate::position::rule::{is_legal_drop, is_legal_move};
use anyhow::{bail, Result};

use crate::piece::{Color, Kind};

use crate::position::{
    bitboard::{self, BitBoard},
    Movement, Position, PositionExt, Square,
};

use super::attack_prevent::{attack_preventing_movements, attacker, Attacker};
use super::pinned::{pinned, Pinned};
use super::{common, AdvanceOptions};

pub(super) fn advance(
    position: &mut Position,
    memo: &mut NoHashMap<u32>,
    next_step: u32,
    options: &AdvanceOptions,
    res: &mut Vec<Movement>,
) -> anyhow::Result<()> {
    debug_assert_eq!(position.turn(), Color::BLACK);
    let mut ctx = Context::new(position, memo, next_step, options, res)?;
    ctx.advance()?;
    Ok(())
}

pub(super) fn advance_old(
    position: &mut Position,
    result: &mut Vec<Movement>,
) -> anyhow::Result<()> {
    advance(
        position,
        &mut NoHashMap::default(),
        0,
        &AdvanceOptions::default(),
        result,
    )
}

struct Context<'a> {
    // Immutable fields
    position: &'a mut Position,
    next_step: u32,
    white_king_pos: Square,
    black_king_pos: Option<Square>,
    attacker: Option<Attacker>,
    pinned: Pinned,
    pawn_mask: usize,
    options: &'a AdvanceOptions,

    // Mutable fields
    memo: &'a mut NoHashMap<u32>,
    result: &'a mut Vec<Movement>,
    num_branches: usize,
}

impl<'a> Context<'a> {
    fn new(
        position: &'a mut Position,
        memo: &'a mut NoHashMap<u32>,
        next_step: u32,
        options: &'a AdvanceOptions,
        result: &'a mut Vec<Movement>,
    ) -> anyhow::Result<Self> {
        let kings = position.kind_bb().bitboard(Kind::King);
        let white_king_pos = if let Some(p) = (kings & position.color_bb().white()).next() {
            p
        } else {
            bail!("No white king");
        };
        let black_king_pos = (kings & position.color_bb().black()).next();
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
            num_branches: 0,
            options,
        })
    }

    fn advance(&mut self) -> Result<()> {
        if let Some(attacker) = self.attacker.clone() {
            attack_preventing_movements(
                self.position,
                self.memo,
                self.next_step,
                self.black_king_pos.unwrap(),
                true,
                self.options,
                attacker.into(),
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
            let check_needed = matches!(kind, Kind::Pawn | Kind::Lance | Kind::Knight);

            let empty_attack_squares = self
                .attack_squares(kind)
                .and_not(self.position.color_bb().bitboard(Color::WHITE));
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
        let king_range = king_power(self.white_king_pos).and_not(self.position.color_bb().black());

        let attacker_cands = self.position.kind_bb().pawn_silver_goldish()
            & lion_king_range
            & self.position.color_bb().black();

        for attacker_pos in attacker_cands {
            let attacker_source_kind = self.position.must_get_kind(attacker_pos);

            let attacker_range = if self.pinned.is_pinned(attacker_pos) {
                self.pinned.pinned_area(attacker_pos)
            } else {
                bitboard::power(Color::BLACK, attacker_pos, attacker_source_kind)
            } & king_range;
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

                let attack_squares = power(Color::WHITE, self.white_king_pos, attacker_dest_kind);

                for dest in attacker_range & attack_squares {
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
        // Knight
        for promote in [true, false] {
            let reach_from_king = if promote {
                gold_power(Color::WHITE, self.white_king_pos) & BitBoard::BLACK_PROMOTABLE
            } else {
                knight_power(Color::WHITE, self.white_king_pos)
            }
            .and_not(self.position.color_bb().black());
            if reach_from_king.is_empty() {
                continue;
            }

            let reachable_squares = reach_from_king & self.position.black_knight_reach();

            for dest in reachable_squares {
                let capture_kind = self.position.get_kind(dest);
                let sources = self.position.bitboard(Color::BLACK, Kind::Knight)
                    & knight_power(Color::WHITE, dest);
                for source in sources {
                    if self.pinned.is_pinned(source) {
                        continue;
                    }
                    self.maybe_add_move(
                        Movement::move_with_hint(source, Kind::Knight, dest, promote, capture_kind),
                        Kind::Knight,
                    )?;
                }
            }
        }

        for attacker_source_kind in [
            Kind::Lance,
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
            self.position,
            Color::WHITE,
            self.white_king_pos,
            Color::BLACK,
        );
        for &(blocker_pos, blocker_pinned_area) in blockers.iter() {
            let blocker_kind = self.position.must_get_kind(blocker_pos);

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
    fn maybe_add_move(&mut self, movement: Movement, kind: Kind) -> Result<()> {
        let undo = self.position.do_move(&movement);

        if kind == Kind::King
            && common::checked(self.position, Color::BLACK, movement.dest().into())
        {
            self.position.undo_move(&undo);
            return Ok(());
        }

        debug_assert!(
            !common::checked(&self.position, Color::BLACK, self.black_king_pos),
            "Black king checked: {:?}",
            self.position
        );

        self.num_branches += 1;
        self.options.check_allowed_branches(self.num_branches)?;

        if !self.options.no_memo {
            let digest = self.position.digest();
            if self.memo.contains_key(&digest) {
                self.position.undo_move(&undo);
                return Ok(());
            }
            self.memo.insert(digest, self.next_step);
        }

        self.result.push(movement);

        self.position.undo_move(&undo);

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
