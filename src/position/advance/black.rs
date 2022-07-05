use std::cell::RefCell;

use anyhow::bail;

use crate::piece::{Color, Kind};

use crate::position::{
    bitboard11::{self, BitBoard},
    rule, Movement, Position, PositionExt, Square,
};

use super::common::{self, Pinned};

pub(super) fn advance(position: &Position) -> anyhow::Result<Vec<Position>> {
    debug_assert_eq!(position.turn(), Color::Black);
    let ctx = Context::new(position)?;
    ctx.advance();
    Ok(ctx.result.take())
}

struct Context<'a> {
    position: &'a Position,
    white_king_pos: Square,
    black_king_checked: bool,
    black_pieces: BitBoard,
    white_pieces: BitBoard,
    pinned: Option<Pinned>,
    pawn_mask: usize,
    result: RefCell<Vec<Position>>,
}

impl<'a> Context<'a> {
    fn new(position: &'a Position) -> anyhow::Result<Self> {
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
                common::pinned(position, black_pieces, white_pieces, Color::Black, king_pos)
            });

        let pawn_mask = {
            let mut mask = Default::default();
            for pos in position.bitboard(Color::Black.into(), Kind::Pawn.into()) {
                mask |= 1 << pos.col()
            }
            mask
        };

        Ok(Self {
            position,
            white_king_pos,
            black_king_checked,
            black_pieces,
            white_pieces,
            pinned,
            pawn_mask,
            result: vec![].into(),
        })
    }

    fn advance(&self) {
        self.drops();
        self.direct_attack_moves();
        self.discovered_attack_moves();
    }

    fn drops(&self) {
        for kind in self.position.hands().kinds(Color::Black) {
            let empty_attack_squares = self.attack_squares(kind) & !self.white_pieces;
            empty_attack_squares.for_each(|pos| {
                self.maybe_add_move(&Movement::Drop(pos, kind), kind);
            })
        }
    }

    fn direct_attack_moves(&self) {
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
            let attacker_power =
                bitboard11::power(Color::Black, attacker_pos, attacker_source_kind);
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
                    let attacker_reachable = bitboard11::reachable(
                        self.black_pieces,
                        self.white_pieces,
                        Color::Black,
                        attacker_pos,
                        attacker_source_kind,
                    );

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

    fn discovered_attack_moves(&self) {
        for kind in [Kind::Lance, Kind::Bishop, Kind::Rook] {
            let attacker_cands = {
                let mut cands = self.position.bitboard(Some(Color::Black), Some(kind));
                if kind != Kind::Lance {
                    cands |= self
                        .position
                        .bitboard(Some(Color::Black), Some(kind.promote().unwrap()));
                }
                if cands.is_empty() {
                    continue;
                }
                cands &= bitboard11::power(Color::White, self.white_king_pos, kind);
                if cands.is_empty() {
                    continue;
                }
                cands
            };
            let blocker_cands = bitboard11::reachable(
                self.black_pieces,
                self.white_pieces,
                Color::White,
                self.white_king_pos,
                kind,
            );
            if blocker_cands.is_empty() {
                continue;
            }
            for attacker_pos in attacker_cands {
                let blocker_pos = {
                    let pos = bitboard11::reachable(
                        self.white_pieces,
                        self.black_pieces,
                        Color::Black,
                        attacker_pos,
                        kind,
                    ) & blocker_cands;
                    if pos.is_empty() {
                        continue;
                    }
                    pos.into_iter().next().unwrap()
                };
                let blocker_kind = self.position.get(blocker_pos).unwrap().1;

                let blocker_dests = {
                    let attacker_preventing =
                        bitboard11::power(Color::White, self.white_king_pos, kind)
                            & bitboard11::power(Color::Black, attacker_pos, kind);
                    !attacker_preventing
                        & bitboard11::reachable(
                            self.black_pieces,
                            self.white_pieces,
                            Color::Black,
                            blocker_pos,
                            blocker_kind,
                        )
                };
                for blocker_dest in blocker_dests {
                    self.maybe_add_move(
                        &Movement::Move {
                            source: blocker_pos,
                            dest: blocker_dest,
                            promote: false,
                        },
                        blocker_kind,
                    );
                    if (rule::promotable(blocker_pos, Color::Black)
                        || rule::promotable(blocker_dest, Color::Black))
                        && blocker_kind.promote().is_some()
                    {
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
}

// Helper
impl<'a> Context<'a> {
    fn maybe_add_move(&self, movement: &Movement, kind: Kind) {
        if !common::maybe_legal_movement(Color::Black, movement, kind, self.pawn_mask) {
            return;
        }
        if let Some(pinned) = self.pinned.as_ref() {
            if let Movement::Move {
                source: from,
                dest: to,
                promote: _,
            } = movement
            {
                if !pinned.legal_move(*from, *to) {
                    return;
                }
            }
        }

        let mut next_position = self.position.clone();
        next_position.do_move(movement);

        if (self.black_king_checked || kind == Kind::King)
            && common::checked(&next_position, Color::Black)
        {
            return;
        }

        debug_assert!(
            !common::checked(&next_position, Color::Black),
            "Black king checked: {:?}",
            next_position
        );

        self.result.borrow_mut().push(next_position);
    }

    // Squares moving to which produces a check.
    fn attack_squares(&self, kind: Kind) -> BitBoard {
        bitboard11::reachable(
            self.white_pieces,
            self.black_pieces,
            Color::White,
            self.white_king_pos,
            kind,
        )
    }
}

fn lion_king_power(pos: Square) -> BitBoard {
    let mut res = bitboard11::power(Color::Black, pos, Kind::King);
    for i in [-1, 1] {
        for j in [-1, 1] {
            let col = pos.col() as isize + i;
            let row = pos.row() as isize + j;
            if (0..9).contains(&col) && (0..9).contains(&row) {
                res |= bitboard11::power(
                    Color::Black,
                    Square::new(col as usize, row as usize),
                    Kind::King,
                );
            }
        }
    }
    res
}
