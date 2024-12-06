use super::Square;

const M: [u64; 81] = {
    let mut m = [0u64; 81];
    m[0] = 0x9d39247e33776d41;
    // MMIX
    let mut i = 1;
    while i < 81 {
        m[i] = m[i - 1]
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        i += 1;
    }
    m
};

pub(crate) fn zobrist(pos: Square) -> u64 {
    M[pos.index()]
}
