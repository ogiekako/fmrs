//! 3x27-bit rotated bitboard implementation ported from shtsume's `bitboard.c` & `global.c`.

use crate::shtsume::types::{
    GFU, GGI, GHI, GKA, GKE, GKI, GKY, GNG, GNK, GNY, GOU, GRY, GTO, GUM, N_SQUARE, SFU, SGI, SHI,
    SKA, SKE, SKI, SKY, SNG, SNK, SNY, SOU, SRY, STO, SUM,
};
use rand::{rngs::StdRng, Rng, SeedableRng};

/// 3x27-bit bitboard matching shtsume's `bitboard_t` (`uint32_t pos[3]`).
/// Bits 0..27 of `0[0]` = ranks 0..2 (sq 0..26),
/// Bits 0..27 of `0[1]` = ranks 3..5 (sq 27..53),
/// Bits 0..27 of `0[2]` = ranks 6..8 (sq 54..80).
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct SBitBoard(pub [u32; 3]);

pub const BB_EMPTY: SBitBoard = SBitBoard([0, 0, 0]);
pub const BB_FULL: SBitBoard = SBitBoard([0x07ff_ffff, 0x07ff_ffff, 0x07ff_ffff]);

impl SBitBoard {
    #[inline(always)]
    pub const fn is_empty(self) -> bool {
        (self.0[0] | self.0[1] | self.0[2]) == 0
    }

    #[inline(always)]
    pub const fn is_nonzero(self) -> bool {
        (self.0[0] | self.0[1] | self.0[2]) != 0
    }

    #[inline(always)]
    pub const fn test(self, sq: usize) -> bool {
        (self.0[sq / 27] & (1u32 << (sq % 27))) != 0
    }

    #[inline(always)]
    pub fn set(&mut self, sq: usize) {
        self.0[sq / 27] |= 1u32 << (sq % 27);
    }

    #[inline(always)]
    pub fn xor_sq(&mut self, sq: usize) {
        self.0[sq / 27] ^= 1u32 << (sq % 27);
    }

    #[inline(always)]
    pub const fn and(self, rhs: Self) -> Self {
        Self([
            self.0[0] & rhs.0[0],
            self.0[1] & rhs.0[1],
            self.0[2] & rhs.0[2],
        ])
    }

    #[inline(always)]
    pub const fn or(self, rhs: Self) -> Self {
        Self([
            self.0[0] | rhs.0[0],
            self.0[1] | rhs.0[1],
            self.0[2] | rhs.0[2],
        ])
    }

    #[inline(always)]
    pub const fn xor(self, rhs: Self) -> Self {
        Self([
            self.0[0] ^ rhs.0[0],
            self.0[1] ^ rhs.0[1],
            self.0[2] ^ rhs.0[2],
        ])
    }

    #[inline(always)]
    pub const fn and_not(self, rhs: Self) -> Self {
        Self([
            self.0[0] & !rhs.0[0],
            self.0[1] & !rhs.0[1],
            self.0[2] & !rhs.0[2],
        ])
    }

    #[inline(always)]
    pub fn or_assign(&mut self, rhs: Self) {
        self.0[0] |= rhs.0[0];
        self.0[1] |= rhs.0[1];
        self.0[2] |= rhs.0[2];
    }

    #[inline(always)]
    pub fn and_assign(&mut self, rhs: Self) {
        self.0[0] &= rhs.0[0];
        self.0[1] &= rhs.0[1];
        self.0[2] &= rhs.0[2];
    }

    #[inline(always)]
    pub fn xor_assign(&mut self, rhs: Self) {
        self.0[0] ^= rhs.0[0];
        self.0[1] ^= rhs.0[1];
        self.0[2] ^= rhs.0[2];
    }

    #[inline(always)]
    pub fn and_not_assign(&mut self, rhs: Self) {
        self.0[0] &= !rhs.0[0];
        self.0[1] &= !rhs.0[1];
        self.0[2] &= !rhs.0[2];
    }

    #[inline(always)]
    pub fn pop_lsb(&mut self) -> Option<usize> {
        if self.0[0] != 0 {
            let tz = self.0[0].trailing_zeros() as usize;
            self.0[0] &= self.0[0] - 1;
            return Some(tz);
        }
        if self.0[1] != 0 {
            let tz = self.0[1].trailing_zeros() as usize;
            self.0[1] &= self.0[1] - 1;
            return Some(27 + tz);
        }
        if self.0[2] != 0 {
            let tz = self.0[2].trailing_zeros() as usize;
            self.0[2] &= self.0[2] - 1;
            return Some(54 + tz);
        }
        None
    }

    #[inline(always)]
    pub fn min_pos(self) -> Option<usize> {
        if self.0[0] != 0 {
            return Some(self.0[0].trailing_zeros() as usize);
        }
        if self.0[1] != 0 {
            return Some(27 + self.0[1].trailing_zeros() as usize);
        }
        if self.0[2] != 0 {
            return Some(54 + self.0[2].trailing_zeros() as usize);
        }
        None
    }
}

/// Diagonal bit indices in rotated bitboards (exact match with `st_rindex` and `st_lindex` in `bitboard.c`).
pub const RINDEX: [usize; N_SQUARE] = [
    0, 1, 3, 6, 10, 15, 21, 28, 36, 2, 4, 7, 11, 16, 22, 29, 37, 45, 5, 8, 12, 17, 23, 30, 38, 46,
    53, 9, 13, 18, 24, 31, 39, 47, 54, 60, 14, 19, 25, 32, 40, 48, 55, 61, 66, 20, 26, 33, 41, 49,
    56, 62, 67, 71, 27, 34, 42, 50, 57, 63, 68, 72, 75, 35, 43, 51, 58, 64, 69, 73, 76, 78, 44, 52,
    59, 65, 70, 74, 77, 79, 80,
];

pub const LINDEX: [usize; N_SQUARE] = [
    36, 28, 21, 15, 10, 6, 3, 1, 0, 45, 37, 29, 22, 16, 11, 7, 4, 2, 53, 46, 38, 30, 23, 17, 12, 8,
    5, 60, 54, 47, 39, 31, 24, 18, 13, 9, 66, 61, 55, 48, 40, 32, 25, 19, 14, 71, 67, 62, 56, 49,
    41, 33, 26, 20, 75, 72, 68, 63, 57, 50, 42, 34, 27, 78, 76, 73, 69, 64, 58, 51, 43, 35, 80, 79,
    77, 74, 70, 65, 59, 52, 44,
];

const KA_INDEX: [usize; N_SQUARE] = [
    0, 1, 2, 3, 5, 7, 9, 13, 17, 21, 25, 33, 41, 49, 57, 65, 81, 97, 113, 129, 145, 161, 193, 225,
    257, 289, 321, 353, 385, 449, 513, 577, 641, 705, 769, 833, 897, 1025, 1153, 1281, 1409, 1537,
    1665, 1793, 1921, 2049, 2113, 2177, 2241, 2305, 2369, 2433, 2497, 2561, 2593, 2625, 2657, 2689,
    2721, 2753, 2785, 2801, 2817, 2833, 2849, 2865, 2881, 2889, 2897, 2905, 2913, 2921, 2925, 2929,
    2933, 2937, 2939, 2941, 2943, 2944, 2945,
];

const HV_WORD: [usize; 9] = [0, 0, 0, 1, 1, 1, 2, 2, 2];
const HV_SHIFT: [u32; 9] = [1, 10, 19, 1, 10, 19, 1, 10, 19];

const RL_WORD: [usize; 17] = [0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 2, 2, 2, 2, 2, 2, 2];
const RL_SHIFT: [u32; 17] = [0, 0, 4, 7, 11, 16, 22, 2, 10, 19, 0, 7, 13, 18, 22, 0, 0];
const RL_MASK: [u32; 17] = [
    0, 0, 0x01, 0x03, 0x07, 0x0f, 0x1f, 0x3f, 0x7f, 0x3f, 0x1f, 0x0f, 0x07, 0x03, 0x01, 0, 0,
];

/// Right-down diagonal ID (`0..17`, matches `g_rslp` in `global.c`): `file + rank`.
#[inline(always)]
pub const fn rslp(sq: usize) -> usize {
    (sq % 9) + (sq / 9)
}

/// Left-down diagonal ID (`0..17`, matches `g_lslp` in `global.c`): `8 - file + rank`.
#[inline(always)]
pub const fn lslp(sq: usize) -> usize {
    8 - (sq % 9) + (sq / 9)
}

pub struct Tables {
    pub bpos: [SBitBoard; N_SQUARE],
    pub vpos: [SBitBoard; N_SQUARE],
    pub rpos: [SBitBoard; N_SQUARE],
    pub lpos: [SBitBoard; N_SQUARE],

    pub eff_sfu: [SBitBoard; N_SQUARE],
    pub eff_gfu: [SBitBoard; N_SQUARE],
    pub eff_ske: [SBitBoard; N_SQUARE],
    pub eff_gke: [SBitBoard; N_SQUARE],
    pub eff_sgi: [SBitBoard; N_SQUARE],
    pub eff_ggi: [SBitBoard; N_SQUARE],
    pub eff_ski: [SBitBoard; N_SQUARE],
    pub eff_gki: [SBitBoard; N_SQUARE],
    pub eff_ou: [SBitBoard; N_SQUARE],

    pub eff_sky: Vec<[SBitBoard; 128]>,
    pub eff_gky: Vec<[SBitBoard; 128]>,
    pub eff_hhi: Vec<[SBitBoard; 128]>,
    pub eff_vhi: Vec<[SBitBoard; 128]>,
    pub eff_rka: Vec<SBitBoard>,
    pub eff_lka: Vec<SBitBoard>,

    /// Line masks for pin / discovered check filtering:
    /// 0 = all squares, 1..=9 = rank 0..8, 10..=18 = file 0..8,
    /// 19..=35 = lslp 0..16, 36..=52 = rslp 0..16.
    pub line_mask: [SBitBoard; 53],

    /// Promotion zone masks for Sente (ranks 0..3) and Gote (ranks 6..9).
    pub prom_zone: [SBitBoard; 2],

    /// Zobrist seeds: `zkey_seed[koma * 81 + sq]`, plus `zkey_turn`.
    pub zkey_seed: Vec<u64>,
    pub zkey_turn: u64,
}

lazy_static! {
    pub static ref TABLES: Tables = Tables::build();
}

fn ray_bb(sq: usize, df: isize, dr: isize, occ: SBitBoard) -> SBitBoard {
    let mut bb = BB_EMPTY;
    let mut f = (sq % 9) as isize + df;
    let mut r = (sq / 9) as isize + dr;
    while (0..9).contains(&f) && (0..9).contains(&r) {
        let p = (r as usize) * 9 + (f as usize);
        bb.set(p);
        if occ.test(p) {
            break;
        }
        f += df;
        r += dr;
    }
    bb
}

impl Tables {
    fn build() -> Self {
        let mut bpos = [BB_EMPTY; N_SQUARE];
        let mut vpos = [BB_EMPTY; N_SQUARE];
        let mut rpos = [BB_EMPTY; N_SQUARE];
        let mut lpos = [BB_EMPTY; N_SQUARE];

        for sq in 0..N_SQUARE {
            bpos[sq].set(sq);
            let f = sq % 9;
            let r = sq / 9;
            vpos[sq].set(f * 9 + r);
            rpos[sq].set(RINDEX[sq]);
            lpos[sq].set(LINDEX[sq]);
        }

        // Step-piece attack tables
        let mut eff_sfu = [BB_EMPTY; N_SQUARE];
        let mut eff_gfu = [BB_EMPTY; N_SQUARE];
        let mut eff_ske = [BB_EMPTY; N_SQUARE];
        let mut eff_gke = [BB_EMPTY; N_SQUARE];
        let mut eff_sgi = [BB_EMPTY; N_SQUARE];
        let mut eff_ggi = [BB_EMPTY; N_SQUARE];
        let mut eff_ski = [BB_EMPTY; N_SQUARE];
        let mut eff_gki = [BB_EMPTY; N_SQUARE];
        let mut eff_ou = [BB_EMPTY; N_SQUARE];

        let add_steps = |sq: usize, deltas: &[(isize, isize)]| -> SBitBoard {
            let mut bb = BB_EMPTY;
            let f = (sq % 9) as isize;
            let r = (sq / 9) as isize;
            for &(df, dr) in deltas {
                let nf = f + df;
                let nr = r + dr;
                if (0..9).contains(&nf) && (0..9).contains(&nr) {
                    bb.set((nr as usize) * 9 + (nf as usize));
                }
            }
            bb
        };

        for sq in 0..N_SQUARE {
            eff_sfu[sq] = add_steps(sq, &[(0, -1)]);
            eff_gfu[sq] = add_steps(sq, &[(0, 1)]);
            eff_ske[sq] = add_steps(sq, &[(-1, -2), (1, -2)]);
            eff_gke[sq] = add_steps(sq, &[(-1, 2), (1, 2)]);
            eff_sgi[sq] = add_steps(sq, &[(-1, -1), (0, -1), (1, -1), (-1, 1), (1, 1)]);
            eff_ggi[sq] = add_steps(sq, &[(-1, 1), (0, 1), (1, 1), (-1, -1), (1, -1)]);
            eff_ski[sq] = add_steps(sq, &[(-1, -1), (0, -1), (1, -1), (-1, 0), (1, 0), (0, 1)]);
            eff_gki[sq] = add_steps(sq, &[(-1, 1), (0, 1), (1, 1), (-1, 0), (1, 0), (0, -1)]);
            eff_ou[sq] = add_steps(
                sq,
                &[
                    (-1, -1),
                    (0, -1),
                    (1, -1),
                    (-1, 0),
                    (1, 0),
                    (-1, 1),
                    (0, 1),
                    (1, 1),
                ],
            );
        }

        // Sliding attack tables: horizontal rook, vertical rook, lances
        let mut eff_hhi = vec![[BB_EMPTY; 128]; N_SQUARE];
        let mut eff_vhi = vec![[BB_EMPTY; 128]; N_SQUARE];
        let mut eff_sky = vec![[BB_EMPTY; 128]; N_SQUARE];
        let mut eff_gky = vec![[BB_EMPTY; 128]; N_SQUARE];

        for sq in 0..N_SQUARE {
            let f = sq % 9;
            let r = sq / 9;
            for occ_bits in 0..128usize {
                // Horizontal occupancy along rank `r` (files 1..=7)
                let mut h_occ = BB_EMPTY;
                for i in 0..7 {
                    if (occ_bits & (1 << i)) != 0 {
                        h_occ.set(r * 9 + (i + 1));
                    }
                }
                eff_hhi[sq][occ_bits] = ray_bb(sq, -1, 0, h_occ).or(ray_bb(sq, 1, 0, h_occ));

                // Vertical occupancy along file `f` (ranks 1..=7)
                let mut v_occ = BB_EMPTY;
                for i in 0..7 {
                    if (occ_bits & (1 << i)) != 0 {
                        v_occ.set((i + 1) * 9 + f);
                    }
                }
                eff_sky[sq][occ_bits] = ray_bb(sq, 0, -1, v_occ);
                eff_gky[sq][occ_bits] = ray_bb(sq, 0, 1, v_occ);
                eff_vhi[sq][occ_bits] = eff_sky[sq][occ_bits].or(eff_gky[sq][occ_bits]);
            }
        }

        // Diagonal bishop tables (matching `create_ka_effect` in `bitboard.c`)
        let mut eff_rka = vec![BB_EMPTY; 2946];
        let mut eff_lka = vec![BB_EMPTY; 2946];

        // Build inverse maps from rotated bit index to square
        let mut r_inv = [0usize; N_SQUARE];
        let mut l_inv = [0usize; N_SQUARE];
        for sq in 0..N_SQUARE {
            r_inv[RINDEX[sq]] = sq;
            l_inv[LINDEX[sq]] = sq;
        }

        for sq in 0..N_SQUARE {
            // Right-down diagonal (rslp = f + r, direction (-1,+1) and (+1,-1))
            let rs = rslp(sq);
            let r_mask = RL_MASK[rs];
            let r_base = KA_INDEX[RINDEX[sq]];
            let r_word = RL_WORD[rs];
            let r_shift = RL_SHIFT[rs];
            let count = (r_mask as usize) + 1;
            for occ_bits in 0..count {
                let mut occ = BB_EMPTY;
                for b in 0..7u32 {
                    if (occ_bits as u32 & (1 << b)) != 0 {
                        let bit_idx = r_word * 27 + (r_shift + b) as usize;
                        if bit_idx < N_SQUARE {
                            occ.set(r_inv[bit_idx]);
                        }
                    }
                }
                eff_rka[r_base + occ_bits] = ray_bb(sq, 1, -1, occ).or(ray_bb(sq, -1, 1, occ));
            }

            // Left-down diagonal (lslp = 8 - f + r, direction (-1,-1) and (+1,+1))
            let ls = lslp(sq);
            let l_mask = RL_MASK[ls];
            let l_base = KA_INDEX[LINDEX[sq]];
            let l_word = RL_WORD[ls];
            let l_shift = RL_SHIFT[ls];
            let l_count = (l_mask as usize) + 1;
            for occ_bits in 0..l_count {
                let mut occ = BB_EMPTY;
                for b in 0..7u32 {
                    if (occ_bits as u32 & (1 << b)) != 0 {
                        let bit_idx = l_word * 27 + (l_shift + b) as usize;
                        if bit_idx < N_SQUARE {
                            occ.set(l_inv[bit_idx]);
                        }
                    }
                }
                eff_lka[l_base + occ_bits] = ray_bb(sq, -1, -1, occ).or(ray_bb(sq, 1, 1, occ));
            }
        }

        // Line masks for pin/discovered check
        let mut line_mask = [BB_EMPTY; 53];
        line_mask[0] = BB_FULL;
        for sq in 0..N_SQUARE {
            let f = sq % 9;
            let r = sq / 9;
            line_mask[1 + r].set(sq);
            line_mask[10 + f].set(sq);
            line_mask[19 + lslp(sq)].set(sq);
            line_mask[36 + rslp(sq)].set(sq);
        }

        let mut prom_sente = BB_EMPTY;
        let mut prom_gote = BB_EMPTY;
        for sq in 0..27 {
            prom_sente.set(sq);
        }
        for sq in 54..81 {
            prom_gote.set(sq);
        }

        // Deterministic Zobrist seeds
        let mut rng = StdRng::seed_from_u64(0x2022_0209_5348_5453);
        let mut zkey_seed = vec![0u64; 32 * N_SQUARE];
        for v in &mut zkey_seed {
            *v = rng.gen::<u64>();
        }
        let zkey_turn = rng.gen::<u64>();

        Self {
            bpos,
            vpos,
            rpos,
            lpos,
            eff_sfu,
            eff_gfu,
            eff_ske,
            eff_gke,
            eff_sgi,
            eff_ggi,
            eff_ski,
            eff_gki,
            eff_ou,
            eff_sky,
            eff_gky,
            eff_hhi,
            eff_vhi,
            eff_rka,
            eff_lka,
            line_mask,
            prom_zone: [prom_sente, prom_gote],
            zkey_seed,
            zkey_turn,
        }
    }

    #[inline(always)]
    pub fn effect_sky(&self, sq: usize, vocc: SBitBoard) -> SBitBoard {
        let f = sq % 9;
        let idx = ((vocc.0[HV_WORD[f]] >> HV_SHIFT[f]) & 0x7f) as usize;
        self.eff_sky[sq][idx]
    }

    #[inline(always)]
    pub fn effect_gky(&self, sq: usize, vocc: SBitBoard) -> SBitBoard {
        let f = sq % 9;
        let idx = ((vocc.0[HV_WORD[f]] >> HV_SHIFT[f]) & 0x7f) as usize;
        self.eff_gky[sq][idx]
    }

    #[inline(always)]
    pub fn effect_hi(&self, sq: usize, occ: SBitBoard, vocc: SBitBoard) -> SBitBoard {
        let r = sq / 9;
        let f = sq % 9;
        let h_idx = ((occ.0[HV_WORD[r]] >> HV_SHIFT[r]) & 0x7f) as usize;
        let v_idx = ((vocc.0[HV_WORD[f]] >> HV_SHIFT[f]) & 0x7f) as usize;
        self.eff_hhi[sq][h_idx].or(self.eff_vhi[sq][v_idx])
    }

    #[inline(always)]
    pub fn effect_ka(&self, sq: usize, rocc: SBitBoard, locc: SBitBoard) -> SBitBoard {
        let rs = rslp(sq);
        let r_idx = ((rocc.0[RL_WORD[rs]] >> RL_SHIFT[rs]) & RL_MASK[rs]) as usize;
        let ls = lslp(sq);
        let l_idx = ((locc.0[RL_WORD[ls]] >> RL_SHIFT[ls]) & RL_MASK[ls]) as usize;
        self.eff_rka[KA_INDEX[RINDEX[sq]] + r_idx].or(self.eff_lka[KA_INDEX[LINDEX[sq]] + l_idx])
    }

    #[inline(always)]
    pub fn effect_tbl(
        &self,
        sq: usize,
        koma: u8,
        occ: SBitBoard,
        vocc: SBitBoard,
        rocc: SBitBoard,
        locc: SBitBoard,
    ) -> SBitBoard {
        match koma {
            SFU => self.eff_sfu[sq],
            GFU => self.eff_gfu[sq],
            SKY => self.effect_sky(sq, vocc),
            GKY => self.effect_gky(sq, vocc),
            SKE => self.eff_ske[sq],
            GKE => self.eff_gke[sq],
            SGI => self.eff_sgi[sq],
            GGI => self.eff_ggi[sq],
            SKI | STO | SNY | SNK | SNG => self.eff_ski[sq],
            GKI | GTO | GNY | GNK | GNG => self.eff_gki[sq],
            SKA | GKA => self.effect_ka(sq, rocc, locc),
            SHI | GHI => self.effect_hi(sq, occ, vocc),
            SOU | GOU => self.eff_ou[sq],
            SUM | GUM => self.effect_ka(sq, rocc, locc).or(self.eff_ou[sq]),
            SRY | GRY => self.effect_hi(sq, occ, vocc).or(self.eff_ou[sq]),
            _ => BB_EMPTY,
        }
    }
}
