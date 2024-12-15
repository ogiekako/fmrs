use std::cell::RefCell;

use crate::piece::{Color, Kind};

use super::{
    bitboard::{self, ColorBitBoard},
    rule, Position, Square, UndoMove,
};

pub fn previous(position: Position, allow_drop_pawn: bool) -> Vec<UndoMove> {
    let ctx = Context::new(position, allow_drop_pawn);
    ctx.previous();
    ctx.result.take()
}

struct Context {
    position: Position,
    color_bb: ColorBitBoard,
    allow_drop_pawn: bool,
    turn: Color,
    result: RefCell<Vec<UndoMove>>,
}

impl Context {
    fn new(position: Position, allow_drop_pawn: bool) -> Self {
        let turn = position.turn();
        let color_bb = position.color_bb();
        Self {
            position,
            color_bb,
            allow_drop_pawn,
            turn,
            result: vec![].into(),
        }
    }

    fn previous(&self) {
        for kind in Kind::iter() {
            let dests = self.position.bitboard(self.turn.opposite(), kind);
            for dest in dests {
                self.add_undo_moves_to(dest, kind, false);
                self.add_undo_moves_to(dest, kind, true);
            }
        }
    }

    fn add_undo_moves_to(&self, dest: Square, kind: Kind, was_pawn_drop: bool) {
        if self.position.pawn_drop() {
            if kind != Kind::Pawn || !self.allow_drop_pawn {
                return;
            }
            self.maybe_add_undo_move(UndoMove::UnDrop(dest, was_pawn_drop));
            return;
        }
        // Drop
        if kind.is_hand_piece() && kind != Kind::Pawn {
            self.maybe_add_undo_move(UndoMove::UnDrop(dest, was_pawn_drop));
        }
        // Move
        let prev_kinds = [(kind.unpromote(), true), (kind.into(), false)]
            .into_iter()
            .filter_map(|x| x.0.map(|k| (k, x.1)));
        for (prev_kind, promote) in prev_kinds {
            let sources = bitboard::reachable(&self.color_bb, self.turn, dest, prev_kind, false)
                .and_not(self.color_bb.both());
            for source in sources {
                self.maybe_add_undo_move(UndoMove::UnMove {
                    source,
                    dest,
                    promote,
                    capture: None,
                    pawn_drop: was_pawn_drop,
                });
                for capture in self.position.hands().kinds(self.turn.opposite()) {
                    self.maybe_add_undo_move(UndoMove::UnMove {
                        source,
                        dest,
                        promote,
                        capture: capture.into(),
                        pawn_drop: was_pawn_drop,
                    });
                    if let Some(promoted) = capture.promote() {
                        self.maybe_add_undo_move(UndoMove::UnMove {
                            source,
                            dest,
                            promote,
                            capture: promoted.into(),
                            pawn_drop: was_pawn_drop,
                        })
                    }
                }
            }
        }
    }
}

// Helper methods
impl Context {
    fn maybe_add_undo_move(&self, movement: UndoMove) {
        if let UndoMove::UnMove {
            source: from,
            dest: to,
            promote,
            capture: _,
            pawn_drop: _,
        } = &movement
        {
            let mut kind = self.position.get(*to).unwrap().1;
            if *promote {
                kind = kind.unpromote().unwrap();
            }
            if !rule::is_legal_move(self.position.turn().opposite(), *from, *to, kind, *promote) {
                return;
            }
        }
        self.result.borrow_mut().push(movement);
    }
}
