//! `PositionBackend` implementation backed by `fmrs` (`PositionAux` + magic bitboards + `advance_aux`).

use std::cell::RefCell;

use smallvec::SmallVec;

use crate::piece::{Color, Kind};
use crate::position::advance::advance::{advance_aux, advance_white_with_attacker};
use crate::position::advance::attack_prevent::{attacker, Attacker};
use crate::position::advance::AdvanceOptions;
use crate::position::bitboard::{
    bishop_reachable, gold_power, king_power, knight_power, lance_reachable, pawn_power, power,
    rook_reachable, silver_power, BitBoard,
};
use crate::position::position::PositionAux;
use crate::position::zobrist::{zobrist, zobrist_raw};
use crate::position::{Movement, Square};
use crate::sfen;
use crate::futsuu::backend::{HsOracle, OnePlyMate, PositionBackend};
use crate::futsuu::ni_cache::{
    mkey_set, pack_ni_mask, pack_ni_mask_cap, req_no_w_lance_col, white_slider_mask_from_bbs,
    BoardPlanes, MAX_HAND, REQ_NO_W_BISH, REQ_NO_W_ROOK,
};
use crate::futsuu::types::{
    KeyState, MKey, Move, MoveChain, MvItem, Turn, DISTANCE, FU, GFU, GGI, GHI, GI, GKA, GKE, GKI,
    GKY, GNG, GNK, GNY, GOU, GRY, GTO, GUM, HI, KA, KE, KI, KY, N_KOMAS, N_SQUARE, SFU, SGI, SHI,
    SKA, SKE, SKI, SKY, SNG, SNK, SNY, SOU, SPC, SRY, STO, SUM,
};

/// Constant XORed into `board_digest()` when it is White's turn so board+turn Zobrist key
/// distinguishes side-to-move while excluding hands.
const TURN_ZOBRIST: u64 = 0x9e37_79b9_7f4a_7c15;
const NO_SQ: u8 = N_SQUARE as u8;

/// Precomputed `shtsume` square index (`rank * 9 + file`) -> `fmrs::Square` (`col * 9 + row`),
/// avoiding `% 9` and `/ 9` integer divisions on the hot path.
static SQ_TO_FMRS: [Square; N_SQUARE] = {
    let mut t = [Square::S11; N_SQUARE];
    let mut sq = 0;
    while sq < N_SQUARE {
        t[sq] = Square::new(sq % 9, sq / 9);
        sq += 1;
    }
    t
};

/// Precomputed `shtsume` square -> `fmrs::Square::index() << 5` for direct Zobrist lookup.
const SQ_ZBASE: [usize; N_SQUARE] = {
    let mut t = [0usize; N_SQUARE];
    let mut sq = 0;
    while sq < N_SQUARE {
        t[sq] = ((sq % 9) * 9 + (sq / 9)) << 5;
        sq += 1;
    }
    t
};

/// Precomputed `shtsume` koma -> `(color_idx << 4) | kind_idx` for direct Zobrist lookup.
const KOMA_CK: [usize; N_KOMAS] = {
    let mut t = [0usize; N_KOMAS];
    t[SFU as usize] = Kind::Pawn as usize;
    t[SKY as usize] = Kind::Lance as usize;
    t[SKE as usize] = Kind::Knight as usize;
    t[SGI as usize] = Kind::Silver as usize;
    t[SKI as usize] = Kind::Gold as usize;
    t[SKA as usize] = Kind::Bishop as usize;
    t[SHI as usize] = Kind::Rook as usize;
    t[SOU as usize] = Kind::King as usize;
    t[STO as usize] = Kind::ProPawn as usize;
    t[SNY as usize] = Kind::ProLance as usize;
    t[SNK as usize] = Kind::ProKnight as usize;
    t[SNG as usize] = Kind::ProSilver as usize;
    t[SUM as usize] = Kind::ProBishop as usize;
    t[SRY as usize] = Kind::ProRook as usize;

    t[GFU as usize] = 16 | (Kind::Pawn as usize);
    t[GKY as usize] = 16 | (Kind::Lance as usize);
    t[GKE as usize] = 16 | (Kind::Knight as usize);
    t[GGI as usize] = 16 | (Kind::Silver as usize);
    t[GKI as usize] = 16 | (Kind::Gold as usize);
    t[GKA as usize] = 16 | (Kind::Bishop as usize);
    t[GHI as usize] = 16 | (Kind::Rook as usize);
    t[GOU as usize] = 16 | (Kind::King as usize);
    t[GTO as usize] = 16 | (Kind::ProPawn as usize);
    t[GNY as usize] = 16 | (Kind::ProLance as usize);
    t[GNK as usize] = 16 | (Kind::ProKnight as usize);
    t[GNG as usize] = 16 | (Kind::ProSilver as usize);
    t[GUM as usize] = 16 | (Kind::ProBishop as usize);
    t[GRY as usize] = 16 | (Kind::ProRook as usize);
    t
};

/// Precomputed `fmrs::Square::index()` (`col * 9 + row`) -> `shtsume` square index (`row * 9 + col`).
const SQ_FROM_FMRS: [u8; N_SQUARE] = {
    let mut t = [0u8; N_SQUARE];
    let mut col = 0;
    while col < 9 {
        let mut row = 0;
        while row < 9 {
            t[col * 9 + row] = (row * 9 + col) as u8;
            row += 1;
        }
        col += 1;
    }
    t
};

#[inline(always)]
fn sq_to_fmrs(sq: usize) -> Square {
    SQ_TO_FMRS[sq]
}

#[inline(always)]
fn sq_from_fmrs(sq: Square) -> usize {
    SQ_FROM_FMRS[sq.index()] as usize
}

/// Convert a `shtsume::Move` into an `fmrs_core::position::Movement`.
#[inline]
pub fn move_to_movement(mv: Move) -> Movement {
    let dest = sq_to_fmrs(mv.dest());
    if mv.is_drop() {
        Movement::Drop(dest, ktype_to_kind(mv.hand_ktype()))
    } else {
        Movement::Move {
            source: sq_to_fmrs(mv.prev_pos as usize),
            dest,
            promote: mv.is_promote(),
            source_kind_hint: None,
            capture_kind_hint: None,
        }
    }
}

/// Convert a `shtsume` principal variation into `fmrs_core` `Movement`s for KIF/SFEN conversion.
pub fn pv_to_movements(pv: &[Move]) -> Vec<Movement> {
    pv.iter().copied().map(move_to_movement).collect()
}

#[inline(always)]
fn ktype_to_kind(ktype: u8) -> Kind {
    match ktype {
        FU => Kind::Pawn,
        KY => Kind::Lance,
        KE => Kind::Knight,
        GI => Kind::Silver,
        KI => Kind::Gold,
        KA => Kind::Bishop,
        HI => Kind::Rook,
        _ => unreachable!("invalid hand ktype {}", ktype),
    }
}

#[inline(always)]
fn kind_to_ktype(kind: Kind) -> u8 {
    match kind {
        Kind::Pawn | Kind::ProPawn => FU,
        Kind::Lance | Kind::ProLance => KY,
        Kind::Knight | Kind::ProKnight => KE,
        Kind::Silver | Kind::ProSilver => GI,
        Kind::Gold => KI,
        Kind::Bishop | Kind::ProBishop => KA,
        Kind::Rook | Kind::ProRook => HI,
        Kind::King => 0,
    }
}

#[inline(always)]
fn kind_color_to_koma(color: Color, kind: Kind) -> u8 {
    match (color, kind) {
        (Color::BLACK, Kind::Pawn) => SFU,
        (Color::BLACK, Kind::Lance) => SKY,
        (Color::BLACK, Kind::Knight) => SKE,
        (Color::BLACK, Kind::Silver) => SGI,
        (Color::BLACK, Kind::Gold) => SKI,
        (Color::BLACK, Kind::Bishop) => SKA,
        (Color::BLACK, Kind::Rook) => SHI,
        (Color::BLACK, Kind::King) => SOU,
        (Color::BLACK, Kind::ProPawn) => STO,
        (Color::BLACK, Kind::ProLance) => SNY,
        (Color::BLACK, Kind::ProKnight) => SNK,
        (Color::BLACK, Kind::ProSilver) => SNG,
        (Color::BLACK, Kind::ProBishop) => SUM,
        (Color::BLACK, Kind::ProRook) => SRY,
        (Color::WHITE, Kind::Pawn) => GFU,
        (Color::WHITE, Kind::Lance) => GKY,
        (Color::WHITE, Kind::Knight) => GKE,
        (Color::WHITE, Kind::Silver) => GGI,
        (Color::WHITE, Kind::Gold) => GKI,
        (Color::WHITE, Kind::Bishop) => GKA,
        (Color::WHITE, Kind::Rook) => GHI,
        (Color::WHITE, Kind::King) => GOU,
        (Color::WHITE, Kind::ProPawn) => GTO,
        (Color::WHITE, Kind::ProLance) => GNY,
        (Color::WHITE, Kind::ProKnight) => GNK,
        (Color::WHITE, Kind::ProSilver) => GNG,
        (Color::WHITE, Kind::ProBishop) => GUM,
        (Color::WHITE, Kind::ProRook) => GRY,
    }
}

#[inline(always)]
fn movement_to_move_and_hints(pos: &PositionAux, side: Color, m: Movement) -> (Move, u8, u8) {
    match m {
        Movement::Drop(to, kind) => (
            Move::drop(kind_to_ktype(kind), SQ_FROM_FMRS[to.index()]),
            0,
            0,
        ),
        Movement::Move {
            source,
            source_kind_hint,
            dest,
            promote,
            capture_kind_hint,
        } => {
            let s_kind = source_kind_hint.unwrap_or_else(|| pos.must_get_kind(source));
            let src_koma = kind_color_to_koma(side, s_kind);
            let cap_koma = match capture_kind_hint.unwrap_or_else(|| pos.get_kind(dest)) {
                Some(ck) => kind_color_to_koma(side.opposite(), ck),
                None => SPC,
            };
            let s_idx = SQ_FROM_FMRS[source.index()];
            let d_idx = SQ_FROM_FMRS[dest.index()];
            let mv = if promote {
                Move::promote(s_idx, d_idx)
            } else {
                Move::normal(s_idx, d_idx)
            };
            (mv, src_koma, cap_koma)
        }
    }
}

#[inline]
fn hands_to_mkey(pos: &PositionAux, color: Color) -> MKey {
    let h = pos.hands();
    MKey::new(
        h.count(color, Kind::Pawn) as u8,
        h.count(color, Kind::Lance) as u8,
        h.count(color, Kind::Knight) as u8,
        h.count(color, Kind::Silver) as u8,
        h.count(color, Kind::Gold) as u8,
        h.count(color, Kind::Bishop) as u8,
        h.count(color, Kind::Rook) as u8,
    )
}

const BB_FULL: u128 = (1u128 << 81) - 1;
const BB_NOT_ROW1: u128 = BB_FULL & !BitBoard::ROW1.u128();
const BB_NOT_ROW9: u128 = BB_FULL & !BitBoard::ROW9.u128();
const BB_NOT_ROW12: u128 = BB_FULL & !(BitBoard::ROW1.u128() | BitBoard::ROW2.u128());
const BB_NOT_ROW89: u128 = BB_FULL & !(BitBoard::ROW8.u128() | BitBoard::ROW9.u128());

/// Vectorized 128-bit bit-parallel attack-count maps over all 81 squares simultaneously.
///
/// Computed in ~15-20 ns via column-major `u128` shifts + slider magic bitboards.
#[derive(Copy, Clone, Debug)]
pub struct FastBoardAttacks {
    pub black_e1: u128,
    pub black_e2: u128,
    pub black_e3: u128,
    pub white_nk_e1: u128,
    pub white_nk_e2: u128,
}

#[inline(always)]
fn add_att3(e1: &mut u128, e2: &mut u128, e3: &mut u128, x: u128) {
    *e3 |= *e2 & x;
    *e2 |= *e1 & x;
    *e1 |= x;
}

#[inline(always)]
fn add_att2(e1: &mut u128, e2: &mut u128, x: u128) {
    *e2 |= *e1 & x;
    *e1 |= x;
}

impl FastBoardAttacks {
    /// Compute only Black (`Color::BLACK`) `(>=1, >=2)` attack maps in ~10 ns for `find_1ply_mate`.
    #[inline]
    pub fn compute_black_e12(p: &PositionAux) -> (u128, u128) {
        let kbb = p.kind_bitboard();
        let occ = p.occupied_bb();
        let b = p.black_bb().u128();
        let goldish = kbb.goldish().u128();
        let psg = kbb.pawn_silver_goldish().u128();
        let silver = kbb.bitboard(Kind::Silver).u128();
        let prom = kbb.promote_layer().u128();
        let bishopish = kbb.bishopish();
        let rookish = kbb.rookish();
        let uma = bishopish.u128() & prom;
        let ryu = rookish.u128() & prom;
        let king = kbb.bitboard(Kind::King).u128();

        let mut e1: u128 = 0;
        let mut e2: u128 = 0;

        let b_n = (psg | uma | king) & b;
        add_att2(&mut e1, &mut e2, (b_n & BB_NOT_ROW1) >> 1);

        let b_swe = (goldish | uma | king) & b;
        add_att2(&mut e1, &mut e2, (b_swe & BB_NOT_ROW9) << 1);
        add_att2(&mut e1, &mut e2, (b_swe << 9) & BB_FULL);
        add_att2(&mut e1, &mut e2, b_swe >> 9);

        let b_dn = (silver | goldish | ryu | king) & b;
        add_att2(&mut e1, &mut e2, ((b_dn & BB_NOT_ROW1) << 8) & BB_FULL);
        add_att2(&mut e1, &mut e2, (b_dn & BB_NOT_ROW1) >> 10);

        let b_ds = (silver | ryu | king) & b;
        add_att2(&mut e1, &mut e2, ((b_ds & BB_NOT_ROW9) << 10) & BB_FULL);
        add_att2(&mut e1, &mut e2, (b_ds & BB_NOT_ROW9) >> 8);

        let b_ke = kbb.bitboard(Kind::Knight).u128() & b & BB_NOT_ROW12;
        if b_ke != 0 {
            add_att2(&mut e1, &mut e2, (b_ke << 7) & BB_FULL);
            add_att2(&mut e1, &mut e2, b_ke >> 11);
        }

        let b_bb = BitBoard::from_u128(b);
        for sq in kbb.bitboard(Kind::Lance) & b_bb {
            add_att2(&mut e1, &mut e2, lance_reachable(occ, Color::BLACK, sq).u128());
        }
        for sq in bishopish & b_bb {
            add_att2(&mut e1, &mut e2, bishop_reachable(occ, sq).u128());
        }
        for sq in rookish & b_bb {
            add_att2(&mut e1, &mut e2, rook_reachable(occ, sq).u128());
        }

        (e1, e2)
    }

    /// Compute Black `(>=1, >=2)` and White non-King `(>=1)` attack masks in ~10 ns
    /// for ultra-fast SEE state & safe-flight extraction.
    #[inline]
    pub fn compute_see_masks(p: &PositionAux) -> (u128, u128, u128) {
        let kbb = p.kind_bitboard();
        let occ = p.occupied_bb();
        let b = p.black_bb().u128();
        let w = p.white_bb().u128();
        let goldish = kbb.goldish().u128();
        let psg = kbb.pawn_silver_goldish().u128();
        let silver = kbb.bitboard(Kind::Silver).u128();
        let knight = kbb.bitboard(Kind::Knight).u128();
        let lance = kbb.bitboard(Kind::Lance);
        let bishopish = kbb.bishopish();
        let rookish = kbb.rookish();
        let prom = kbb.promote_layer().u128();
        let uma = bishopish.u128() & prom;
        let ryu = rookish.u128() & prom;
        let king = kbb.bitboard(Kind::King).u128();

        let mut be1: u128 = 0;
        let mut be2: u128 = 0;

        let b_n = (psg | uma | king) & b;
        add_att2(&mut be1, &mut be2, (b_n & BB_NOT_ROW1) >> 1);

        let b_swe = (goldish | uma | king) & b;
        add_att2(&mut be1, &mut be2, (b_swe & BB_NOT_ROW9) << 1);
        add_att2(&mut be1, &mut be2, (b_swe << 9) & BB_FULL);
        add_att2(&mut be1, &mut be2, b_swe >> 9);

        let b_dn = (silver | goldish | ryu | king) & b;
        add_att2(&mut be1, &mut be2, ((b_dn & BB_NOT_ROW1) << 8) & BB_FULL);
        add_att2(&mut be1, &mut be2, (b_dn & BB_NOT_ROW1) >> 10);

        let b_ds = (silver | ryu | king) & b;
        add_att2(&mut be1, &mut be2, ((b_ds & BB_NOT_ROW9) << 10) & BB_FULL);
        add_att2(&mut be1, &mut be2, (b_ds & BB_NOT_ROW9) >> 8);

        let b_ke = knight & b & BB_NOT_ROW12;
        if b_ke != 0 {
            add_att2(&mut be1, &mut be2, (b_ke << 7) & BB_FULL);
            add_att2(&mut be1, &mut be2, b_ke >> 11);
        }

        let b_bb = BitBoard::from_u128(b);
        for sq in lance & b_bb {
            add_att2(&mut be1, &mut be2, lance_reachable(occ, Color::BLACK, sq).u128());
        }
        for sq in bishopish & b_bb {
            add_att2(&mut be1, &mut be2, bishop_reachable(occ, sq).u128());
        }
        for sq in rookish & b_bb {
            add_att2(&mut be1, &mut be2, rook_reachable(occ, sq).u128());
        }

        let w_s = (psg | uma) & w;
        let w_nwe = (goldish | uma) & w;
        let w_ds = (silver | goldish | ryu) & w;
        let w_dn = (silver | ryu) & w;
        let mut we1: u128 = ((w_s & BB_NOT_ROW9) << 1)
            | ((w_nwe & BB_NOT_ROW1) >> 1)
            | ((w_nwe << 9) & BB_FULL)
            | (w_nwe >> 9)
            | (((w_ds & BB_NOT_ROW9) << 10) & BB_FULL)
            | ((w_ds & BB_NOT_ROW9) >> 8)
            | (((w_dn & BB_NOT_ROW1) << 8) & BB_FULL)
            | ((w_dn & BB_NOT_ROW1) >> 10);

        let w_ke = knight & w & BB_NOT_ROW89;
        if w_ke != 0 {
            we1 |= ((w_ke << 11) & BB_FULL) | (w_ke >> 7);
        }

        let w_bb = BitBoard::from_u128(w);
        for sq in lance & w_bb {
            we1 |= lance_reachable(occ, Color::WHITE, sq).u128();
        }
        for sq in bishopish & w_bb {
            we1 |= bishop_reachable(occ, sq).u128();
        }
        for sq in rookish & w_bb {
            we1 |= rook_reachable(occ, sq).u128();
        }

        debug_assert!({
            let full = Self::compute(p);
            be1 == full.black_e1 && be2 == full.black_e2 && we1 == full.white_nk_e1
        });
        (be1, be2, we1)
    }

    /// Compute full Black `(>=1, >=2, >=3)` and White non-King `(>=1, >=2)` attack maps in ~20 ns.
    #[inline]
    pub fn compute(p: &PositionAux) -> Self {
        let kbb = p.kind_bitboard();
        let occ = p.occupied_bb();
        let b = p.black_bb().u128();
        let w = p.white_bb().u128();
        let goldish = kbb.goldish().u128();
        let psg = kbb.pawn_silver_goldish().u128();
        let silver = kbb.bitboard(Kind::Silver).u128();
        let knight = kbb.bitboard(Kind::Knight).u128();
        let lance = kbb.bitboard(Kind::Lance);
        let bishopish = kbb.bishopish();
        let rookish = kbb.rookish();
        let prom = kbb.promote_layer().u128();
        let uma = bishopish.u128() & prom;
        let ryu = rookish.u128() & prom;
        let king = kbb.bitboard(Kind::King).u128();

        let mut be1: u128 = 0;
        let mut be2: u128 = 0;
        let mut be3: u128 = 0;

        let b_n = (psg | uma | king) & b;
        add_att3(&mut be1, &mut be2, &mut be3, (b_n & BB_NOT_ROW1) >> 1);

        let b_swe = (goldish | uma | king) & b;
        add_att3(&mut be1, &mut be2, &mut be3, (b_swe & BB_NOT_ROW9) << 1);
        add_att3(&mut be1, &mut be2, &mut be3, (b_swe << 9) & BB_FULL);
        add_att3(&mut be1, &mut be2, &mut be3, b_swe >> 9);

        let b_dn = (silver | goldish | ryu | king) & b;
        add_att3(&mut be1, &mut be2, &mut be3, ((b_dn & BB_NOT_ROW1) << 8) & BB_FULL);
        add_att3(&mut be1, &mut be2, &mut be3, (b_dn & BB_NOT_ROW1) >> 10);

        let b_ds = (silver | ryu | king) & b;
        add_att3(&mut be1, &mut be2, &mut be3, ((b_ds & BB_NOT_ROW9) << 10) & BB_FULL);
        add_att3(&mut be1, &mut be2, &mut be3, (b_ds & BB_NOT_ROW9) >> 8);

        let b_ke = knight & b & BB_NOT_ROW12;
        if b_ke != 0 {
            add_att3(&mut be1, &mut be2, &mut be3, (b_ke << 7) & BB_FULL);
            add_att3(&mut be1, &mut be2, &mut be3, b_ke >> 11);
        }

        let b_bb = BitBoard::from_u128(b);
        for sq in lance & b_bb {
            add_att3(&mut be1, &mut be2, &mut be3, lance_reachable(occ, Color::BLACK, sq).u128());
        }
        for sq in bishopish & b_bb {
            add_att3(&mut be1, &mut be2, &mut be3, bishop_reachable(occ, sq).u128());
        }
        for sq in rookish & b_bb {
            add_att3(&mut be1, &mut be2, &mut be3, rook_reachable(occ, sq).u128());
        }

        let mut we1: u128 = 0;
        let mut we2: u128 = 0;

        let w_s = (psg | uma) & w;
        add_att2(&mut we1, &mut we2, (w_s & BB_NOT_ROW9) << 1);

        let w_nwe = (goldish | uma) & w;
        add_att2(&mut we1, &mut we2, (w_nwe & BB_NOT_ROW1) >> 1);
        add_att2(&mut we1, &mut we2, (w_nwe << 9) & BB_FULL);
        add_att2(&mut we1, &mut we2, w_nwe >> 9);

        let w_ds = (silver | goldish | ryu) & w;
        add_att2(&mut we1, &mut we2, ((w_ds & BB_NOT_ROW9) << 10) & BB_FULL);
        add_att2(&mut we1, &mut we2, (w_ds & BB_NOT_ROW9) >> 8);

        let w_dn = (silver | ryu) & w;
        add_att2(&mut we1, &mut we2, ((w_dn & BB_NOT_ROW1) << 8) & BB_FULL);
        add_att2(&mut we1, &mut we2, (w_dn & BB_NOT_ROW1) >> 10);

        let w_ke = knight & w & BB_NOT_ROW89;
        if w_ke != 0 {
            add_att2(&mut we1, &mut we2, (w_ke << 11) & BB_FULL);
            add_att2(&mut we1, &mut we2, w_ke >> 7);
        }

        let w_bb = BitBoard::from_u128(w);
        for sq in lance & w_bb {
            add_att2(&mut we1, &mut we2, lance_reachable(occ, Color::WHITE, sq).u128());
        }
        for sq in bishopish & w_bb {
            add_att2(&mut we1, &mut we2, bishop_reachable(occ, sq).u128());
        }
        for sq in rookish & w_bb {
            add_att2(&mut we1, &mut we2, rook_reachable(occ, sq).u128());
        }

        Self {
            black_e1: be1,
            black_e2: be2,
            black_e3: be3,
            white_nk_e1: we1,
            white_nk_e2: we2,
        }
    }

    #[inline(always)]
    pub fn black_count_at(&self, fsq_idx: usize) -> u8 {
        (((self.black_e1 >> fsq_idx) & 1)
            + ((self.black_e2 >> fsq_idx) & 1)
            + ((self.black_e3 >> fsq_idx) & 1)) as u8
    }

    #[inline(always)]
    pub fn white_nk_count_at(&self, fsq_idx: usize) -> u8 {
        (((self.white_nk_e1 >> fsq_idx) & 1) + ((self.white_nk_e2 >> fsq_idx) & 1)) as u8
    }
}

/// Check if `attacker_color` has any piece in `atk_bb` attacking `sq` under `occ` (~4 ns, zero loops).
#[inline]
pub(crate) fn has_attack_on_masked(
    pos: &PositionAux,
    attacker_color: Color,
    atk_bb: BitBoard,
    occ: BitBoard,
    sq: Square,
    exclude_king: bool,
) -> bool {
    let opp = attacker_color.opposite();
    let kbb = pos.kind_bitboard();
    let king_steps = if exclude_king {
        kbb.promote_layer() & (kbb.bishopish() | kbb.rookish())
    } else {
        kbb.bitboard(Kind::King) | (kbb.promote_layer() & (kbb.bishopish() | kbb.rookish()))
    };
    let step = (pawn_power(opp, sq) & kbb.bitboard(Kind::Pawn))
        | (knight_power(opp, sq) & kbb.bitboard(Kind::Knight))
        | (silver_power(opp, sq) & kbb.bitboard(Kind::Silver))
        | (gold_power(opp, sq) & kbb.goldish())
        | (king_power(sq) & king_steps);
    if !(step & atk_bb).is_empty() {
        return true;
    }
    if !(lance_reachable(occ, opp, sq) & kbb.bitboard(Kind::Lance) & atk_bb).is_empty() {
        return true;
    }
    if !(bishop_reachable(occ, sq) & kbb.bishopish() & atk_bb).is_empty() {
        return true;
    }
    !(rook_reachable(occ, sq) & kbb.rookish() & atk_bb).is_empty()
}

/// Check if `attacker_color` has any piece attacking `sq` (pseudo-legal attack check, ~4 ns).
#[inline]
pub(crate) fn has_attack_on(pos: &PositionAux, attacker_color: Color, sq: Square) -> bool {
    has_attack_on_masked(
        pos,
        attacker_color,
        pos.color_bb(attacker_color),
        pos.occupied_bb(),
        sq,
        false,
    )
}

thread_local! {
    static DUMMY_BUF: RefCell<Vec<Movement>> = RefCell::new(Vec::with_capacity(8));
    static RAW_MOV_POOL: RefCell<Vec<Vec<Movement>>> = RefCell::new(
        (0..4).map(|_| Vec::with_capacity(128)).collect()
    );
}

#[inline(always)]
fn acquire_raw_mov_buf() -> Vec<Movement> {
    RAW_MOV_POOL.with(|cell| {
        let mut pool = cell.borrow_mut();
        let mut v = pool.pop().unwrap_or_else(|| Vec::with_capacity(128));
        v.clear();
        v
    })
}

#[inline(always)]
fn release_raw_mov_buf(v: Vec<Movement>) {
    RAW_MOV_POOL.with(|cell| {
        cell.borrow_mut().push(v);
    });
}

/// `fmrs`-backed position for `shtsume`'s df-pn solver.
#[derive(Clone)]
pub struct FmrsPos {
    pos: PositionAux,
    zkey: u64,
    mkey: [MKey; 2],
    attacker: Option<Attacker>,
    count: u16,
    n_oute: u8,
    attack_sq: u8,
    white_ou: u8,
    black_ou: u8,
}

impl FmrsPos {
    /// Construct a root `FmrsPos` (`count = 0`) directly from a `PositionAux` without SFEN string encoding.
    #[inline]
    pub fn from_pos_aux_root(mut pos: PositionAux) -> Self {
        if pos.turn().is_white() {
            pos = pos.flipped();
        }
        let mkey = [
            hands_to_mkey(&pos, Color::BLACK),
            hands_to_mkey(&pos, Color::WHITE),
        ];
        Self::from_pos_aux(pos, mkey, 0)
    }

    #[inline(always)]
    pub fn pos_aux(&self) -> &PositionAux {
        &self.pos
    }

    #[inline(always)]
    pub fn with_count(mut self, count: u16) -> Self {
        self.count = count;
        self
    }

    #[inline(always)]
    pub fn self_ou(&self) -> usize {
        if self.pos.turn().is_white() {
            self.white_ou as usize
        } else {
            self.black_ou as usize
        }
    }

    fn from_pos_aux(pos: PositionAux, mkey: [MKey; 2], count: u16) -> Self {
        let atk = Self::compute_attacker(&pos);
        let (n_oute, attack_sq) = match atk {
            None => (0, 0),
            Some(a) => (
                if a.double_check.is_some() { 2 } else { 1 },
                SQ_FROM_FMRS[a.pos.index()],
            ),
        };
        let white_ou = SQ_FROM_FMRS[pos.white_king_pos().index()];
        let black_ou = pos
            .black_king_pos()
            .map(|sq| SQ_FROM_FMRS[sq.index()])
            .unwrap_or(NO_SQ);
        let zkey = if pos.turn().is_white() {
            pos.board_digest() ^ TURN_ZOBRIST
        } else {
            pos.board_digest()
        };
        Self {
            pos,
            zkey,
            mkey,
            attacker: atk,
            count,
            n_oute,
            attack_sq,
            white_ou,
            black_ou,
        }
    }

    #[inline]
    fn compute_attacker(pos: &PositionAux) -> Option<Attacker> {
        let turn = pos.turn();
        if turn.is_black() && pos.black_king_pos().is_none() {
            return None;
        }
        attacker(pos, turn, false)
    }

    /// Returns true if the pawn drop `mv` is a legal check (i.e. NOT drop-pawn mate / 打ち歩詰め).
    /// Works even when Black currently has 0 pawns in hand (e.g. hypothetical `drop_check`).
    #[inline]
    fn is_legal_pawn_drop(&self, mv: Move) -> bool {
        let dest = sq_to_fmrs(mv.dest());
        // Fast path (~4 ns): if Black has no piece defending `dest`, White's King can always
        // legally capture the dropped Pawn (`King -> dest`), so 打ち歩詰め is impossible.
        // (`white_ou` is directly North of `dest`; at Black's turn Black cannot have an unblocked
        // Rook/Dragon North of `white_ou`, as `white_ou` would already be in check.)
        if !has_attack_on(&self.pos, Color::BLACK, dest) {
            debug_assert!(self.is_legal_pawn_drop_slow(dest));
            return true;
        }
        self.is_legal_pawn_drop_slow(dest)
    }

    fn is_legal_pawn_drop_slow(&self, dest: Square) -> bool {
        let mut child = self.pos.clone();
        child.set(dest, Color::BLACK, Kind::Pawn);
        child.set_turn(Color::WHITE);
        let opts = AdvanceOptions {
            max_allowed_branches: Some(0),
            assume_not_in_check: false,
        };
        // A pawn drop onto an empty square cannot discover a second attacker, so the
        // dropped pawn is the unique attacker on White's king.
        let atk = Attacker {
            pos: dest,
            kind: Kind::Pawn,
            double_check: None,
        };
        DUMMY_BUF.with(|cell| {
            let mut dummy = cell.borrow_mut();
            dummy.clear();
            match advance_white_with_attacker(&child, &opts, Some(atk), &mut dummy) {
                Err(crate::position::advance::AdvanceError::TooManyBranches) => true,
                Ok(_) => !dummy.is_empty(),
                Err(_) => false,
            }
        })
    }

    /// Futile interposition check on `dest` (analogous to `invalid_drops` in shtsume's `ntbase.c`).
    fn is_futile_drop_square<O: HsOracle>(&self, dest: usize, oracle: &mut O) -> bool {
        let dest_sq = sq_to_fmrs(dest);
        let occ = self.pos.occupied_bb();

        let black_bb = self.pos.color_bb(Color::BLACK);
        let mut candidates = (king_power(dest_sq) & black_bb)
            | (knight_power(Color::WHITE, dest_sq)
                & self.pos.bitboard(Color::BLACK, Kind::Knight));
        candidates |= lance_reachable(occ, Color::WHITE, dest_sq)
            & self.pos.bitboard(Color::BLACK, Kind::Lance);
        candidates |= bishop_reachable(occ, dest_sq)
            & (self.pos.bishopish() & black_bb);
        candidates |= rook_reachable(occ, dest_sq)
            & (self.pos.rookish() & black_bb);

        let opts_zero = AdvanceOptions {
            max_allowed_branches: Some(0),
            assume_not_in_check: false,
        };

        for src_sq in candidates {
            let src_kind = self.pos.must_get_kind(src_sq);
            if src_kind == Kind::King || !power(Color::WHITE, dest_sq, src_kind).contains(src_sq) {
                continue;
            }
            let can_prom =
                src_kind.promote().is_some() && (src_sq.row() < 3 || dest_sq.row() < 3);
            let must_prom = match src_kind {
                Kind::Pawn | Kind::Lance => dest_sq.row() == 0,
                Kind::Knight => dest_sq.row() <= 1,
                _ => false,
            };

            let try_proms: &[bool] = if must_prom {
                &[true]
            } else if can_prom {
                &[true, false]
            } else {
                &[false]
            };

            for &promote in try_proms {
                let mut test_pos = self.pos.clone();
                test_pos.set_turn(Color::BLACK);
                test_pos.do_move(&Movement::Move {
                    source: src_sq,
                    dest: dest_sq,
                    promote,
                    source_kind_hint: Some(src_kind),
                    capture_kind_hint: None,
                });
                if self.black_ou != NO_SQ && attacker(&test_pos, Color::BLACK, true).is_some() {
                    continue;
                }
                let Some(atk) = attacker(&test_pos, Color::WHITE, false) else {
                    continue;
                };
                let is_imm_mate = DUMMY_BUF.with(|cell| {
                    let mut dummy = cell.borrow_mut();
                    dummy.clear();
                    match advance_white_with_attacker(&test_pos, &opts_zero, Some(atk), &mut dummy)
                    {
                        Ok(_) => dummy.is_empty(),
                        Err(_) => false,
                    }
                });
                if is_imm_mate {
                    return true;
                }
                let n_oute = if atk.double_check.is_some() { 2 } else { 1 };
                let zkey = test_pos.board_digest() ^ TURN_ZOBRIST;
                let child = FmrsPos {
                    pos: test_pos,
                    zkey,
                    mkey: self.mkey,
                    attacker: Some(atk),
                    count: self.count + 2,
                    n_oute,
                    attack_sq: SQ_FROM_FMRS[atk.pos.index()],
                    white_ou: self.white_ou,
                    black_ou: self.black_ou,
                };
                if oracle.hs_lookup(&child, Turn::Sente) {
                    return true;
                }
            }
        }
        false
    }

    #[inline]
    fn test_drop_1ply_mate(&self, dest_sq: Square, kind: Kind, ktype: u8) -> Option<OnePlyMate> {
        let mut test_pos = self.pos.clone();
        test_pos.set(dest_sq, Color::BLACK, kind);
        test_pos.set_turn(Color::WHITE);
        let opts_zero = AdvanceOptions {
            max_allowed_branches: Some(0),
            assume_not_in_check: false,
        };
        let atk = Attacker {
            pos: dest_sq,
            kind,
            double_check: None,
        };
        let is_mate = DUMMY_BUF.with(|cell| {
            let mut dummy = cell.borrow_mut();
            dummy.clear();
            match advance_white_with_attacker(&test_pos, &opts_zero, Some(atk), &mut dummy) {
                Ok(_) => dummy.is_empty(),
                Err(_) => false,
            }
        });
        if is_mate {
            Some(OnePlyMate {
                mv: Move::drop(ktype, SQ_FROM_FMRS[dest_sq.index()] as u8),
                src_koma: 0,
                cap_koma: 0,
                final_atk_koma: kind_color_to_koma(Color::BLACK, kind),
                is_double_check: false,
            })
        } else {
            None
        }
    }

    #[inline]
    fn try_board_dest_1ply_mate(&self, dest_sq: Square, ou_sq: Square) -> Option<OnePlyMate> {
        let occ = self.pos.occupied_bb();
        let black_bb = self.pos.black_bb();
        let mut candidates = (king_power(dest_sq) & black_bb)
            | (knight_power(Color::WHITE, dest_sq)
                & self.pos.bitboard(Color::BLACK, Kind::Knight));
        candidates |= lance_reachable(occ, Color::WHITE, dest_sq)
            & self.pos.bitboard(Color::BLACK, Kind::Lance);
        candidates |= bishop_reachable(occ, dest_sq) & (self.pos.bishopish() & black_bb);
        candidates |= rook_reachable(occ, dest_sq) & (self.pos.rookish() & black_bb);

        let cap_kind_opt = self.pos.get_kind(dest_sq);
        let cap_koma = match cap_kind_opt {
            Some(ck) => kind_color_to_koma(Color::WHITE, ck),
            None => SPC,
        };
        let d_idx = SQ_FROM_FMRS[dest_sq.index()];
        let opts_zero = AdvanceOptions {
            max_allowed_branches: Some(0),
            assume_not_in_check: false,
        };

        for src_sq in candidates {
            let src_kind = self.pos.must_get_kind(src_sq);
            if src_kind == Kind::King || !power(Color::WHITE, dest_sq, src_kind).contains(src_sq) {
                continue;
            }
            let can_prom = src_kind.promote().is_some() && (src_sq.row() < 3 || dest_sq.row() < 3);
            let must_prom = match src_kind {
                Kind::Pawn | Kind::Lance => dest_sq.row() == 0,
                Kind::Knight => dest_sq.row() <= 1,
                _ => false,
            };
            let try_proms: &[bool] = if must_prom {
                &[true]
            } else if can_prom {
                &[true, false]
            } else {
                &[false]
            };
            let src_aligned = src_sq.col() == ou_sq.col()
                || src_sq.row() == ou_sq.row()
                || src_sq.col().abs_diff(ou_sq.col()) == src_sq.row().abs_diff(ou_sq.row());

            for &promote in try_proms {
                let after_kind = if promote {
                    src_kind.promote().unwrap_or(src_kind)
                } else {
                    src_kind
                };
                // Quick check filter before cloning `PositionAux`
                let direct_chk = match after_kind {
                    Kind::Lance => dest_sq.col() == ou_sq.col() && dest_sq.row() > ou_sq.row(),
                    Kind::Bishop => {
                        dest_sq.col().abs_diff(ou_sq.col()) == dest_sq.row().abs_diff(ou_sq.row())
                    }
                    Kind::Rook => dest_sq.col() == ou_sq.col() || dest_sq.row() == ou_sq.row(),
                    Kind::ProBishop | Kind::ProRook => true, // dest_sq is in king_power(ou_sq)
                    _ => power(Color::BLACK, dest_sq, after_kind).contains(ou_sq),
                };
                if !direct_chk && !src_aligned {
                    continue;
                }

                let mut test_pos = self.pos.clone();
                test_pos.do_move(&Movement::Move {
                    source: src_sq,
                    source_kind_hint: Some(src_kind),
                    dest: dest_sq,
                    promote,
                    capture_kind_hint: Some(cap_kind_opt),
                });
                if self.black_ou != NO_SQ && attacker(&test_pos, Color::BLACK, true).is_some() {
                    continue;
                }
                let Some(atk) = attacker(&test_pos, Color::WHITE, false) else {
                    continue;
                };
                let is_mate = DUMMY_BUF.with(|cell| {
                    let mut dummy = cell.borrow_mut();
                    dummy.clear();
                    match advance_white_with_attacker(&test_pos, &opts_zero, Some(atk), &mut dummy)
                    {
                        Ok(_) => dummy.is_empty(),
                        Err(_) => false,
                    }
                });
                if is_mate {
                    let s_idx = SQ_FROM_FMRS[src_sq.index()];
                    let mv = if promote {
                        Move::promote(s_idx, d_idx)
                    } else {
                        Move::normal(s_idx, d_idx)
                    };
                    return Some(OnePlyMate {
                        mv,
                        src_koma: kind_color_to_koma(Color::BLACK, src_kind),
                        cap_koma,
                        final_atk_koma: kind_color_to_koma(Color::BLACK, atk.kind),
                        is_double_check: atk.double_check.is_some(),
                    });
                }
            }
        }
        None
    }

    #[inline]
    fn find_1ply_mate_inner(&self, simple: bool) -> Option<OnePlyMate> {
        if !self.pos.turn().is_black() || self.n_oute != 0 {
            return None;
        }
        let ou_sq = sq_to_fmrs(self.white_ou as usize);
        let ring1 = king_power(ou_sq);
        let occ = self.pos.occupied_bb();
        let hm = self.mkey[0];
        let hand_total = hm.total();
        let (atk_e1, atk_e2) = FastBoardAttacks::compute_black_e12(&self.pos);

        let has_non_fu = hm.ki() + hm.gi() + hm.hi() + hm.ka() + hm.ky() + hm.ke() > 0;
        if has_non_fu {
            let sup_empty_ring1 = BitBoard::from_u128(ring1.and_not(occ).u128() & atk_e1);
            for d in sup_empty_ring1 {
                let dc = d.col() as i32 - ou_sq.col() as i32;
                let dr = d.row() as i32 - ou_sq.row() as i32;
                if dc == 0 && dr == 1 {
                    if hm.ki() > 0 {
                        if let Some(m) = self.test_drop_1ply_mate(d, Kind::Gold, KI) {
                            return Some(m);
                        }
                    } else if hm.gi() > 0 {
                        if let Some(m) = self.test_drop_1ply_mate(d, Kind::Silver, GI) {
                            return Some(m);
                        }
                    }
                    if hm.hi() > 0 {
                        if let Some(m) = self.test_drop_1ply_mate(d, Kind::Rook, HI) {
                            return Some(m);
                        }
                    } else if hm.ky() > 0 {
                        if let Some(m) = self.test_drop_1ply_mate(d, Kind::Lance, KY) {
                            return Some(m);
                        }
                    }
                } else if dc == 0 || dr == 0 {
                    if hm.ki() > 0 {
                        if let Some(m) = self.test_drop_1ply_mate(d, Kind::Gold, KI) {
                            return Some(m);
                        }
                    }
                    if hm.hi() > 0 {
                        if let Some(m) = self.test_drop_1ply_mate(d, Kind::Rook, HI) {
                            return Some(m);
                        }
                    }
                } else if dr == 1 {
                    if hm.ki() > 0 {
                        if let Some(m) = self.test_drop_1ply_mate(d, Kind::Gold, KI) {
                            return Some(m);
                        }
                    }
                    if hm.ka() > 0 {
                        if let Some(m) = self.test_drop_1ply_mate(d, Kind::Bishop, KA) {
                            return Some(m);
                        }
                    } else if hm.gi() > 0 {
                        if let Some(m) = self.test_drop_1ply_mate(d, Kind::Silver, GI) {
                            return Some(m);
                        }
                    }
                } else {
                    if hm.gi() > 0 {
                        if let Some(m) = self.test_drop_1ply_mate(d, Kind::Silver, GI) {
                            return Some(m);
                        }
                    }
                    if hm.ka() > 0 {
                        if let Some(m) = self.test_drop_1ply_mate(d, Kind::Bishop, KA) {
                            return Some(m);
                        }
                    }
                }
            }

            if hm.ke() > 0 && (ring1.and_not(self.pos.white_bb()).u128() & !atk_e1) == 0 {
                for d in knight_power(Color::WHITE, ou_sq).and_not(occ) {
                    if let Some(m) = self.test_drop_1ply_mate(d, Kind::Knight, KE) {
                        return Some(m);
                    }
                }
            }
        }

        if simple || hand_total != 1 {
            let empty_sup2 = BitBoard::from_u128(ring1.and_not(occ).u128() & atk_e2);
            for d in empty_sup2 {
                if let Some(m) = self.try_board_dest_1ply_mate(d, ou_sq) {
                    return Some(m);
                }
            }
            if simple || hand_total >= 2 || self.count >= 30 {
                let cap_sup2 = BitBoard::from_u128((ring1 & self.pos.white_bb()).u128() & atk_e2);
                for d in cap_sup2 {
                    if let Some(m) = self.try_board_dest_1ply_mate(d, ou_sq) {
                        return Some(m);
                    }
                }
            }
        }

        None
    }
}

impl PositionBackend for FmrsPos {
    fn from_sfen(sfen_str: &str) -> anyhow::Result<Self> {
        let mut pos = sfen::decode_position(sfen_str)?;
        if pos.turn().is_white() {
            pos = pos.flipped();
        }
        let mkey = [
            hands_to_mkey(&pos, Color::BLACK),
            hands_to_mkey(&pos, Color::WHITE),
        ];
        Ok(Self::from_pos_aux(pos, mkey, 0))
    }

    #[inline(always)]
    fn turn(&self) -> Turn {
        if self.pos.turn().is_black() {
            Turn::Sente
        } else {
            Turn::Gote
        }
    }

    #[inline(always)]
    fn count(&self) -> u16 {
        self.count
    }

    #[inline(always)]
    fn zkey(&self) -> u64 {
        self.zkey
    }

    #[inline(always)]
    fn mkey(&self, side: Turn) -> MKey {
        self.mkey[side.index()]
    }

    #[inline(always)]
    fn key_state(&self) -> KeyState {
        KeyState {
            zkey: self.zkey,
            mkey: self.mkey,
            turn: self.turn(),
            count: self.count,
        }
    }

    #[inline(always)]
    fn key_forward_item(&self, item: &MvItem) -> KeyState {
        let mv = item.head_move();
        let turn = self.turn();
        let side_idx = turn.index();
        let mut zkey = self.zkey ^ TURN_ZOBRIST;
        let mut mkey = self.mkey;
        let dest_z = SQ_ZBASE[mv.dest()];

        if mv.is_drop() {
            let ktype = mv.hand_ktype();
            let drop_koma = (ktype as usize) | (side_idx << 4);
            zkey ^= zobrist_raw(dest_z | KOMA_CK[drop_koma]);
            mkey[side_idx].dec(ktype);
        } else {
            let src_z = SQ_ZBASE[mv.prev_pos as usize];
            let src_koma = item.src_koma as usize;
            let dst_koma = src_koma + if mv.is_promote() { 8 } else { 0 };
            zkey ^= zobrist_raw(src_z | KOMA_CK[src_koma]) ^ zobrist_raw(dest_z | KOMA_CK[dst_koma]);
            let cap_koma = item.cap_koma;
            if cap_koma != 0 {
                zkey ^= zobrist_raw(dest_z | KOMA_CK[cap_koma as usize]);
                mkey[side_idx].inc(cap_koma & 7);
            }
        }

        let ks = KeyState {
            zkey,
            mkey,
            turn: turn.flip(),
            count: self.count + 1,
        };
        debug_assert_eq!(ks.zkey, self.key_forward(mv).zkey);
        debug_assert_eq!(ks.mkey, self.key_forward(mv).mkey);
        ks
    }

    #[inline]
    fn key_forward(&self, mv: Move) -> KeyState {
        let turn = self.turn();
        let color = self.pos.turn();
        let mut zkey = self.zkey ^ TURN_ZOBRIST;
        let mut mkey = self.mkey;
        let dest_sq = sq_to_fmrs(mv.dest());

        if mv.is_drop() {
            let ktype = mv.hand_ktype();
            let kind = ktype_to_kind(ktype);
            zkey ^= zobrist(color, dest_sq, kind);
            mkey[turn.index()].dec(ktype);
        } else {
            let src_sq = sq_to_fmrs(mv.prev_pos as usize);
            let moved_kind = self.pos.must_get_kind(src_sq);
            let new_kind = if mv.is_promote() {
                moved_kind.promote().unwrap_or(moved_kind)
            } else {
                moved_kind
            };
            zkey ^= zobrist(color, src_sq, moved_kind) ^ zobrist(color, dest_sq, new_kind);
            if let Some(cap_kind) = self.pos.get_kind(dest_sq) {
                zkey ^= zobrist(color.opposite(), dest_sq, cap_kind);
                mkey[turn.index()].inc(kind_to_ktype(cap_kind));
            }
        }

        KeyState {
            zkey,
            mkey,
            turn: turn.flip(),
            count: self.count + 1,
        }
    }

    fn move_forward(&self, mv: Move) -> Self {
        let mut next = self.pos.clone();
        let mut mkey = self.mkey;
        let side_idx = self.turn().index();
        let dest_idx = mv.dest();
        let dest = sq_to_fmrs(dest_idx);
        let mut white_ou = self.white_ou;
        let mut black_ou = self.black_ou;
        let next_is_white = side_idx == 0;

        let atk = if mv.is_drop() {
            let ktype = mv.hand_ktype();
            let kind = ktype_to_kind(ktype);
            mkey[side_idx].dec(ktype);
            next.do_move(&Movement::Drop(dest, kind));
            if next_is_white {
                let wo = sq_to_fmrs(white_ou as usize);
                if power(Color::BLACK, dest, kind).contains(wo)
                    && (!kind.is_line_piece()
                        || (BitBoard::between(dest, wo) & next.occupied_bb()).is_empty())
                {
                    Some(Attacker {
                        pos: dest,
                        kind,
                        double_check: None,
                    })
                } else {
                    attacker(&next, Color::WHITE, false)
                }
            } else if black_ou == NO_SQ {
                None
            } else {
                let bo = sq_to_fmrs(black_ou as usize);
                if power(Color::WHITE, dest, kind).contains(bo) {
                    attacker(&next, Color::BLACK, false)
                } else {
                    None
                }
            }
        } else {
            let source = sq_to_fmrs(mv.prev_pos as usize);
            let s_kind = next.must_get_kind(source);
            let c_kind = next.get_kind(dest);
            if let Some(ck) = c_kind {
                mkey[side_idx].inc(kind_to_ktype(ck));
            }
            if s_kind == Kind::King {
                if side_idx == 0 {
                    black_ou = dest_idx as u8;
                } else {
                    white_ou = dest_idx as u8;
                }
            }
            let promote = mv.is_promote();
            let new_kind = if promote {
                s_kind.promote().unwrap_or(s_kind)
            } else {
                s_kind
            };
            next.do_move(&Movement::Move {
                source,
                source_kind_hint: Some(s_kind),
                dest,
                promote,
                capture_kind_hint: Some(c_kind),
            });
            if next_is_white {
                let wo = sq_to_fmrs(white_ou as usize);
                let src_aligned = source.col() == wo.col()
                    || source.row() == wo.row()
                    || source.col().abs_diff(wo.col()) == source.row().abs_diff(wo.row());
                if !src_aligned
                    && power(Color::BLACK, dest, new_kind).contains(wo)
                    && (!new_kind.is_line_piece()
                        || (BitBoard::between(dest, wo) & next.occupied_bb()).is_empty())
                {
                    Some(Attacker {
                        pos: dest,
                        kind: new_kind,
                        double_check: None,
                    })
                } else {
                    attacker(&next, Color::WHITE, false)
                }
            } else if black_ou == NO_SQ {
                None
            } else {
                let bo = sq_to_fmrs(black_ou as usize);
                let src_aligned = source.col() == bo.col()
                    || source.row() == bo.row()
                    || source.col().abs_diff(bo.col()) == source.row().abs_diff(bo.row());
                if !src_aligned && !power(Color::WHITE, dest, new_kind).contains(bo) {
                    None
                } else {
                    attacker(&next, Color::BLACK, false)
                }
            }
        };
        debug_assert_eq!(
            atk.map(|a| (a.pos, a.kind, a.double_check)),
            if !next_is_white && black_ou == NO_SQ {
                None
            } else {
                attacker(
                    &next,
                    if next_is_white {
                        Color::WHITE
                    } else {
                        Color::BLACK
                    },
                    false,
                )
            }
            .map(|a| (a.pos, a.kind, a.double_check))
        );

        let zkey = if next_is_white {
            next.board_digest() ^ TURN_ZOBRIST
        } else {
            next.board_digest()
        };
        let (n_oute, attack_sq) = match atk {
            None => (0, 0),
            Some(a) => (
                if a.double_check.is_some() { 2 } else { 1 },
                SQ_FROM_FMRS[a.pos.index()],
            ),
        };
        Self {
            pos: next,
            zkey,
            mkey,
            attacker: atk,
            count: self.count + 1,
            n_oute,
            attack_sq,
            white_ou,
            black_ou,
        }
    }

    #[inline(always)]
    fn board_at(&self, sq: usize) -> u8 {
        let fsq = sq_to_fmrs(sq);
        match self.pos.get(fsq) {
            None => SPC,
            Some((c, k)) => kind_color_to_koma(c, k),
        }
    }

    #[inline(always)]
    fn enemy_ou(&self) -> usize {
        if self.pos.turn().is_black() {
            self.white_ou as usize
        } else {
            self.black_ou as usize
        }
    }

    #[inline(always)]
    fn self_ou(&self) -> usize {
        if self.pos.turn().is_black() {
            self.black_ou as usize
        } else {
            self.white_ou as usize
        }
    }

    #[inline(always)]
    fn n_oute(&self) -> u8 {
        self.n_oute
    }

    #[inline(always)]
    fn attack_sq(&self) -> usize {
        self.attack_sq as usize
    }

    fn self_effect_contains(&self, sq: usize) -> bool {
        has_attack_on(&self.pos, self.pos.turn(), sq_to_fmrs(sq))
    }

    fn symmetry_check(&self) -> bool {
        let ou = self.enemy_ou();
        if ou % 9 != 4 {
            return false;
        }
        for rank in 0..9 {
            for file in 0..4 {
                let p1 = rank * 9 + file;
                let p2 = rank * 9 + (8 - file);
                if self.board_at(p1) != self.board_at(p2) {
                    return false;
                }
            }
        }
        true
    }

    fn drop_check(&self, ktype: u8) -> bool {
        let ou_sq = sq_to_fmrs(self.white_ou as usize);
        let kind = ktype_to_kind(ktype);
        let occ = self.pos.occupied_bb();
        let empty_attack = match kind {
            Kind::Pawn => {
                if ou_sq.row() >= 8 {
                    return false;
                }
                let sq = Square::new(ou_sq.col(), ou_sq.row() + 1);
                if occ.contains(sq) || self.pos.col_has_pawn(Color::BLACK, sq.col()) {
                    return false;
                }
                let mv = Move::drop(FU, sq_from_fmrs(sq) as u8);
                return self.is_legal_pawn_drop(mv);
            }
            Kind::Lance => lance_reachable(occ, Color::WHITE, ou_sq).and_not(occ),
            Kind::Knight => knight_power(Color::WHITE, ou_sq).and_not(occ),
            Kind::Silver | Kind::Gold => power(Color::WHITE, ou_sq, kind).and_not(occ),
            Kind::Bishop => bishop_reachable(occ, ou_sq).and_not(occ),
            Kind::Rook => rook_reachable(occ, ou_sq).and_not(occ),
            _ => return false,
        };
        !empty_attack.is_empty()
    }

    #[inline(always)]
    fn find_1ply_mate(&self) -> Option<OnePlyMate> {
        self.find_1ply_mate_inner(false)
    }

    #[inline(always)]
    fn find_1ply_mate_simple(&self) -> Option<OnePlyMate> {
        self.find_1ply_mate_inner(true)
    }

    const DEFAULT_EPS_DN_SHIFT: u8 = 4;

    #[inline(always)]
    fn default_score_and_moves(&self, list: &mut [MvItem]) {
        crate::futsuu::ml::score_fmrs_and_default(self, list);
    }

    fn generate_check_fast(&self, out: &mut Vec<MvItem>) {
        let opts = AdvanceOptions {
            max_allowed_branches: None,
            assume_not_in_check: self.n_oute == 0,
        };
        let mut raw = acquire_raw_mov_buf();
        if advance_aux(&self.pos, &opts, &mut raw).is_ok() {
            out.reserve(raw.len());
            for &m in &raw {
                let (mv, src_koma, cap_koma) =
                    movement_to_move_and_hints(&self.pos, Color::BLACK, m);
                if mv.is_drop() && mv.hand_ktype() == FU && !self.is_legal_pawn_drop(mv) {
                    continue;
                }
                out.push(MvItem::single_with_hints(mv, src_koma, cap_koma));
            }
        }
        release_raw_mov_buf(raw);
    }

    fn generate_evasion_with<O: HsOracle>(&self, oracle: &mut O, out: &mut Vec<MvItem>) -> (bool, bool) {
        let opts = AdvanceOptions::default();
        let mut raw = acquire_raw_mov_buf();
        if advance_white_with_attacker(&self.pos, &opts, self.attacker, &mut raw).is_err()
            || raw.is_empty()
        {
            release_raw_mov_buf(raw);
            return (false, false);
        }

        let base = out.len();
        out.reserve(raw.len());

        let ou = self.self_ou();
        let atk_sq = self.attack_sq as usize;
        let is_sliding_check = match self.attacker {
            Some(a) => a.double_check.is_none() && a.kind.is_line_piece(),
            None => false,
        };

        let mut inter_moves: SmallVec<[(Move, u8); 16]> = SmallVec::new();
        let mut inter_drops: SmallVec<[Move; 32]> = SmallVec::new();

        for &m in &raw {
            let (mv, src_koma, cap_koma) = movement_to_move_and_hints(&self.pos, Color::WHITE, m);
            if !is_sliding_check {
                out.push(MvItem::single_with_hints(mv, src_koma, cap_koma));
            } else if mv.is_drop() {
                inter_drops.push(mv);
            } else if mv.prev_pos as usize == ou || mv.dest() == atk_sq {
                out.push(MvItem::single_with_hints(mv, src_koma, cap_koma));
            } else {
                inter_moves.push((mv, src_koma));
            }
        }
        // Release `raw` before `is_futile_drop_square` -> `oracle.hs_lookup` -> recursive `generate_evasion_with`.
        release_raw_mov_buf(raw);

        if !is_sliding_check {
            return (false, false);
        }

        let mut invalid_drops = false;
        let has_king_or_cap = out.len() > base;

        // `ORDERED_BETWEEN` + `Hands::kinds` already emit `inter_drops` in exact
        // `(DISTANCE[ou][mv.dest()], mv.hand_ktype())` non-decreasing order!
        debug_assert!(inter_drops
            .windows(2)
            .all(|w| (DISTANCE[ou][w[0].dest()], w[0].hand_ktype())
                <= (DISTANCE[ou][w[1].dest()], w[1].hand_ktype())));

        let mut filtered_drops: MoveChain = MoveChain::new();
        if !inter_drops.is_empty() {
            if !has_king_or_cap {
                let mut cur_dest: Option<usize> = None;
                let mut cur_futile = false;
                for mv in inter_drops {
                    let d = mv.dest();
                    if cur_dest != Some(d) {
                        cur_dest = Some(d);
                        cur_futile = !self.self_effect_contains(d)
                            && self.is_futile_drop_square(d, oracle);
                        if cur_futile {
                            invalid_drops = true;
                        }
                    }
                    if !cur_futile {
                        filtered_drops.push(mv);
                    }
                }
            } else {
                filtered_drops.extend(inter_drops);
            }
        }

        if !filtered_drops.is_empty() {
            out.push(MvItem::from_moves_with_hints(filtered_drops, 0, 0));
        }

        if !inter_moves.is_empty() {
            if inter_moves.len() > 1 {
                inter_moves.sort_by_key(|(mv, _)| (DISTANCE[ou][mv.dest()], !mv.is_promote()));
            }
            if out.len() == base {
                let first_src = inter_moves[0].1;
                out.push(MvItem::from_moves_with_hints(
                    inter_moves.into_iter().map(|(mv, _)| mv).collect(),
                    first_src,
                    0,
                ));
            } else {
                for (mv, src_koma) in inter_moves {
                    out.push(MvItem::single_with_hints(mv, src_koma, 0));
                }
            }
        }

        (invalid_drops, false)
    }

    fn generate_evasion_simple_fast(&self, out: &mut Vec<MvItem>) {
        let opts = AdvanceOptions::default();
        let mut raw = acquire_raw_mov_buf();
        if advance_white_with_attacker(&self.pos, &opts, self.attacker, &mut raw).is_err()
            || raw.is_empty()
        {
            release_raw_mov_buf(raw);
            return;
        }

        let base = out.len();
        out.reserve(raw.len());

        let ou = self.self_ou();
        let atk_sq = self.attack_sq as usize;
        let is_sliding_check = match self.attacker {
            Some(a) => a.double_check.is_none() && a.kind.is_line_piece(),
            None => false,
        };

        let mut inter_moves: SmallVec<[(Move, u8); 16]> = SmallVec::new();
        let mut inter_drops: MoveChain = MoveChain::new();

        for &m in &raw {
            let (mv, src_koma, cap_koma) = movement_to_move_and_hints(&self.pos, Color::WHITE, m);
            if !is_sliding_check {
                out.push(MvItem::single_with_hints(mv, src_koma, cap_koma));
            } else if mv.is_drop() {
                inter_drops.push(mv);
            } else if mv.prev_pos as usize == ou || mv.dest() == atk_sq {
                out.push(MvItem::single_with_hints(mv, src_koma, cap_koma));
            } else {
                inter_moves.push((mv, src_koma));
            }
        }
        release_raw_mov_buf(raw);

        if !is_sliding_check {
            return;
        }

        if !inter_drops.is_empty() {
            out.push(MvItem::from_moves_with_hints(inter_drops, 0, 0));
        }

        if !inter_moves.is_empty() {
            if inter_moves.len() > 1 {
                inter_moves.sort_by_key(|(mv, _)| (DISTANCE[ou][mv.dest()], !mv.is_promote()));
            }
            if out.len() == base {
                let first_src = inter_moves[0].1;
                out.push(MvItem::from_moves_with_hints(
                    inter_moves.into_iter().map(|(mv, _)| mv).collect(),
                    first_src,
                    0,
                ));
            } else {
                for (mv, src_koma) in inter_moves {
                    out.push(MvItem::single_with_hints(mv, src_koma, 0));
                }
            }
        }
    }

    const SUPPORTS_NI: bool = true;

    #[inline(always)]
    fn ni_ring_hash(&self) -> u64 {
        let p = &self.pos;
        let wk = sq_to_fmrs(self.white_ou as usize);
        let wk_col = wk.col();
        let rshift = (wk_col.saturating_sub(1).min(6) * 9) as u32;
        let ring = (1u128 << wk.index()) | king_power(wk).u128();
        debug_assert_eq!(ring & !(((1u128 << 27) - 1) << rshift), 0);
        let rm27 = (ring >> rshift) as u32;
        let (prom, k0, k1, k2) = p.kind_bitboard().raw_parts();
        let b27 = ((p.black_bb().u128() >> rshift) as u32) & rm27;
        let w27 = ((p.white_bb().u128() >> rshift) as u32) & rm27;
        let p27 = ((prom.u128() >> rshift) as u32) & rm27;
        let k0_27 = ((k0.u128() >> rshift) as u32) & rm27;
        let k1_27 = ((k1.u128() >> rshift) as u32) & rm27;
        let k2_27 = ((k2.u128() >> rshift) as u32) & rm27;

        let u0 = (b27 as u64) | ((w27 as u64) << 32);
        let u1 = (p27 as u64) | ((k0_27 as u64) << 32);
        let u2 = (k1_27 as u64) | ((k2_27 as u64) << 32);
        let is_and = u64::from(p.turn().is_white());
        let tag = (is_and << 8) | (wk.index() as u64) | 0x517c_c1b7_2722_0a95u64;

        let h0 = u0.wrapping_mul(0x9e37_79b9_7f4a_7c15) ^ tag.wrapping_mul(0x94d0_49bb_1331_11eb);
        let h1 = u1.wrapping_mul(0xbf58_476d_1ce4_e5b9) ^ u2.wrapping_mul(0xd6e8_feb8_6659_fd93);
        let mut h = h0 ^ h1;
        h = (h ^ (h >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        h ^= h >> 31;
        let win_idx = wk_col.saturating_sub(3).min(2) as u64;
        (h & !(3u64 << 28)) | (win_idx << 28)
    }

    #[inline(always)]
    fn ni_planes(&self) -> BoardPlanes {
        let p = &self.pos;
        let (prom, k0, k1, k2) = p.kind_bitboard().raw_parts();
        BoardPlanes {
            black: p.black_bb().u128(),
            white: p.white_bb().u128(),
            prom: prom.u128(),
            k0: k0.u128(),
            k1: k1.u128(),
            k2: k2.u128(),
        }
    }

    #[inline(always)]
    fn ni_planes_and_ring_hash(&self) -> (BoardPlanes, u64) {
        (self.ni_planes(), self.ni_ring_hash())
    }

    fn ni_proof_leaf(&self) -> Option<(u128, MKey)> {
        self.ni_proof_and_common(&[])
    }

    fn ni_proof_and_local(&self, evasions: &[MvItem]) -> Option<(u128, MKey)> {
        for it in evasions {
            if it.has_tail() {
                return None;
            }
        }
        self.ni_proof_and_common(evasions)
    }

    fn ni_proof_or_move_mask(&self, mv: Move, child_sh: u16) -> Option<u128> {
        if !self.pos.turn().is_black() || self.n_oute != 0 {
            return None;
        }
        let p = &self.pos;
        let (w_bish, w_rook, w_lance, node_pm) = pos_white_slider_info(p);
        let mut req_absent = 0u16;
        let mut cap_unique = 0u16;
        let occ = p.occupied_bb();
        let wk = sq_to_fmrs(self.white_ou as usize);
        let dest = sq_to_fmrs(mv.dest());
        let dst_bit = 1u128 << dest.index();
        let mut mask = (1u128 << wk.index()) | king_power(wk).u128() | dst_bit;

        if mv.is_drop() {
            if mv.hand_ktype() == FU {
                // Guard nifu (entire file of dest)
                mask |= 0x1ffu128 << (dest.col() * 9);
                // Guard against 打ち歩詰め: prove at least one White evasion to P*dest stays legal.
                if child_sh == 0 {
                    return None;
                }
                let guard = self.ni_guard_one_evasion_after_pawn_drop(dest)?;
                mask |= guard;
            }
        } else {
            if w_bish == dst_bit {
                cap_unique |= REQ_NO_W_BISH;
            } else if w_rook == dst_bit {
                cap_unique |= REQ_NO_W_ROOK;
            } else if (w_lance & (0x1ffu128 << (dest.col() * 9))) == dst_bit {
                cap_unique |= req_no_w_lance_col(dest.col());
            }
            let src = sq_to_fmrs(mv.prev_pos as usize);
            mask |= (1u128 << src.index()) | BitBoard::between(src, dest).u128();
            if self.black_ou != NO_SQ {
                let bk = sq_to_fmrs(self.black_ou as usize);
                if src == bk {
                    let occ_no_bk = BitBoard::from_u128(occ.u128() & !(1u128 << bk.index()));
                    mask |= (1u128 << bk.index())
                        | ni_white_attack_guard_smart(
                            occ_no_bk,
                            dest,
                            w_bish,
                            w_rook,
                            w_lance,
                            &mut req_absent,
                        );
                } else if src.col() == bk.col() || src.row() == bk.row() {
                    let occ_no_src = BitBoard::from_u128(occ.u128() & !(1u128 << src.index()));
                    mask |= 1u128 << bk.index();
                    if w_rook == 0 {
                        req_absent |= REQ_NO_W_ROOK;
                        if src.col() == bk.col() && src.row() < bk.row() && bk.row() >= 2 {
                            let col = bk.col();
                            if (w_lance & (0x1ffu128 << (col * 9))) == 0 {
                                req_absent |= req_no_w_lance_col(col);
                            } else {
                                mask |= lance_reachable(occ_no_src, Color::BLACK, bk).u128();
                            }
                        }
                    } else {
                        mask |= rook_reachable(occ_no_src, bk).u128();
                    }
                } else if src.col().abs_diff(bk.col()) == src.row().abs_diff(bk.row()) {
                    let occ_no_src = BitBoard::from_u128(occ.u128() & !(1u128 << src.index()));
                    mask |= 1u128 << bk.index();
                    if w_bish == 0 {
                        req_absent |= REQ_NO_W_BISH;
                    } else {
                        mask |= bishop_reachable(occ_no_src, bk).u128();
                    }
                }
            }
        }
        Some(pack_ni_mask_cap(mask, req_absent, node_pm, cap_unique))
    }
}

#[inline(always)]
fn pos_white_slider_info(p: &PositionAux) -> (u128, u128, u128, u16) {
    let white_bb = p.white_bb().u128();
    let w_bish = p.bishopish().u128() & white_bb;
    let w_rook = p.rookish().u128() & white_bb;
    let w_lance = p.bitboard(Color::WHITE, Kind::Lance).u128();
    let pm = white_slider_mask_from_bbs(w_bish, w_rook, w_lance);
    (w_bish, w_rook, w_lance, pm)
}

#[inline(always)]
fn ni_white_attack_guard_smart(
    occ: BitBoard,
    target: Square,
    w_bish: u128,
    w_rook: u128,
    w_lance: u128,
    req_absent: &mut u16,
) -> u128 {
    let mut m = (1u128 << target.index())
        | king_power(target).u128()
        | knight_power(Color::BLACK, target).u128();
    if w_bish == 0 {
        *req_absent |= REQ_NO_W_BISH;
    } else {
        m |= bishop_reachable(occ, target).u128();
    }
    if w_rook == 0 {
        *req_absent |= REQ_NO_W_ROOK;
        if target.row() >= 2 {
            let col = target.col();
            if (w_lance & (0x1ffu128 << (col * 9))) == 0 {
                *req_absent |= req_no_w_lance_col(col);
            } else {
                m |= lance_reachable(occ, Color::BLACK, target).u128();
            }
        }
    } else {
        m |= rook_reachable(occ, target).u128();
    }
    m
}

#[inline(always)]
fn ni_black_attack_guard(occ: BitBoard, target: Square) -> u128 {
    (1u128 << target.index())
        | king_power(target).u128()
        | knight_power(Color::WHITE, target).u128()
        | rook_reachable(occ, target).u128()
        | bishop_reachable(occ, target).u128()
}

#[inline]
fn ni_white_pin_rays(p: &PositionAux, wk: Square) -> u128 {
    let occ = p.occupied_bb();
    let white_bb = p.white_bb().u128();
    let black_bb = p.black_bb().u128();
    let r1 = rook_reachable(occ, wk).u128();
    let b1 = bishop_reachable(occ, wk).u128();
    let w_on_r = r1 & white_bb;
    let w_on_b = b1 & white_bb;
    let mut pins = 0u128;
    if w_on_r != 0 {
        let occ2 = BitBoard::from_u128(occ.u128() ^ w_on_r);
        let xray = rook_reachable(occ2, wk).u128() & !r1;
        let black_rookish = p.rookish().u128() & black_bb;
        let black_lance = p.bitboard(Color::BLACK, Kind::Lance).u128();
        for pinner in BitBoard::from_u128(xray & (black_rookish | black_lance)) {
            let k = p.must_get_kind(pinner);
            let is_pinner = match k {
                Kind::Rook | Kind::ProRook => true,
                Kind::Lance => pinner.col() == wk.col() && pinner.row() > wk.row(),
                _ => false,
            };
            if is_pinner {
                pins |= (1u128 << pinner.index()) | BitBoard::between(wk, pinner).u128();
            }
        }
    }
    if w_on_b != 0 {
        let occ2 = BitBoard::from_u128(occ.u128() ^ w_on_b);
        let xray = bishop_reachable(occ2, wk).u128() & !b1;
        let black_bish = p.bishopish().u128() & black_bb;
        for pinner in BitBoard::from_u128(xray & black_bish) {
            pins |= (1u128 << pinner.index()) | BitBoard::between(wk, pinner).u128();
        }
    }
    pins
}

impl FmrsPos {
    /// Guard ONE Black attacker on square `u` (with `occ_no_wk`), preferring step > knight > slider.
    #[inline]
    fn ni_guard_one_black_attacker(&self, occ_no_wk: BitBoard, u: Square) -> Option<u128> {
        let p = &self.pos;
        let b_bb = p.black_bb().u128();
        for a in BitBoard::from_u128(king_power(u).u128() & b_bb) {
            let k = p.must_get_kind(a);
            if power(Color::BLACK, a, k).contains(u) {
                return Some(1u128 << a.index());
            }
        }
        for a in BitBoard::from_u128(knight_power(Color::WHITE, u).u128() & b_bb) {
            if p.must_get_kind(a) == Kind::Knight {
                return Some(1u128 << a.index());
            }
        }
        let r_att = rook_reachable(occ_no_wk, u).u128() & b_bb;
        for a in BitBoard::from_u128(r_att) {
            let k = p.must_get_kind(a);
            let hits = match k {
                Kind::Rook | Kind::ProRook => true,
                Kind::Lance => a.col() == u.col() && a.row() > u.row(),
                _ => false,
            };
            if hits {
                return Some((1u128 << a.index()) | BitBoard::between(a, u).u128());
            }
        }
        let b_att = bishop_reachable(occ_no_wk, u).u128() & b_bb;
        for a in BitBoard::from_u128(b_att) {
            let k = p.must_get_kind(a);
            if matches!(k, Kind::Bishop | Kind::ProBishop) {
                return Some((1u128 << a.index()) | BitBoard::between(a, u).u128());
            }
        }
        None
    }

    /// Guard at least ONE legal White evasion after Black drops a Pawn on `dest` (to rule out 打ち歩詰め).
    fn ni_guard_one_evasion_after_pawn_drop(&self, dest: Square) -> Option<u128> {
        let p = &self.pos;
        let wk = sq_to_fmrs(self.white_ou as usize);
        let occ = p.occupied_bb();
        let occ_after = BitBoard::from_u128(occ.u128() | (1u128 << dest.index()));
        let occ_no_wk = BitBoard::from_u128(occ_after.u128() & !(1u128 << wk.index()));

        if !has_attack_on(p, Color::BLACK, dest) {
            return Some(ni_black_attack_guard(occ_no_wk, dest));
        }

        let mut test_pos = p.clone();
        test_pos.set(dest, Color::BLACK, Kind::Pawn);
        test_pos.set_turn(Color::WHITE);
        let white_bb = p.white_bb().u128();
        for u in BitBoard::from_u128(king_power(wk).u128() & !white_bb) {
            if u == dest {
                continue;
            }
            let mut moved = test_pos.clone();
            moved.do_move(&Movement::Move {
                source: wk,
                source_kind_hint: Some(Kind::King),
                dest: u,
                promote: false,
                capture_kind_hint: None,
            });
            if attacker(&moved, Color::WHITE, true).is_none() {
                return Some((1u128 << u.index()) | ni_black_attack_guard(occ_no_wk, u));
            }
        }

        let w_cand = (king_power(dest).u128() & white_bb)
            | (knight_power(Color::BLACK, dest).u128()
                & p.bitboard(Color::WHITE, Kind::Knight).u128())
            | (lance_reachable(occ, Color::BLACK, dest).u128()
                & p.bitboard(Color::WHITE, Kind::Lance).u128())
            | (bishop_reachable(occ, dest).u128() & (p.bishopish().u128() & white_bb))
            | (rook_reachable(occ, dest).u128() & (p.rookish().u128() & white_bb));
        for d in BitBoard::from_u128(w_cand & !(1u128 << wk.index())) {
            let dk = p.must_get_kind(d);
            if !power(Color::WHITE, d, dk).contains(dest) {
                continue;
            }
            let mut after_cap = test_pos.clone();
            after_cap.do_move(&Movement::Move {
                source: d,
                source_kind_hint: Some(dk),
                dest,
                promote: false,
                capture_kind_hint: Some(Some(Kind::Pawn)),
            });
            if attacker(&after_cap, Color::WHITE, true).is_none() {
                let mut m = (1u128 << d.index()) | BitBoard::between(d, dest).u128();
                if d.col() == wk.col() || d.row() == wk.row() {
                    let occ_no_d = BitBoard::from_u128(occ_after.u128() & !(1u128 << d.index()));
                    m |= rook_reachable(occ_no_d, wk).u128();
                } else if d.col().abs_diff(wk.col()) == d.row().abs_diff(wk.row()) {
                    let occ_no_d = BitBoard::from_u128(occ_after.u128() & !(1u128 << d.index()));
                    m |= bishop_reachable(occ_no_d, wk).u128();
                }
                return Some(m);
            }
        }
        None
    }

    fn ni_proof_and_common(&self, evasions: &[MvItem]) -> Option<(u128, MKey)> {
        if !self.pos.turn().is_white() {
            return None;
        }
        let atk = self.attacker?;
        let p = &self.pos;
        let (w_bish, w_rook, w_lance, node_pm) = pos_white_slider_info(p);
        let mut req_absent = 0u16;
        let occ = p.occupied_bb();
        let wk = sq_to_fmrs(self.white_ou as usize);
        let white_bb = p.white_bb().u128();
        let mut mask = (1u128 << wk.index()) | king_power(wk).u128();

        mask |= (1u128 << atk.pos.index()) | BitBoard::between(atk.pos, wk).u128();
        if let Some((sq2, _)) = atk.double_check {
            mask |= (1u128 << sq2.index()) | BitBoard::between(sq2, wk).u128();
        }

        let ou_shtsume = self.white_ou as usize;
        let mut proven_king_dests = 0u128;
        for it in evasions {
            let mv = it.head_move();
            if mv.is_board_move() && mv.prev_pos as usize == ou_shtsume {
                let d = sq_to_fmrs(mv.dest());
                proven_king_dests |= 1u128 << d.index();
            }
        }

        let occ_no_wk = BitBoard::from_u128(occ.u128() & !(1u128 << wk.index()));
        let blocked_flights = king_power(wk).u128() & !white_bb & !proven_king_dests;
        for u in BitBoard::from_u128(blocked_flights) {
            let g = self.ni_guard_one_black_attacker(occ_no_wk, u)?;
            mask |= g;
        }

        let mut def_max = MAX_HAND;

        if atk.double_check.is_none() {
            mask |= ni_white_attack_guard_smart(
                occ,
                atk.pos,
                w_bish,
                w_rook,
                w_lance,
                &mut req_absent,
            );
            let between = BitBoard::between(atk.pos, wk).u128();
            if between != 0 {
                mask |= between;
                let def_cur = self.mkey[1];
                for s in BitBoard::from_u128(between) {
                    mask |= ni_white_attack_guard_smart(
                        occ,
                        s,
                        w_bish,
                        w_rook,
                        w_lance,
                        &mut req_absent,
                    );
                    if s.row() < 2 && def_cur.ky() > 0 && (w_lance & (0x1ffu128 << (s.col() * 9))) == 0 {
                        req_absent |= req_no_w_lance_col(s.col());
                    }
                }
                let mut can_drop_fu = false;
                let mut can_drop_ky = false;
                let mut can_drop_ke = false;
                let mut fu_nifu_guards = 0u128;
                let w_pawns = p.bitboard(Color::WHITE, Kind::Pawn).u128();
                for s in BitBoard::from_u128(between) {
                    if s.row() < 8 {
                        can_drop_ky = true;
                        let f_pawns = w_pawns & (0x1ffu128 << (s.col() * 9));
                        if f_pawns != 0 {
                            fu_nifu_guards |= 1u128 << f_pawns.trailing_zeros();
                        } else {
                            can_drop_fu = true;
                        }
                    }
                    if s.row() < 7 {
                        can_drop_ke = true;
                    }
                }
                mask |= fu_nifu_guards;
                if can_drop_fu && def_cur.fu() == 0 {
                    mkey_set(&mut def_max, FU, 0);
                }
                if can_drop_ky && def_cur.ky() == 0 {
                    mkey_set(&mut def_max, KY, 0);
                }
                if can_drop_ke && def_cur.ke() == 0 {
                    mkey_set(&mut def_max, KE, 0);
                }
                for &kt in &[GI, KI, KA, HI] {
                    if def_cur.get(kt) == 0 {
                        mkey_set(&mut def_max, kt, 0);
                    }
                }
            }
            mask |= ni_white_pin_rays(p, wk);
        }

        Some((pack_ni_mask(mask, req_absent, node_pm), def_max))
    }
}
