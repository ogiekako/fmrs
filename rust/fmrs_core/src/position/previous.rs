use crate::piece::{Color, Kind};

use super::{
    bitboard::{self},
    position::PositionAux,
    rule, Square, UndoMove,
};

pub fn previous(position: &mut PositionAux, allow_drop_pawn: bool, movements: &mut Vec<UndoMove>) {
    let mut ctx = Context::new(position, allow_drop_pawn, movements);
    ctx.previous();
}

struct Context<'a> {
    position: &'a mut PositionAux,
    allow_drop_pawn: bool,
    turn: Color,
    movements: &'a mut Vec<UndoMove>,
    pawn_mask: usize,
}

impl<'a> Context<'a> {
    fn new(
        position: &'a mut PositionAux,
        allow_drop_pawn: bool,
        movements: &'a mut Vec<UndoMove>,
    ) -> Self {
        let turn = position.turn();
        let mut pawn_mask = 0;
        for pos in position.bitboard(position.turn(), Kind::Pawn) {
            pawn_mask |= 1 << pos.col();
        }

        Self {
            position,
            allow_drop_pawn,
            turn,
            movements,
            pawn_mask,
        }
    }

    fn previous(&mut self) {
        for kind in Kind::iter() {
            let dests = self.position.bitboard(self.turn.opposite(), kind);
            for dest in dests {
                self.add_undo_moves_to(dest, kind, false);
                self.add_undo_moves_to(dest, kind, true);
            }
        }
    }

    fn add_undo_moves_to(&mut self, dest: Square, kind: Kind, was_pawn_drop: bool) {
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
            let sources = bitboard::reachable(self.position, self.turn, dest, prev_kind, false)
                .and_not(self.position.occupied_bb());
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
impl Context<'_> {
    fn maybe_add_undo_move(&mut self, movement: UndoMove) {
        if let UndoMove::UnMove {
            source: from,
            dest: to,
            promote,
            capture,
            pawn_drop: _,
        } = &movement
        {
            if let Some(capture) = capture {
                if !rule::is_legal_drop(self.position.turn(), *to, *capture, self.pawn_mask) {
                    return;
                }
            }

            let mut kind = self.position.get(*to).unwrap().1;
            if *promote {
                kind = kind.unpromote().unwrap();
                if kind == Kind::Pawn
                    && self
                        .position
                        .col_has_pawn(self.position.turn().opposite(), to.col())
                {
                    return;
                }
            }
            if !rule::is_legal_move(self.position.turn().opposite(), *from, *to, kind, *promote) {
                return;
            }
        }
        self.movements.push(movement);
    }
}

#[cfg(test)]
mod tests {
    use crate::position::{position::PositionAux, previous};

    #[test]
    fn test_previous_no_double_pawn() {
        let mut position = PositionAux::from_sfen("8p/8O/9/9/9/7O1/7O+p/7OO/9 b - 1").unwrap();
        let mut movements = vec![];
        previous(&mut position, false, &mut movements);
        assert_eq!(movements.len(), 2);
    }
}
