use anyhow::bail;
use nohash_hasher::IntMap;

use crate::piece::{Color, Kind};

use crate::position::Digest;
use crate::position::{
    bitboard::{self, BitBoard}, Movement, Position, PositionExt, Square,
};

use super::attack_prevent::attack_preventing_movements;
use super::common;
use super::pinned::{pinned, Pinned};

pub(super) fn advance(
    position: &Position,
    memo: &mut IntMap<Digest, usize>,
    next_step: usize,
) -> anyhow::Result<Vec<Position>> {
    debug_assert_eq!(position.turn(), Color::Black);
    let mut ctx = Context::new(position, memo, next_step)?;
    ctx.advance();
    Ok(ctx.result)
}

pub(super) fn advance_old(position: &Position) -> anyhow::Result<Vec<Position>> {
    advance(position, &mut IntMap::default(), 0)
}

struct Context<'a> {
    position: &'a Position,
    next_step: usize,
    white_king_pos: Square,
    black_king_checked: bool,
    black_pieces: BitBoard,
    white_pieces: BitBoard,
    pinned: Pinned,
    pawn_mask: usize,
    // Mutable fields
    memo: &'a mut IntMap<Digest, usize>,
    result: Vec<Position>,
}

impl<'a> Context<'a> {
    fn new(
        position: &'a Position,
        memo: &'a mut IntMap<Digest, usize>,
        next_step: usize,
    ) -> anyhow::Result<Self> {
        let white_king_pos = if let Some(p) = position
            .bitboard(Color::White.into(), Kind::King.into())
            .next()
        {
            p
        } else {
            bail!("No white king");
        };
        let black_king_checked = common::checked(position, Color::Black);
        let black_pieces = position.bitboard(Color::Black.into(), None);
        let white_pieces = position.bitboard(Color::White.into(), None);

        let pinned = position
            .bitboard(Color::Black.into(), Kind::King.into())
            .next()
            .map(|king_pos| {
                pinned(
                    position,
                    black_pieces,
                    white_pieces,
                    Color::Black,
                    king_pos,
                    Color::Black,
                )
            })
            .unwrap_or_else(Pinned::empty);

        let pawn_mask = {
            let mut mask = Default::default();
            for pos in position.bitboard(Color::Black.into(), Kind::Pawn.into()) {
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
            black_pieces,
            white_pieces,
            pinned,
            pawn_mask,
            result: vec![],
        })
    }

    fn advance(&mut self) {
        if self.black_king_checked {
            let black_king_pos = self
                .position
                .bitboard(Color::Black.into(), Kind::King.into())
                .next()
                .unwrap();
            attack_preventing_movements(
                self.position,
                self.memo,
                self.next_step,
                black_king_pos,
                true,
            )
            .unwrap();
            return;
        }

        self.drops();
        self.direct_attack_moves();
        self.discovered_attack_moves();
    }

    fn drops(&mut self) {
        for kind in self.position.hands().kinds(Color::Black) {
            let empty_attack_squares = self.attack_squares(kind).and_not(self.white_pieces);
            empty_attack_squares.for_each(|pos| {
                self.maybe_add_move(&Movement::Drop(pos, kind), kind);
            })
        }
    }

    fn direct_attack_moves(&mut self) {
        self.non_leap_piece_direct_attack();
        self.leap_piece_direct_attack();
    }

    #[inline(never)]
    fn non_leap_piece_direct_attack(&mut self) {
        let lion_king_range = lion_king_power(self.white_king_pos);
        // Non line or leap pieces
        for attacker_pos in lion_king_range & self.black_pieces {
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
                bitboard::power(Color::Black, attacker_pos, attacker_source_kind)
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
                    self.maybe_add_move(
                        &Movement::Move {
                            source: attacker_pos,
                            dest,
                            promote,
                        },
                        attacker_source_kind,
                    );
                }
            }
        }
    }

    #[inline(never)]
    fn leap_piece_direct_attack(&mut self) {
        for attacker_source_kind in [
            Kind::Lance,
            Kind::Knight,
            Kind::Bishop,
            Kind::Rook,
            Kind::ProBishop,
            Kind::ProRook,
        ] {
            let attackers = self
                .position
                .bitboard(Color::Black.into(), attacker_source_kind.into());
            if attackers.is_empty() {
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

                let attack_squares = self.attack_squares(attacker_dest_kind);

                for attacker_pos in attackers {
                    let attacker_reachable = if self.pinned.is_pinned(attacker_pos) {
                        self.pinned.pinned_area(attacker_pos)
                    } else {
                        bitboard::reachable(
                            self.black_pieces,
                            self.white_pieces,
                            Color::Black,
                            attacker_pos,
                            attacker_source_kind,
                        )
                    };

                    for dest in attacker_reachable & attack_squares {
                        self.maybe_add_move(
                            &Movement::Move {
                                source: attacker_pos,
                                dest,
                                promote,
                            },
                            attacker_source_kind,
                        );
                    }
                }
            }
        }
    }

    #[inline(never)]
    fn discovered_attack_moves(&mut self) {
        let blockers = pinned(
            self.position,
            self.black_pieces,
            self.white_pieces,
            Color::White,
            self.white_king_pos,
            Color::Black,
        );
        for (blocker_pos, blocker_pinned_area) in blockers.iter() {
            let (blocker_pos, blocker_pinned_area) = (*blocker_pos, *blocker_pinned_area);
            let blocker_kind = self.position.get(blocker_pos).unwrap().1;
            let mut blocker_dest_cands = bitboard::reachable(
                self.black_pieces,
                self.white_pieces,
                Color::Black,
                blocker_pos,
                blocker_kind,
            )
            .and_not(blocker_pinned_area);
            if self.pinned.is_pinned(blocker_pos) {
                blocker_dest_cands &= self.pinned.pinned_area(blocker_pos);
            }
            let maybe_promotable = blocker_kind.promote().is_some();
            for blocker_dest in blocker_dest_cands {
                self.maybe_add_move(
                    &Movement::Move {
                        source: blocker_pos,
                        dest: blocker_dest,
                        promote: false,
                    },
                    blocker_kind,
                );
                if maybe_promotable {
                    self.maybe_add_move(
                        &Movement::Move {
                            source: blocker_pos,
                            dest: blocker_dest,
                            promote: true,
                        },
                        blocker_kind,
                    )
                }
            }
        }
    }
}

// Helper
impl<'a> Context<'a> {
    fn maybe_add_move(&mut self, movement: &Movement, kind: Kind) {
        if !common::maybe_legal_movement(Color::Black, movement, kind, self.pawn_mask) {
            return;
        }

        let mut next_position = self.position.clone();
        next_position.do_move(movement);

        if kind == Kind::King && common::checked(&next_position, Color::Black) {
            return;
        }

        debug_assert!(
            !common::checked(&next_position, Color::Black),
            "Black king checked: {:?}",
            next_position
        );

        let digest = next_position.digest();
        if self.memo.contains_key(&digest) {
            return;
        }
        self.memo.insert(digest, self.next_step);

        self.result.push(next_position);
    }

    // Squares moving to which produces a check.
    fn attack_squares(&self, kind: Kind) -> BitBoard {
        bitboard::reachable(
            self.white_pieces,
            self.black_pieces,
            Color::White,
            self.white_king_pos,
            kind,
        )
    }
}

fn lion_king_power(pos: Square) -> BitBoard {
    let mut res = bitboard::power(Color::Black, pos, Kind::King);
    for i in [-1, 1] {
        for j in [-1, 1] {
            let col = pos.col() as isize + i;
            let row = pos.row() as isize + j;
            if (0..9).contains(&col) && (0..9).contains(&row) {
                res |= bitboard::power(
                    Color::Black,
                    Square::new(col as usize, row as usize),
                    Kind::King,
                );
            }
        }
    }
    res
}
