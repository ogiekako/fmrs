use std::cell::RefCell;
use std::collections::HashMap;

use anyhow::bail;

use crate::piece::{Color, Kind};

use crate::position::Digest;
use crate::position::{
    bitboard::{self, BitBoard},
    Movement, Position, PositionExt, Square,
};

use super::common;

pub(super) fn advance_old(position: &Position) -> anyhow::Result<Vec<Position>> {
    advance(position, &mut HashMap::new(), 0).map(|x| x.0)
}

pub(super) fn advance(
    position: &Position,
    memo: &mut HashMap<Digest, usize>,
    next_step: usize,
) -> anyhow::Result<(Vec<Position>, /* is mate */ bool)> {
    debug_assert_eq!(position.turn(), Color::White);
    let ctx = Context::new(position, memo, next_step)?;
    ctx.advance();
    Ok((ctx.result.take(), ctx.is_mate.take()))
}

struct Context<'a> {
    position: &'a Position,
    memo: RefCell<&'a mut HashMap<Digest, usize>>,
    next_step: usize,
    white_king_pos: Square,
    black_pieces: BitBoard,
    white_pieces: BitBoard,
    pinned: common::Pinned,
    attacker: Attacker,
    pawn_mask: usize,
    result: RefCell<Vec<Position>>,
    is_mate: RefCell<bool>,
}

impl<'a> Context<'a> {
    fn new(
        position: &'a Position,
        memo: &'a mut HashMap<Digest, usize>,
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
        let black_pieces = position.bitboard(Color::Black.into(), None);
        let white_pieces = position.bitboard(Color::White.into(), None);
        let pinned = common::pinned(
            position,
            black_pieces,
            white_pieces,
            Color::White,
            white_king_pos,
        );
        let attacker = attacker(position, black_pieces, white_pieces, white_king_pos)
            .ok_or_else(|| anyhow::anyhow!("white not checked"))?;
        let pawn_mask = {
            let mut mask = Default::default();
            for pos in position.bitboard(Color::White.into(), Kind::Pawn.into()) {
                mask |= 1 << pos.col()
            }
            mask
        };

        Ok(Self {
            position,
            memo: memo.into(),
            next_step,
            white_king_pos,
            black_pieces,
            white_pieces,
            pinned,
            attacker,
            pawn_mask,
            result: vec![].into(),
            is_mate: true.into(),
        })
    }

    fn advance(&self) {
        if !self.attacker.double_check {
            self.white_block(self.attacker.pos, self.attacker.kind);
            self.white_capture(self.attacker.pos);
        }
        self.white_king_move();
    }

    fn white_block(&self, attacker_pos: Square, attacker_kind: Kind) {
        if attacker_kind.is_line_piece() {
            let blockable = self.blockable_squares(attacker_pos, attacker_kind);
            for dest in blockable {
                self.add_movements_to(dest, true);
            }
        }
    }

    fn white_capture(&self, attacker_pos: Square) {
        self.add_movements_to(attacker_pos, false)
    }

    fn white_king_move(&self) {
        let king_reachable = bitboard::reachable(
            self.black_pieces,
            self.white_pieces,
            Color::White,
            self.white_king_pos,
            Kind::King,
        );
        let mut under_attack = BitBoard::empty();
        for attacker_kind in Kind::iter() {
            for attacker_pos in self
                .position
                .bitboard(Color::Black.into(), attacker_kind.into())
            {
                let attacker_power = bitboard::power(Color::Black, attacker_pos, attacker_kind);
                if (attacker_power & king_reachable).is_empty() {
                    continue;
                }
                if !attacker_kind.is_line_piece() {
                    under_attack |= attacker_power;
                    continue;
                }
                let attacker_reachable = bitboard::reachable(
                    self.white_pieces,
                    self.black_pieces,
                    Color::Black,
                    attacker_pos,
                    attacker_kind,
                );
                under_attack |= attacker_reachable;

                // Hidden by king
                if attacker_pos == self.attacker.pos {
                    if let Some(hidden_pos) = hidden_square(attacker_pos, self.white_king_pos) {
                        if attacker_power.get(hidden_pos) {
                            under_attack.set(hidden_pos);
                        }
                    }
                }
            }
        }

        for dest in king_reachable.and_not(under_attack) {
            self.maybe_add_move(
                &Movement::Move {
                    source: self.white_king_pos,
                    dest,
                    promote: false,
                },
                Kind::King,
            )
        }
    }

    fn add_movements_to(&self, dest: Square, include_drop: bool) {
        // Drop
        if include_drop {
            for kind in self.position.hands().kinds(Color::White) {
                self.maybe_add_move(&Movement::Drop(dest, kind), kind);
            }
        }

        // Move
        let around_dest = bitboard::power(Color::White, dest, Kind::King) & self.white_pieces;
        for source_pos in around_dest {
            let source_kind = self.position.get(source_pos).unwrap().1;
            if source_kind == Kind::King {
                continue;
            }
            let source_power = bitboard::power(Color::White, source_pos, source_kind);
            if source_power.get(dest) {
                for promote in [false, true] {
                    if promote && source_kind.promote().is_none() {
                        continue;
                    }
                    self.maybe_add_move(
                        &Movement::Move {
                            source: source_pos,
                            dest,
                            promote,
                        },
                        source_kind,
                    );
                }
            }
        }

        for leap_kind in [Kind::Lance, Kind::Knight, Kind::Bishop, Kind::Rook] {
            let on_board = {
                let raw_pieces = self
                    .position
                    .bitboard(Color::White.into(), leap_kind.into());
                let promoted_kind = leap_kind.promote().unwrap();
                if promoted_kind.is_line_piece() {
                    raw_pieces
                        | self
                            .position
                            .bitboard(Color::White.into(), promoted_kind.into())
                } else {
                    raw_pieces
                }
            };
            if on_board.is_empty() {
                continue;
            }
            let sources = bitboard::reachable(
                self.black_pieces,
                self.white_pieces,
                Color::Black,
                dest,
                leap_kind,
            ) & on_board;
            for source_pos in sources {
                let source_kind = self.position.get(source_pos).unwrap().1;
                for promote in [false, true] {
                    if promote && source_kind.promote().is_none() {
                        continue;
                    }
                    self.maybe_add_move(
                        &Movement::Move {
                            source: source_pos,
                            dest,
                            promote,
                        },
                        source_kind,
                    )
                }
            }
        }
    }
}

// Helper methods
impl<'a> Context<'a> {
    fn maybe_add_move(&self, movement: &Movement, kind: Kind) {
        if !common::maybe_legal_movement(Color::White, movement, kind, self.pawn_mask) {
            return;
        }
        if let Movement::Move {
            source,
            dest,
            promote: _,
        } = movement
        {
            if !self.pinned.legal_move(*source, *dest) {
                return;
            }
        }

        let mut next_position = self.position.clone();
        next_position.do_move(movement);

        if self.attacker.double_check && common::checked(&next_position, Color::White) {
            return;
        }

        debug_assert!(
            !common::checked(&next_position, Color::White),
            "white king checked: posision={:?} movement={:?} next={:?}",
            self.position,
            movement,
            next_position
        );

        *self.is_mate.borrow_mut() = false;
        let digest = next_position.digest();
        if self.memo.borrow().contains_key(&digest) {
            return;
        }
        self.memo.borrow_mut().insert(digest, self.next_step);

        self.result.borrow_mut().push(next_position);
    }

    fn blockable_squares(&self, attacker_pos: Square, attacker_kind: Kind) -> BitBoard {
        if bitboard::power(Color::White, self.white_king_pos, Kind::King).get(attacker_pos) {
            return BitBoard::empty();
        }
        bitboard::reachable(
            self.black_pieces,
            self.white_pieces,
            Color::White,
            self.white_king_pos,
            attacker_kind.maybe_unpromote(),
        ) & bitboard::reachable(
            self.black_pieces,
            self.white_pieces,
            Color::Black,
            attacker_pos,
            attacker_kind.maybe_unpromote(),
        )
    }
}

struct Attacker {
    pos: Square,
    kind: Kind,
    double_check: bool,
}

impl Attacker {
    fn new(pos: Square, kind: Kind, double_check: bool) -> Self {
        Self {
            pos,
            kind,
            double_check,
        }
    }
}

fn attacker(
    position: &Position,
    black_pieces: BitBoard,
    white_pieces: BitBoard,
    white_king_pos: Square,
) -> Option<Attacker> {
    let mut attacker: Option<Attacker> = None;
    for attacker_kind in Kind::iter() {
        let existing = position.bitboard(Color::Black.into(), attacker_kind.into());
        if existing.is_empty() {
            continue;
        }
        let attacking = bitboard::reachable(
            black_pieces,
            white_pieces,
            Color::White,
            white_king_pos,
            attacker_kind,
        ) & existing;
        if attacking.is_empty() {
            continue;
        }
        for attacker_pos in attacking {
            if let Some(mut attacker) = attacker.take() {
                attacker.double_check = true;
                return Some(attacker);
            }
            attacker = Some(Attacker::new(attacker_pos, attacker_kind, false));
        }
    }
    attacker
}

// Potentially attacked position which is currently hidden by the king.
fn hidden_square(attacker_pos: Square, king_pos: Square) -> Option<Square> {
    let (kc, kr) = (king_pos.col() as isize, king_pos.row() as isize);
    let (ac, ar) = (attacker_pos.col() as isize, attacker_pos.row() as isize);

    let (dc, dr) = (kc - ac, kr - ar);
    let d = dc.abs().max(dr.abs());
    let (rc, rr) = (kc + dc / d, kr + dr / d);
    if (0..9).contains(&rc) && (0..9).contains(&rr) {
        Some(Square::new(rc as usize, rr as usize))
    } else {
        None
    }
}
