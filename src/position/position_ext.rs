use std::collections::HashMap;

use anyhow::bail;

use crate::piece::{Color, Kind, NUM_KIND};

use super::{
    bitboard11::{self, BitBoard},
    rule::promotable,
    Movement, Position, Square,
};

pub enum UndoMove {
    UnDrop((Square, bool /* pawn drop */)),
    UnMove {
        from: Square,
        to: Square,
        promote: bool,
        capture: Option<Kind>,
        pawn_drop: bool,
    },
}

pub trait PositionExt {
    fn do_move(&mut self, m: &Movement) -> UndoMove;
    fn undo_move(&mut self, m: &UndoMove) -> Movement;
    fn checked(&self, c: Color) -> bool;
}

impl PositionExt for Position {
    fn do_move(&mut self, m: &Movement) -> UndoMove {
        let color = self.turn();
        let token;
        match m {
            Movement::Drop(pos, k) => {
                let (pos, k) = (*pos, *k);
                self.hands_mut().remove(color, k);
                self.set(pos, color, k);
                token = UndoMove::UnDrop((pos, self.pawn_drop()));
                self.set_pawn_drop(k == Kind::Pawn);
            }
            Movement::Move {
                from: source,
                to: dest,
                promote,
            } => {
                let kind = self.get(*source).unwrap().1;
                self.unset(*source, color, kind);
                let capture = if let Some(capture) = self.get(*dest).map(|(c, k)| k) {
                    self.unset(*dest, color.opposite(), capture);
                    self.hands_mut().add(color, capture.maybe_unpromote());
                    Some(capture)
                } else {
                    None
                };
                if *promote {
                    self.set(*dest, color, kind.promote().unwrap());
                } else {
                    self.set(*dest, color, kind);
                }
                token = UndoMove::UnMove {
                    from: *source,
                    to: *dest,
                    promote: *promote,
                    capture,
                    pawn_drop: self.pawn_drop(),
                };
                self.set_pawn_drop(false);
            }
        }
        self.set_turn(color.opposite());
        token
    }

    // Undoes an movement. The token should be valid for the current position and otherwise it panics.
    // Returns the movement to redo.
    fn undo_move(&mut self, token: &UndoMove) -> Movement {
        use UndoMove::*;
        let prev_turn = self.turn().opposite();
        self.set_turn(prev_turn);
        match token {
            &UnDrop((pos, pawn_drop)) => {
                let (c, k) = self
                    .get(pos)
                    .expect(&format!("{:?} doesn't contain any piece", pos));
                debug_assert_eq!(prev_turn, c);
                self.unset(pos, c, k);
                self.hands_mut().add(c, k.maybe_unpromote());
                self.set_pawn_drop(pawn_drop);
                Movement::Drop(pos, k.maybe_unpromote())
            }
            &UnMove {
                from,
                to,
                promote,
                capture,
                pawn_drop,
            } => {
                let (c, k) = self
                    .get(to)
                    .expect(&format!("{:?} doesn't contain any piece", to));
                debug_assert_eq!(prev_turn, c);
                self.unset(to, c, k);
                debug_assert_eq!(None, self.get(from));
                let prev_k = if promote {
                    k.unpromote().expect(&format!("can't unpromote {:?}", k))
                } else {
                    k
                };
                self.set(from, c, prev_k);
                if let Some(captured_k) = capture {
                    self.set(to, c.opposite(), captured_k);
                    self.hands_mut().remove(c, captured_k.maybe_unpromote());
                }
                self.set_pawn_drop(pawn_drop);
                Movement::Move { from, to, promote }
            }
        }
    }

    fn checked(&self, c: Color) -> bool {
        match king(self, c) {
            Some(king_pos) => attackers_to_with_king(self, king_pos, c.opposite())
                .next()
                .is_some(),
            None => false,
        }
    }
}

pub(super) fn attackers_to_with_king(
    position: &Position,
    target: Square,
    color: Color,
) -> impl Iterator<Item = (Square, Kind)> + '_ {
    let black_pieces = position.bitboard(Color::Black.into(), None);
    let white_pieces = position.bitboard(Color::White.into(), None);
    Kind::iter().flat_map(move |kind| {
        let b = bitboard11::reachable(black_pieces, white_pieces, color.opposite(), target, kind)
            & position.bitboard(Some(color), Some(kind));
        b.map(move |from| (from, kind))
    })
}
fn king(position: &Position, c: Color) -> Option<Square> {
    for k in position.bitboard(Some(c), Some(Kind::King)) {
        return Some(k);
    }
    None
}
lazy_static! {
    static ref COL_MASKS: [BitBoard; 9] = {
        let mut res = [BitBoard::new(); 9];
        for pos in Square::iter() {
            res[pos.col()].set(pos);
        }
        res
    };
}

#[cfg(test)]
mod tests {
    use crate::{
        piece::{Color, Kind},
        position::{position_ext::king, Movement, Position, PositionExt, Square},
        sfen,
    };

    #[test]
    fn test_do_move_undo() {
        use crate::sfen;
        for tc in vec![
            (
                sfen::tests::START,
                Movement::Move {
                    from: Square::new(1, 6),
                    to: Square::new(1, 5),
                    promote: false,
                },
                "lnsgkgsnl/1r5b1/ppppppppp/9/9/7P1/PPPPPPP1P/1B5R1/LNSGKGSNL w -",
            ),
            (
                sfen::tests::RYUO,
                Movement::Drop(Square::new(2, 0), Kind::Pawn),
                "6p1l/1l+R2P3/p2pBG1pp/kps1p4/Nn1P2G2/P1P1P2PP/1PS6/1KSG3+r1/LN2+p3L b Sbgn2p",
            ),
            (
                sfen::tests::RYUO,
                // Capture and promote.
                Movement::Move {
                    from: Square::new(7, 4),
                    to: Square::new(6, 6),
                    promote: true,
                },
                "8l/1l+R2P3/p2pBG1pp/kps1p4/N2P2G2/P1P1P2PP/1P+n6/1KSG3+r1/LN2+p3L b Sbgsn3p",
            ),
        ] {
            let (board_sfen, movement, want) = (tc.0, tc.1, tc.2);
            let mut board = sfen::decode_position(board_sfen).unwrap();
            let token = board.do_move(&movement);
            assert_eq!(want, sfen::encode_position(&board));
            let m = board.undo_move(&token);
            assert_eq!(board_sfen, sfen::encode_position(&board));
            board.do_move(&m);
            assert_eq!(want, sfen::encode_position(&board));
        }
    }
}
