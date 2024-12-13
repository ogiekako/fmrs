use crate::piece::{Color, Kind};

use super::Square;

const W: usize = 128 * 2 * 16;
const M: [u64; W] = {
    let mut m = [0u64; W];
    m[0] = 0x9d39247e33776d41;
    // MMIX
    let mut i = 1;
    while i < W {
        m[i] = m[i - 1]
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        i += 1;
    }
    i = 0;
    while i < W {
        m[i] = m[i].rotate_left(20);
        i += 1;
    }
    m
};

pub(crate) fn zobrist(color: Color, pos: Square, kind: Kind) -> u64 {
    M[pos.index() << 5 | color.index() << 4 | kind.index()]
}
