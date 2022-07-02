use crate::{
    piece::{Color, Kind},
    position::bitboard11::{BitBoard, Square},
};

use super::power::{lance_power, power};

pub fn reachable(
    black_pieces: BitBoard,
    white_pieces: BitBoard,
    color: Color,
    pos: Square,
    kind: Kind,
) -> BitBoard {
    let mask = reachable_sub(black_pieces | white_pieces, color, pos, kind);
    match color {
        Color::Black => mask & !black_pieces,
        Color::White => mask & !white_pieces,
    }
}

fn reachable_sub(occupied: BitBoard, color: Color, pos: Square, kind: Kind) -> BitBoard {
    match kind {
        Kind::Lance => lance_reachable(occupied, color, pos),
        Kind::Bishop => bishop_reachable(occupied, pos),
        Kind::Rook => rook_reachable(occupied, pos),
        Kind::ProBishop => power(color, pos, Kind::King) | bishop_reachable(occupied, pos),
        Kind::ProRook => power(color, pos, Kind::King) | rook_reachable(occupied, pos),
        _ => power(color, pos, kind),
    }
}

fn lance_reachable(occupied: BitBoard, color: Color, pos: Square) -> BitBoard {
    let power = lance_power(color, pos);
    let block = occupied & power;
    if block.is_empty() {
        return power;
    }
    BitBoard::from_u128(match color {
        Color::Black => (1 << pos.index()) - ((block.x + 1).next_power_of_two() >> 1),
        Color::White => ((block.x - 1) ^ block.x) & power.x,
    })
}

fn bishop_reachable(occupied: BitBoard, pos: Square) -> BitBoard {
    todo!()
}

fn rook_reachable(occupied: BitBoard, pos: Square) -> BitBoard {
    todo!()
}
