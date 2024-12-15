use crate::piece::{Color, Kind};

use super::{checked, Movement, Position, Square};

pub enum UndoMove {
    UnDrop(Square, bool /* pawn drop */),
    UnMove {
        source: Square,
        dest: Square,
        promote: bool,
        capture: Option<Kind>,
        pawn_drop: bool,
    },
}

pub trait PositionExt {
    fn do_move(&mut self, m: &Movement) -> UndoMove;
    fn undo_move(&mut self, m: &UndoMove) -> Movement;
    fn checked_slow(&self, c: Color) -> bool;
}

impl PositionExt for Position {
    fn do_move(&mut self, m: &Movement) -> UndoMove {
        let color = self.turn();
        self.set_turn(color.opposite());
        let pawn_drop = self.pawn_drop();
        match *m {
            Movement::Drop(pos, k) => {
                self.hands_mut().remove(color, k);
                self.set(pos, color, k);
                self.set_pawn_drop(k == Kind::Pawn);

                UndoMove::UnDrop(pos, pawn_drop)
            }
            Movement::Move {
                source,
                source_kind_hint,
                dest,
                promote,
                capture_kind_hint,
            } => {
                let kind = if let Some(kind) = source_kind_hint {
                    kind
                } else {
                    self.kind_bb().must_get(source)
                };

                self.unset(source, color, kind);

                let capture = if let Some(capture) = capture_kind_hint {
                    capture
                } else {
                    self.kind_bb().get(dest)
                };

                if let Some(capture) = capture {
                    self.unset(dest, color.opposite(), capture);
                    self.hands_mut().add(color, capture.maybe_unpromote());
                }
                if promote {
                    self.set(dest, color, kind.promote().unwrap());
                } else {
                    self.set(dest, color, kind);
                }
                self.set_pawn_drop(false);

                UndoMove::UnMove {
                    source,
                    dest,
                    promote,
                    capture,
                    pawn_drop,
                }
            }
        }
    }

    // Undoes an movement. The token should be valid for the current position and otherwise it panics.
    // Returns the movement to redo.
    fn undo_move(&mut self, token: &UndoMove) -> Movement {
        use UndoMove::*;
        let prev_turn = self.turn().opposite();
        self.set_turn(prev_turn);
        match token {
            &UnDrop(pos, pawn_drop) => {
                let k = self.kind_bb().must_get(pos);
                self.unset(pos, prev_turn, k);
                self.hands_mut().add(prev_turn, k.maybe_unpromote());
                self.set_pawn_drop(pawn_drop);
                Movement::Drop(pos, k.maybe_unpromote())
            }
            &UnMove {
                source: from,
                dest: to,
                promote,
                capture,
                pawn_drop,
            } => {
                let k = self.kind_bb().must_get(to);
                self.unset(to, prev_turn, k);
                debug_assert_eq!(None, self.get(from));
                let prev_k = if promote { k.unpromote().unwrap() } else { k };
                self.set(from, prev_turn, prev_k);
                if let Some(captured_k) = capture {
                    self.set(to, prev_turn.opposite(), captured_k);
                    self.hands_mut()
                        .remove(prev_turn, captured_k.maybe_unpromote());
                }
                self.set_pawn_drop(pawn_drop);
                Movement::Move {
                    source: from,
                    dest: to,
                    promote,

                    capture_kind_hint: None,
                    source_kind_hint: None,
                }
            }
        }
    }

    fn checked_slow(&self, c: Color) -> bool {
        checked(self, c, None, None)
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        piece::Kind,
        position::{Movement, PositionExt, Square},
    };

    #[test]
    fn test_do_move_undo() {
        use crate::sfen;
        for tc in &[
            (
                sfen::tests::START,
                Movement::move_without_hint(Square::new(1, 6), Square::new(1, 5), false),
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
                Movement::move_without_hint(Square::new(7, 4), Square::new(6, 6), true),
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
