use crate::piece::{Color, Kind};

use super::{BitBoard, Square};

pub fn attacks_from(pos: Square, c: Color, k: Kind) -> BitBoard {
    todo!()
}

const PAWN_ATTACK: [u128; 2] = [];

fn generate_attack(shifts: &[isize]) -> u128 {
    
}

fn shift(col: isize, row: isize) -> isize {
    col * 13 + row
}
