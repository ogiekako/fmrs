use rand::{rngs::StdRng, Rng, SeedableRng};

use crate::piece::{Color, Kind};

use super::Square;

const W: usize = 128 * 2 * 16;
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

pub(crate) fn zobrist_stone(pos: Square) -> u64 {
    M[pos.index() << 5 | 15]
}
