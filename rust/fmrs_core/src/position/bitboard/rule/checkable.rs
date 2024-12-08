use crate::{
    piece::{Color, Kind},
    position::{
        advance::maybe_legal_movement,
        bitboard::{rule::power, BitBoard},
        Movement, Square,
    },
};

// Returns a bitboard. A square is contained in the bitboard if a non linear piece in the square
// can check the king in one move.
pub fn chekable_non_linear_piece(white_king_pos: Square) -> BitBoard {
    CHECKABLE_NON_LINEAR_PIECE[white_king_pos.index()]
}

lazy_static! {
    pub static ref CHECKABLE_NON_LINEAR_PIECE: [BitBoard; 81] = {
        let mut res = [BitBoard::default(); 81];
        for i in 0..81 {
            res[i] = chekable_non_linear_piece_slow(Square::from_index(i));
        }
        res
    };
}

fn chekable_non_linear_piece_slow(white_king_pos: Square) -> BitBoard {
    let mut res = BitBoard::default();
    for (source_kind, dest_kind) in Kind::iter_transitions() {
        if source_kind.is_line_piece() {
            continue;
        }
        let promote = source_kind != dest_kind;

        for dest_pos in power(Color::White, white_king_pos, dest_kind) {
            for source_pos in power(Color::White, dest_pos, source_kind) {
                if maybe_legal_movement(
                    Color::Black,
                    &Movement::Move {
                        source: source_pos,
                        dest: dest_pos,
                        promote,
                    },
                    source_kind,
                    0,
                ) {
                    res.set(source_pos);
                }
            }
        }
    }
    res
}
