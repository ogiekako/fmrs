use std::collections::HashMap;

use crate::piece::{Color, Kind};

use super::{
    bitboard::BitBoard,
    position_ext::{generate_attack_preventing_moves, has_pawn_in_col, movable, pinned},
    Movement, Position, Square,
};

pub struct Checker {
    position: Position,
    pinned: Option<HashMap<Square, BitBoard>>,
    black_attack_prevent_moves: Option<Vec<Movement>>,
}

impl Checker {
    pub fn new(position: Position) -> Self {
        let turn = position.turn;
        let pinned = position
            .king(turn)
            .map(|king_pos| pinned(&position, king_pos, turn));
        // If black king is checked, it must be stopped.
        let black_attack_prevent_moves = {
            let mut black_attack_prevent_moves = None;
            if turn == Color::Black {
                if let Some(black_king_pos) = position.king(Color::Black) {
                    let attackers: Vec<_> = position
                        .attackers_to(black_king_pos, Color::White)
                        .collect();
                    if !attackers.is_empty() {
                        let mut moves = vec![];
                        generate_attack_preventing_moves(
                            &position,
                            &mut moves,
                            Color::Black,
                            black_king_pos,
                            attackers,
                        )
                        .unwrap();
                        moves.sort();
                        black_attack_prevent_moves = Some(moves);
                    }
                }
            }
            black_attack_prevent_moves
        };
        Self {
            position,
            pinned,
            black_attack_prevent_moves,
        }
    }

    pub fn is_allowed(&self, m: Movement) -> bool {
        let turn = self.position.turn;

        if let Some(allowed_moves) = &self.black_attack_prevent_moves {
            if allowed_moves.binary_search(&m).is_err() {
                return false;
            }
        }
        match m {
            Movement::Drop(pos, k) => {
                if !movable(pos, turn, k) {
                    return false;
                }
                if k == Kind::Pawn {
                    if has_pawn_in_col(&self.position, pos, turn) {
                        return false;
                    }
                }
                return true;
            }
            Movement::Move { from, to, promote } => {
                let k = self.position.get(from).unwrap().1;
                if !promote && !movable(to, turn, k) {
                    return false;
                }
                if promote {
                    if !super::rule::promotable(from, turn) && !super::rule::promotable(to, turn) {
                        return false;
                    }
                }
                if let Some(mask) = self.pinned.as_ref().and_then(|x| x.get(&from)) {
                    if !mask.get(to) {
                        return false;
                    }
                }
                if k == Kind::King {
                    if self
                        .position
                        .attackers_to_with_king(to, turn.opposite())
                        .next()
                        .is_some()
                    {
                        return false;
                    }
                }
                return true;
            }
        };
    }
}
