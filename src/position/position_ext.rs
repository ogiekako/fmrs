use crate::piece::{Color, Kind, NUM_KIND};

use super::{bitboard::BitBoard, position::promotable, Movement, Position, UndoToken};

pub trait PositionExt {
    fn do_move(&mut self, m: &Movement) -> UndoToken;
    // fn undo(&mut self, token: &UndoToken) -> Movement;
    fn move_candidates(&self, res: &mut Vec<Movement>) -> anyhow::Result<()>;
    fn checked(&self, c: Color) -> bool;
}

impl PositionExt for Position {
    fn do_move(&mut self, m: &Movement) -> UndoToken {
        let c = self.turn;
        let token;
        match m {
            Movement::Drop(pos, k) => {
                let (pos, k) = (*pos, *k);
                self.hands_mut().remove(c, k);
                self.set(pos, c, k);
                token = UndoToken::UnDrop((pos, self.pawn_drop()));
                self.set_pawn_drop(k == Kind::Pawn);
            }
            Movement::Move { from, to, promote } => {
                let (from, to, promote) = (*from, *to, *promote);
                // TODO: return error instead of unwrapping.
                let k = self.kind(from).unwrap();
                self.unset(from, c, k);
                let mut capture = None;
                if self.bitboard(Some(c.opposite()), None).get(to) {
                    for capt_k in Kind::iter() {
                        if self.bitboard(None, Some(capt_k)).get(to) {
                            self.unset(to, c.opposite(), capt_k);
                            self.hands_mut().add(c, capt_k.maybe_unpromote());
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
                    pawn_drop: self.pawn_drop(),
                };
                self.set_pawn_drop(false);
            }
        }
        self.turn = c.opposite();
        token
    }

    // Generate check/uncheck moves without checking illegality.
    fn move_candidates(&self, res: &mut Vec<Movement>) -> anyhow::Result<()> {
        let occu = self.bitboard(None, None);
        let white_king_pos = self
            .king(Color::White)
            .ok_or(anyhow::anyhow!("White king not found"))?;

        let turn = self.turn;

        // Black.
        if turn == Color::Black {
            // Drop
            for k in self.hands().kinds(turn) {
                for pos in (!occu)
                    & (super::bitboard::movable_positions(occu, white_king_pos, Color::White, k))
                {
                    res.push(Movement::Drop(pos, k));
                }
            }

            let mut direct_attack_goals = [BitBoard::new(); NUM_KIND];
            for k in Kind::iter() {
                direct_attack_goals[k.index()] = !self.bitboard(Some(Color::Black), None)
                    & super::bitboard::movable_positions(occu, white_king_pos, Color::White, k);
            }

            // Direct attack
            for k in Kind::iter() {
                if k == Kind::King {
                    continue;
                }
                let froms = self.bitboard(Some(Color::Black), Some(k));
                let goal = direct_attack_goals[k.index()];
                for from in froms {
                    for to in goal & super::bitboard::movable_positions(occu, from, Color::Black, k)
                    {
                        res.push(Movement::Move {
                            from,
                            to,
                            promote: false,
                        })
                    }
                }
                // promote
                if let Some(k) = k.unpromote() {
                    for from in self.bitboard(Some(Color::Black), Some(k)) {
                        for to in
                            goal & super::bitboard::movable_positions(occu, from, Color::Black, k)
                        {
                            if promotable(from, Color::Black) || promotable(to, Color::Black) {
                                res.push(Movement::Move {
                                    from,
                                    to,
                                    promote: true,
                                });
                            }
                        }
                    }
                }
            }

            // Discovered attack
            for k in vec![Kind::Lance, Kind::Bishop, Kind::Rook] {
                let mut attacker_cands = self.bitboard(Some(Color::Black), Some(k));
                if k != Kind::Lance {
                    attacker_cands |= self.bitboard(Some(Color::Black), Some(k.promote().unwrap()));
                }
                attacker_cands &= super::bitboard::attacks_from(white_king_pos, Color::White, k);
                if attacker_cands.is_empty() {
                    continue;
                }
                let blocker_cands = self.bitboard(Some(Color::Black), None)
                    & super::bitboard::movable_positions(occu, white_king_pos, Color::White, k);
                if blocker_cands.is_empty() {
                    continue;
                }
                for attacker in attacker_cands {
                    if let Some(from) =
                        (super::bitboard::movable_positions(occu, attacker, Color::Black, k)
                            & blocker_cands)
                            .next()
                    {
                        let from_k = self.kind(from).unwrap();
                        for to in
                            (!(super::bitboard::attacks_from(white_king_pos, Color::White, k)
                                & super::bitboard::attacks_from(attacker, Color::Black, k)))
                                & ((!self.bitboard(Some(Color::Black), None))
                                    & super::bitboard::movable_positions(
                                        occu,
                                        from,
                                        Color::Black,
                                        from_k,
                                    ))
                        {
                            if !direct_attack_goals[from_k.index()].get(to) {
                                res.push(Movement::Move {
                                    from,
                                    to,
                                    promote: false,
                                })
                            }

                            if (promotable(from, Color::Black) || promotable(to, Color::Black))
                                && from_k.promote().is_some()
                                && !direct_attack_goals[from_k.promote().unwrap().index()].get(to)
                            {
                                res.push(Movement::Move {
                                    from,
                                    to,
                                    promote: true,
                                })
                            }
                        }
                    }
                }
            }
        } else {
            self.generate_attack_preventing_moves(
                res,
                Color::White,
                white_king_pos,
                self.attackers_to(white_king_pos, Color::Black).collect(),
            )?;
        }
        Ok(())
    }

    fn checked(&self, c: Color) -> bool {
        match self.king(c) {
            Some(king_pos) => self.attackers_to(king_pos, c.opposite()).next().is_some(),
            None => false,
        }
    }
}
