trait PositionExt {
    fn do_move(&mut self, m: &Movement) -> UndoToken;
    fn undo(&mut self, token: &UndoToken) -> Movement;
    fn move_candidates(&self, res: &mut Vec<Movement>) -> anyhow::Result<()>;   
}

impl PositionExt for Position {
    fn do_move(&mut self, m: &Movement) -> UndoToken {
        let c = self.turn;
        let token;
        match m {
            Movement::Drop(pos, k) => {
                let (pos, k) = (*pos, *k);
                self.hands.remove(c, k);
                self.set(pos, c, k);
                token = UndoToken::UnDrop((pos, self.pawn_drop));
                self.pawn_drop = k == Kind::Pawn;
            }
            Movement::Move { from, to, promote } => {
                let (from, to, promote) = (*from, *to, *promote);
                // TODO: return error instead of unwrapping.
                let k = self.kind(from).unwrap();
                self.unset(from, c, k);
                let mut capture = None;
                if self.color_bb[c.opposite().index()].get(to) {
                    for capt_k in Kind::iter() {
                        if self.kind_bb[capt_k.index()].get(to) {
                            self.unset(to, c.opposite(), capt_k);
                            self.hands.add(c, capt_k.maybe_unpromote());
                            capture = Some(capt_k);
                            break;
                        }
                    }
                }
                if promote {
                    self.set(to, c, k.promote().unwrap());
                } else {
                    self.set(to, c, k);
                }
                token = UndoToken::UnMove {
                    from,
                    to,
                    promote,
                    capture,
                    pawn_drop: self.pawn_drop,
                };
                self.pawn_drop = false;
            }
        }
        self.turn = c.opposite();
        token   
    }
}