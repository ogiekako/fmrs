use fmrs_core::{
    piece::{Color, Kind},
    position::position::PositionAux,
    solve::standard_solve::standard_solve,
};
use serde::{Deserialize, Serialize};

use super::frame::Frame;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(super) struct MateFormationFilter {
    pub(super) attackers: Vec<Kind>,
    pub(super) no_redundant: bool,
    pub(super) unique: bool,
    pub(super) no_less_pro_pawn: u8,
    pub(super) pawn_maximally_constrained: bool,
}

impl MateFormationFilter {
    pub(super) fn check(&self, frame: &Frame) -> Vec<PositionAux> {
        let mut mate_positions = vec![];

        let room = frame.room.bitboard();
        if room.is_empty() {
            return mate_positions;
        }

        if self.attackers.is_empty() {
            return mate_positions;
        }

        let has_parity = self.attackers.iter().all(|k| *k != Kind::Rook);

        let mut representatives = vec![];

        let frame_position = frame.to_position();

        for king_pos in room {
            if representatives.len() >= 2 || representatives.len() == 1 && !has_parity {
                break;
            }
            let mut position = frame_position.clone();
            position.set(king_pos, Color::WHITE, Kind::King);
            for k in &self.attackers {
                position.hands_mut().add(Color::BLACK, *k);
            }
            representatives.push(position);
        }

        for representative in representatives {
            let mut impossible_max_pawn = 0;
            for bit in [4, 2, 1] {
                let c = impossible_max_pawn | bit;

                let mut position = representative.clone();
                for _ in 0..c {
                    position.hands_mut().add(Color::WHITE, Kind::Pawn);
                }
                let solution = standard_solve(position.clone(), 1, true).unwrap();
                if solution.is_empty() {
                    impossible_max_pawn = c;
                }
            }
            if impossible_max_pawn == 7 {
                continue;
            }
            let min_pawn = impossible_max_pawn + 1;

            let mut position = representative.clone();
            for _ in 0..min_pawn {
                position.hands_mut().add(Color::WHITE, Kind::Pawn);
            }
            let solution = standard_solve(position.clone(), 1, true).unwrap().remove(0);

            let mut black_pawn = frame_position.bitboard(Color::BLACK, Kind::Pawn);
            let mut white_pawn = frame_position.bitboard(Color::WHITE, Kind::Pawn);

            let mut mate_position = position.clone();
            for m in solution {
                mate_position.do_move(&m);
                black_pawn |= mate_position.bitboard(Color::BLACK, Kind::Pawn);
                white_pawn |= mate_position.bitboard(Color::WHITE, Kind::Pawn);
            }

            if self.no_redundant && !mate_position.hands().is_empty(Color::BLACK) {
                continue;
            }
            if (mate_position
                .bitboard(Color::WHITE, Kind::ProPawn)
                .u128()
                .count_ones() as u8)
                < self.no_less_pro_pawn
            {
                continue;
            }
            if self.pawn_maximally_constrained {
                if black_pawn.col_mask().count_ones() != frame.room.width() as u32 {
                    continue;
                }
                if white_pawn.col_mask().count_ones() != frame.room.width() as u32 {
                    continue;
                }
            }

            if !self.unique {
                mate_positions.push(mate_position);
                continue;
            }

            let mut variants = vec![];

            for king_pos in room {
                if has_parity
                    && king_pos.parity()
                        != representative
                            .bitboard(Color::WHITE, Kind::King)
                            .singleton()
                            .parity()
                {
                    continue;
                }

                let mut position = frame_position.clone();
                position.set(king_pos, Color::WHITE, Kind::King);
                for k in &self.attackers {
                    position.hands_mut().add(Color::BLACK, *k);
                }
                for _ in 0..min_pawn {
                    position.hands_mut().add(Color::WHITE, Kind::Pawn);
                }

                variants.push(position);
            }

            let mut unique = true;
            for mut position in variants {
                let Some(solution) = standard_solve(position.clone(), 1, true).unwrap().pop()
                else {
                    continue;
                };
                for m in solution {
                    position.do_move(&m);
                }
                if position.digest() != mate_position.digest() {
                    unique = false;
                    break;
                }
            }
            if unique {
                mate_positions.push(mate_position);
            }
        }

        mate_positions
    }
}
