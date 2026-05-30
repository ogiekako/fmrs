use crate::piece::{Color, Kind};

use super::{
    bitboard::{self},
    position::PositionAux,
    rule, Square, UndoMove,
};

pub fn previous(position: &mut PositionAux, allow_drop_pawn: bool, movements: &mut Vec<UndoMove>) {
    previous_with_digest(position, allow_drop_pawn, |movement, _digest| {
        movements.push(movement);
    });
}

pub fn previous_with_digest<F: FnMut(UndoMove, u64)>(
    position: &mut PositionAux,
    allow_drop_pawn: bool,
    f: F,
) {
    let mut ctx = Context::new(position, allow_drop_pawn, f);
    ctx.previous();
}

struct Context<'a, F> {
    position: &'a mut PositionAux,
    allow_drop_pawn: bool,
    turn: Color,
    f: F,
    pawn_mask: usize,
}

impl<'a, F: FnMut(UndoMove, u64)> Context<'a, F> {
    fn new(position: &'a mut PositionAux, allow_drop_pawn: bool, f: F) -> Self {
        let turn = position.turn();
        let mut pawn_mask = 0;
        for pos in position.bitboard(turn, Kind::Pawn) {
            pawn_mask |= 1 << pos.col()
        }

        Self {
            position,
            allow_drop_pawn,
            turn,
            f,
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

    #[inline]
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
impl<F: FnMut(UndoMove, u64)> Context<'_, F> {
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
        let digest = self.position.undo_digest(&movement);
        (self.f)(movement, digest);
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

    /// `previous()` must be a pure function of `(position core, allow_drop_pawn)`:
    /// repeated calls from the same core return the same `Vec<UndoMove>` in the
    /// same order. The CandRef refactor (storing `(frontier_idx, undo1_idx,
    /// undo2_idx)` instead of materialised `Position`) relies on this so a
    /// candidate can be reconstructed in Phase 2 without ever materialising it
    /// in Phase 1.
    #[test]
    fn previous_is_deterministic_2ply() {
        let sfens = [
            // Backward-search seed used in backward_search_seed_max_step_5.
            "4k4/4+N4/9/9/9/4L4/9/9/9 w 2r2b4g4s3n3l18p 1",
            // Hand-rich position to exercise the UnDrop / capture branches.
            "8p/8O/9/9/9/7O1/7O+p/7OO/9 b RBGS 1",
            // Position with promoted captures available.
            "4k4/9/9/9/9/9/9/9/4K4 b 2r2b4g4s4n4l18p 1",
        ];

        for sfen in sfens {
            let p0 = PositionAux::from_sfen(sfen).unwrap();

            // Round 1: enumerate undo1 from a fresh clone.
            let mut p0a = p0.clone();
            let mut undo1_a = vec![];
            previous(&mut p0a, true, &mut undo1_a);

            // Round 2: same enumeration must yield identical results.
            let mut p0b = p0.clone();
            let mut undo1_b = vec![];
            previous(&mut p0b, true, &mut undo1_b);
            assert_eq!(
                undo1_a, undo1_b,
                "previous(1st ply) not deterministic for sfen={}",
                sfen
            );

            // For each undo1[i1], enumerating undo2 from the reconstructed q1
            // twice must also be deterministic, and the q2 digest must depend
            // only on (sfen, i1, i2).
            for (i1, m1) in undo1_a.iter().enumerate() {
                let mut q1a = p0.clone();
                q1a.undo_move(m1);
                let mut undo2_a = vec![];
                previous(&mut q1a, true, &mut undo2_a);

                let mut q1b = p0.clone();
                q1b.undo_move(m1);
                let mut undo2_b = vec![];
                previous(&mut q1b, true, &mut undo2_b);

                assert_eq!(q1a.digest(), q1b.digest(), "q1 digest differs i1={}", i1);
                assert_eq!(
                    undo2_a, undo2_b,
                    "previous(2nd ply) not deterministic sfen={} i1={}",
                    sfen, i1
                );

                for (i2, m2) in undo2_a.iter().enumerate() {
                    let mut q2a = q1a.clone();
                    q2a.undo_move(m2);
                    let mut q2b = q1b.clone();
                    q2b.undo_move(m2);
                    assert_eq!(
                        q2a.digest(),
                        q2b.digest(),
                        "q2 digest differs sfen={} i1={} i2={}",
                        sfen,
                        i1,
                        i2
                    );
                }
            }
        }
    }
}
