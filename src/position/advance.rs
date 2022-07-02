use std::cell::{Cell, RefCell};

use anyhow::bail;

use crate::piece::{Color, Kind};

use super::{
    bitboard11::{self, BitBoard},
    rule, Movement, Position, PositionExt, Square,
};

pub fn advance(position: Position) -> anyhow::Result<Vec<Position>> {
    let ctx = Context::new(position)?;
    match ctx.turn {
        crate::piece::Color::Black => ctx.advance_black(),
        crate::piece::Color::White => ctx.advance_white(),
    };
    Ok(ctx.result.take())
}

struct Context {
    position: Position,
    turn: Color,
    white_king_pos: Square,
    black_pieces: BitBoard,
    white_pieces: BitBoard,
    pawn_mask: u16,
    result: RefCell<Vec<Position>>,
}

impl Context {
    fn new(position: Position) -> anyhow::Result<Self> {
        let turn = position.turn();
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

        let pawn_mask = {
            let mut mask = Default::default();
            for pos in position.bitboard(turn.into(), Kind::Pawn.into()) {
                mask |= 1 << pos.col()
            }
            mask
        };

        Ok(Self {
            position,
            turn,
            white_king_pos,
            black_pieces,
            white_pieces,
            pawn_mask,
            result: vec![].into(),
        })
    }

    fn advance_black(&self) {
        self.direct_attack_movements();
        self.discovered_attack_moves();
    }

    fn direct_attack_movements(&self) {
        debug_assert_eq!(self.turn, Color::Black);

        Kind::iter().for_each(|kind| {
            let attack_squares = self.attack_squares(kind);
            if attack_squares.is_empty() {
                return;
            }
            let empty_attack_squares = attack_squares & !self.white_pieces;
            // Drop
            if !empty_attack_squares.is_empty() && self.position.hands().contains(self.turn, kind) {
                empty_attack_squares.for_each(|pos| {
                    self.maybe_add_move(Movement::Drop(pos, kind));
                })
            }
            // Move
            for (sources, promote, source_kind) in self.sources_becoming(kind) {
                if sources.is_empty() {
                    continue;
                }
                sources.into_iter().for_each(|source| {
                    let move_to = bitboard11::reachable(
                        self.black_pieces,
                        self.white_pieces,
                        self.turn,
                        source,
                        source_kind,
                    ) & attack_squares;
                    if move_to.is_empty() {
                        return;
                    }
                    move_to.into_iter().for_each(|dest| {
                        self.maybe_add_move(Movement::Move {
                            from: source,
                            to: dest,
                            promote,
                        })
                    });
                })
            }
        });
    }

    fn discovered_attack_moves(&self) {
        debug_assert_eq!(self.turn, Color::Black);

        for kind in vec![Kind::Lance, Kind::Bishop, Kind::Rook] {
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
                            self.turn,
                            blocker_pos,
                            blocker_kind,
                        )
                };
                for blocker_dest in blocker_dests {
                    self.maybe_add_move(Movement::Move {
                        from: blocker_pos,
                        to: blocker_dest,
                        promote: false,
                    });
                    if (rule::promotable(blocker_pos, Color::Black)
                        || rule::promotable(blocker_dest, Color::Black))
                        && blocker_kind.promote().is_some()
                    {
                        self.maybe_add_move(Movement::Move {
                            from: blocker_pos,
                            to: blocker_dest,
                            promote: true,
                        })
                    }
                }
            }
        }
    }

    fn advance_white(&self) {
        let (attacker_pos, attacker_kind) = self
            .attacker()
            .unwrap_or_else(|| panic!("BUG: attacker not found: {:?}", self.position));
        self.white_block(attacker_pos, attacker_kind);
        self.white_capture(attacker_pos);
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
        let dests = bitboard11::reachable(
            self.black_pieces,
            self.white_pieces,
            self.turn,
            self.white_king_pos,
            Kind::King,
        );
        for dest in dests {
            self.maybe_add_move(Movement::Move {
                from: self.white_king_pos,
                to: dest,
                promote: false,
            })
        }
    }

    fn add_movements_to(&self, dest: Square, include_drop: bool) {
        debug_assert_eq!(self.turn, Color::White);
        // Drop
        if include_drop {
            for kind in self.position.hands().kinds(self.turn) {
                self.maybe_add_move(Movement::Drop(dest, kind));
            }
        }
        // Move
        for kind in Kind::iter() {
            if kind == Kind::King {
                continue;
            }
            for (sources, promote, source_kind) in self.sources_becoming(kind) {
                let source_cands = bitboard11::reachable(
                    self.black_pieces,
                    self.white_pieces,
                    self.turn.opposite(),
                    dest,
                    source_kind,
                );
                for source in sources & source_cands {
                    self.maybe_add_move(Movement::Move {
                        from: source,
                        to: dest,
                        promote,
                    })
                }
            }
        }
    }

    fn attacker(&self) -> Option<(Square, Kind)> {
        for kind in Kind::iter() {
            let existing = self
                .position
                .bitboard(self.turn.opposite().into(), kind.into());
            if existing.is_empty() {
                continue;
            }
            let attacking = bitboard11::reachable(
                self.black_pieces,
                self.white_pieces,
                self.turn,
                self.white_king_pos,
                kind,
            ) & existing;
            if attacking.is_empty() {
                continue;
            }
            return attacking.into_iter().next().map(|pos| (pos, kind));
        }
        None
    }
}

// Helper methods
impl Context {
    fn maybe_add_move(&self, movement: Movement) {
        eprintln!("maybe_add_move: {:?}", movement);
        match movement {
            Movement::Drop(pos, kind) => {
                if kind == Kind::Pawn && self.pawn_mask >> pos.col() & 1 > 0 {
                    return;
                }
                if !rule::is_movable(self.turn, pos, kind) {
                    return;
                }
            }
            Movement::Move {
                from: source,
                to: dest,
                promote,
            } => {
                let kind = self.position.get(source).unwrap().1;
                if !rule::is_allowed_move(self.turn, source, dest, kind, promote) {
                    return;
                }
            }
        }

        let mut next_position = self.position.clone();
        next_position.do_move(&movement);

        if position_illegal_by_selfcheck(&next_position) {
            return;
        }
        if self.position.turn() == Color::Black {
            debug_assert!(
                next_position.checked(Color::White),
                "Not checking: {:?}",
                next_position
            );
        }
        self.result.borrow_mut().push(next_position);
    }

    fn attack_squares(&self, kind: Kind) -> BitBoard {
        debug_assert_eq!(self.turn, Color::Black);
        bitboard11::reachable(
            self.white_pieces,
            self.black_pieces,
            Color::White,
            self.white_king_pos,
            kind,
        )
    }

    fn sources_becoming(&self, kind: Kind) -> impl Iterator<Item = (BitBoard, bool, Kind)> {
        [
            Some((
                self.position.bitboard(self.turn.into(), kind.into()),
                false,
                kind,
            )),
            kind.unpromote().map(|raw| {
                (
                    self.position.bitboard(self.turn.into(), raw.into()),
                    true,
                    raw,
                )
            }),
        ]
        .into_iter()
        .filter_map(|x| x)
        .filter(|x| !x.0.is_empty())
    }

    fn blockable_squares(&self, attacker_pos: Square, attacker_kind: Kind) -> BitBoard {
        debug_assert!(self.turn == Color::White);
        bitboard11::reachable(
            self.black_pieces,
            self.white_pieces,
            self.turn,
            self.white_king_pos,
            attacker_kind.maybe_unpromote(),
        ) & bitboard11::reachable(
            self.black_pieces,
            self.white_pieces,
            self.turn.opposite(),
            attacker_pos,
            attacker_kind.maybe_unpromote(),
        )
    }
}

fn position_illegal_by_selfcheck(position: &Position) -> bool {
    position.checked(position.turn().opposite())
}

#[cfg(test)]
mod tests {
    use crate::position::{Position, PositionExt};

    #[test]
    fn advance() {
        use crate::sfen;
        use pretty_assertions::assert_eq;
        for tc in vec![
            // Black moves
            (
                "8k/9/9/9/9/9/9/9/9 b P2r2b4g4s4n4l17p 1",
                // Drop pawn
                vec!["P*12"],
            ),
            (
                "9/9/5lp2/5lk2/5l3/9/5N3/7L1/9 b P2r2b4g4s3n16p 1",
                // Drop pawn mate is not checked here
                vec!["P*35"],
            ),
            (
                "8k/9/8K/9/9/9/9/9/9 b 2r2b4g4s4n4l18p 1",
                // King cannot attack
                vec![],
            ),
            (
                "4R4/9/4P4/9/4k1P1R/9/2N1s4/1B7/4L4 b b4g3s3n3l16p 1",
                // Discovered attacks
                vec!["7785", "7765", "5957", "3534"],
            ),
            ("9/9/9/9/9/k8/n1P6/LB7/9 b P2rb4g4s3n3l15p", vec!["9897"]),
            // White moves
            (
                "3pks3/4+B4/4+P4/9/9/9/9/9/9 w S2rb4g2s4n4l16p 1",
                vec!["4152"],
            ),
            (
                "3+pk4/5S3/9/9/9/8B/9/9/9 w 2rb4g2s4n4l17p",
                vec!["5142", "5162"],
            ),
            ("7br/5ssss/5gggg/9/9/B8/1n1K5/9/R2k5 w 3n4l18p 1", vec![]),
        ] {
            eprintln!("{}", tc.0);

            let position =
                sfen::decode_position(tc.0).expect(&format!("Failed to decode {}", tc.0));
            let mut got = super::advance(position.clone()).unwrap();
            got.sort();

            let mut want = sfen::decode_moves(&tc.1.join(" "))
                .unwrap()
                .iter()
                .map(|m| {
                    let mut b = position.clone();
                    b.do_move(m);
                    b
                })
                .collect::<Vec<Position>>();
            want.sort();
            assert_eq!(got, want);
        }
    }
}
