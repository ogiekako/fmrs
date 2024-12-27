use rand::{rngs::StdRng, Rng, SeedableRng};

use crate::piece::{Color, Kind};

use super::Square;

const W: usize = 256 * 2 * 16;
lazy_static! {
    static ref M: [u64; W] = {
        let mut res = [0; W];
        let mut rng = StdRng::seed_from_u64(202412141622);
        for i in 0..W {
            res[i] = rng.gen();
        }
        res
    };
}

pub(crate) fn zobrist(color: Color, pos: Square, kind: Kind) -> u64 {
    M[pos.index() << 5 | color.index() << 4 | kind.index()]
}

pub(crate) fn zobrist_hand(color: Color, kind: Kind, n: usize) -> u64 {
    if n == 0 {
        return 0;
    }
    debug_assert!(82 + n < 255);
    M[(82 + n) << 5 | color.index() << 4 | kind.index()]
}

pub(crate) fn zobrist_turn() -> u64 {
    M[W - 1]
}

pub(crate) fn zobrist_pawn_drop() -> u64 {
    M[W - 2]
}
