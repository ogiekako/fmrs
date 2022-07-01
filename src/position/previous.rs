use std::cell::RefCell;

use crate::piece::{Color, Kind};

use super::{bitboard::BitBoard, rule, Position, Square, UndoMove};

pub fn previous(position: Position, allow_drop_pawn: bool) -> Vec<UndoMove> {
    let ctx = Context::new(position, allow_drop_pawn);
    ctx.previous();
    ctx.result.take()
}

struct Context {
    position: Position,
    allow_drop_pawn: bool,
    turn: Color,
    turn_pieces: BitBoard,
    opponent_pieces: BitBoard,
    result: RefCell<Vec<UndoMove>>,
}

impl Context {
    fn new(position: Position, allow_drop_pawn: bool) -> Self {
        let turn = position.turn();
        let turn_pieces = position.bitboard(turn.into(), None);
        let opponent_pieces = position.bitboard(turn.opposite().into(), None);
        Self {
            position,
            allow_drop_pawn,
            turn,
            turn_pieces,
            opponent_pieces,
            result: vec![].into(),
        }
    }

    fn previous(&self) {
        for kind in Kind::iter() {
            let dests = self
                .position
                .bitboard(self.turn.opposite().into(), kind.into());
            for dest in dests {
                self.add_undo_moves_to(dest, kind, false);
                self.add_undo_moves_to(dest, kind, true);
            }
        }
    }

    fn add_undo_moves_to(&self, dest: Square, kind: Kind, pawn_drop: bool) {
        // Drop
        if kind.is_hand_piece() {
            if kind != Kind::Pawn || self.allow_drop_pawn {
                self.add_undo_move(UndoMove::UnDrop((dest, pawn_drop)));
            }
        }
        // Move
        let prev_kinds = [(kind.unpromote(), true), (kind.into(), false)]
            .into_iter()
            .filter_map(|x| x.0.map(|k| (k, x.1)));
        for (prev_kind, promote) in prev_kinds {
            let sources = rule::movable_positions(
                self.turn_pieces,
                self.opponent_pieces,
                self.turn,
                dest,
                prev_kind,
            ) & !self.opponent_pieces;
            for source in sources {
                self.add_undo_move(UndoMove::UnMove {
                    from: source,
                    to: dest,
                    promote,
                    capture: None,
                    pawn_drop,
                });
                for capture in self.position.hands().kinds(self.turn.opposite()) {
                    self.add_undo_move(UndoMove::UnMove {
                        from: source,
                        to: dest,
                        promote,
                        capture: capture.into(),
                        pawn_drop,
                    });
                    if let Some(promoted) = capture.promote() {
                        self.add_undo_move(UndoMove::UnMove {
                            from: source,
                            to: dest,
                            promote,
                            capture: promoted.into(),
                            pawn_drop,
                        })
                    }
                }
            }
        }
    }
}

// Helper methods
impl Context {
    fn add_undo_move(&self, movement: UndoMove) {
        self.result.borrow_mut().push(movement);
    }
}
