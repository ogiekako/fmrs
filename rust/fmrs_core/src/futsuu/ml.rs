//! Ultra-fast tactical, geometric, and King-relative feature extraction for `FmrsPos`,
//! shared identically between offline dataset extraction (`extract_branch_data`) and
//! online `SearchHeuristic<FmrsPos>` scoring inside `Solver`.

use serde::{Deserialize, Serialize};

use crate::piece::{Color, Kind};
use crate::position::bitboard::{
    bishop_reachable, king_power, knight_power, lance_reachable, power, rook_reachable, BitBoard,
};
use crate::position::position::PositionAux;
use crate::position::Square;
use crate::futsuu::backend::PositionBackend;
use crate::futsuu::fmrs_backend::{
    has_attack_on, has_attack_on_masked, FastBoardAttacks, FmrsPos,
};
use crate::futsuu::types::{
    KOMA_PROMOTION, MvItem, DISTANCE, GFU, GGI, GHI, GKA, GKE, GKI, GKY, GNG, GNK, GNY, GOU, GRY,
    GTO, GUM, N_SQUARE, PROMOTED, SFU, SGI, SHI, SKA, SKE, SKI, SKY, SNG, SNK, SNY, SOU, SRY, STO,
    SUM,
};

static SQ_TO_FMRS: [Square; N_SQUARE] = {
    let mut t = [Square::S11; N_SQUARE];
    let mut sq = 0;
    while sq < N_SQUARE {
        t[sq] = Square::new(sq % 9, sq / 9);
        sq += 1;
    }
    t
};

/// 8-neighbor ring around each `shtsume` square (`row * 9 + col`) in `fmrs::BitBoard` layout.
static KING_RING1: [BitBoard; N_SQUARE] = {
    let mut t = [BitBoard::EMPTY; N_SQUARE];
    let mut sq = 0;
    while sq < N_SQUARE {
        let f = (sq % 9) as i32;
        let r = (sq / 9) as i32;
        let mut bits: u128 = 0;
        let mut df = -1;
        while df <= 1 {
            let mut dr = -1;
            while dr <= 1 {
                if !(df == 0 && dr == 0) {
                    let nf = f + df;
                    let nr = r + dr;
                    if nf >= 0 && nf < 9 && nr >= 0 && nr < 9 {
                        let idx = (nf as usize) * 9 + (nr as usize);
                        bits |= 1u128 << idx;
                    }
                }
                dr += 1;
            }
            df += 1;
        }
        t[sq] = BitBoard::from_u128(bits);
        sq += 1;
    }
    t
};

/// Outer Chebyshev-2 ring (`max(|df|, |dr|) == 2`, up to 16 squares) around each `shtsume` square.
static KING_RING2_OUTER: [BitBoard; N_SQUARE] = {
    let mut t = [BitBoard::EMPTY; N_SQUARE];
    let mut sq = 0;
    while sq < N_SQUARE {
        let f = (sq % 9) as i32;
        let r = (sq / 9) as i32;
        let mut bits: u128 = 0;
        let mut df: i32 = -2;
        while df <= 2 {
            let mut dr: i32 = -2;
            while dr <= 2 {
                if df.abs() == 2 || dr.abs() == 2 {
                    let nf = f + df;
                    let nr = r + dr;
                    if nf >= 0 && nf < 9 && nr >= 0 && nr < 9 {
                        let idx = (nf as usize) * 9 + (nr as usize);
                        bits |= 1u128 << idx;
                    }
                }
                dr += 1;
            }
            df += 1;
        }
        t[sq] = BitBoard::from_u128(bits);
        sq += 1;
    }
    t
};

/// King board-zone classification (0..6):
/// 0 = top corner (`row <= 1` and `(file == 0 || file == 8)`)
/// 1 = top edge (`row == 0`, interior file)
/// 2 = side edge (`file == 0 || file == 8`, `row >= 2`)
/// 3 = rank 2 interior (`row == 1`, `1 <= file <= 7`)
/// 4 = rank 3 interior (`row == 2`, `1 <= file <= 7`)
/// 5 = deep interior (`row >= 3`, `1 <= file <= 7`)
pub static KING_ZONE: [u8; N_SQUARE] = {
    let mut t = [0u8; N_SQUARE];
    let mut sq = 0;
    while sq < N_SQUARE {
        let f = sq % 9;
        let r = sq / 9;
        let edge_f = f == 0 || f == 8;
        t[sq] = if r <= 1 && edge_f {
            0
        } else if r == 0 {
            1
        } else if edge_f {
            2
        } else if r == 1 {
            3
        } else if r == 2 {
            4
        } else {
            5
        };
        sq += 1;
    }
    t
};

#[inline(always)]
fn popcnt(bb: BitBoard) -> u8 {
    let (hi, lo) = bb.high_low();
    (hi.count_ones() + lo.count_ones()) as u8
}

/// Map `shtsume` koma (`1..31`) to side-agnostic piece index `0..14`:
/// `0=FU, 1=KY, 2=KE, 3=GI, 4=KI, 5=KA, 6=HI, 7=TO, 8=NY, 9=NK, 10=NG, 11=UM, 12=RY, 13=OU`.
#[inline(always)]
pub fn koma_to_idx14(koma: u8) -> u8 {
    match koma {
        SFU | GFU => 0,
        SKY | GKY => 1,
        SKE | GKE => 2,
        SGI | GGI => 3,
        SKI | GKI => 4,
        SKA | GKA => 5,
        SHI | GHI => 6,
        STO | GTO => 7,
        SNY | GNY => 8,
        SNK | GNK => 9,
        SNG | GNG => 10,
        SUM | GUM => 11,
        SRY | GRY => 12,
        SOU | GOU => 13,
        _ => 0,
    }
}

#[inline(always)]
pub fn cap_category(cap_koma: u8) -> u8 {
    match cap_koma {
        0 => 0,
        SFU | GFU | STO | GTO => 1,
        SKY | GKY | SKE | GKE | SNY | GNY | SNK | GNK => 2,
        SGI | GGI | SKI | GKI | SNG | GNG => 3,
        _ => 4, // KA, HI, UM, RY
    }
}

#[inline(always)]
fn idx14_to_kind(idx14: u8) -> Kind {
    match idx14 {
        0 => Kind::Pawn,
        1 => Kind::Lance,
        2 => Kind::Knight,
        3 => Kind::Silver,
        4 => Kind::Gold,
        5 => Kind::Bishop,
        6 => Kind::Rook,
        7 => Kind::ProPawn,
        8 => Kind::ProLance,
        9 => Kind::ProKnight,
        10 => Kind::ProSilver,
        11 => Kind::ProBishop,
        12 => Kind::ProRook,
        _ => Kind::King,
    }
}

/// Count how many pieces of `attacker_color` in `atk_bb` (e.g. excluding moved `src`) attack `sq`
/// under occupancy `occ`, capped at 2 (`0, 1, 2`).
#[inline]
pub fn count_attacks_capped(
    pos: &PositionAux,
    attacker_color: Color,
    atk_bb: BitBoard,
    occ: BitBoard,
    sq: Square,
    exclude_king: bool,
) -> u8 {
    let opp = attacker_color.opposite();
    let mut cnt: u8 = 0;

    let near = (king_power(sq) | knight_power(opp, sq)) & atk_bb;
    for p in near {
        let k = pos.must_get_kind(p);
        if exclude_king && k == Kind::King {
            continue;
        }
        let step_hit = match k {
            Kind::Lance | Kind::Bishop | Kind::Rook => false,
            Kind::ProBishop => {
                king_power(sq).contains(p) && (p.col() == sq.col() || p.row() == sq.row())
            }
            Kind::ProRook => {
                king_power(sq).contains(p) && p.col() != sq.col() && p.row() != sq.row()
            }
            _ => power(opp, sq, k).contains(p),
        };
        if step_hit {
            cnt += 1;
            if cnt >= 2 {
                return 2;
            }
        }
    }

    let lances = lance_reachable(occ, opp, sq) & pos.bitboard(attacker_color, Kind::Lance) & atk_bb;
    if !lances.is_empty() {
        cnt = cnt.saturating_add(popcnt(lances));
        if cnt >= 2 {
            return 2;
        }
    }

    let bishops = bishop_reachable(occ, sq) & pos.bishopish() & atk_bb;
    if !bishops.is_empty() {
        cnt = cnt.saturating_add(popcnt(bishops));
        if cnt >= 2 {
            return 2;
        }
    }

    let rooks = rook_reachable(occ, sq) & pos.rookish() & atk_bb;
    if !rooks.is_empty() {
        cnt = cnt.saturating_add(popcnt(rooks));
        if cnt >= 2 {
            return 2;
        }
    }

    cnt.min(2)
}

/// Attack bitboard generated by a single piece of `(color, kind)` standing at `sq` on `occ`.
#[inline]
fn single_piece_attacks(color: Color, kind: Kind, sq: Square, occ: BitBoard) -> BitBoard {
    match kind {
        Kind::Lance => lance_reachable(occ, color, sq),
        Kind::Bishop => bishop_reachable(occ, sq),
        Kind::Rook => rook_reachable(occ, sq),
        Kind::ProBishop => bishop_reachable(occ, sq) | king_power(sq),
        Kind::ProRook => rook_reachable(occ, sq) | king_power(sq),
        _ => power(color, sq, kind),
    }
}

/// Relative square index `0..30` of `dest` w.r.t. `king_sq` (normalized to Sente attacking North):
/// `0..25`: exact 5x5 grid `(df + 2) * 5 + (dr + 2)` where `dr = dest_row - king_row`
///          (`dr = +1` is directly in front/head of White's King for Sente!).
/// `25..30`: outer rays (`25=far_front`, `26=far_back`, `27=far_side`, `28=far_diag_front`, `29=far_diag_back`).
#[inline(always)]
pub fn relative_cell_30(king_sq: usize, dest_sq: usize, attacker_is_gote: bool) -> u8 {
    let kf = (king_sq % 9) as i32;
    let kr = (king_sq / 9) as i32;
    let df_raw = (dest_sq % 9) as i32 - kf;
    let dr_raw = (dest_sq / 9) as i32 - kr;
    let (df, dr) = if attacker_is_gote {
        (-df_raw, -dr_raw)
    } else {
        (df_raw, dr_raw)
    };
    if df.abs() <= 2 && dr.abs() <= 2 {
        ((df + 2) * 5 + (dr + 2)) as u8
    } else if df == 0 {
        if dr > 0 { 25 } else { 26 }
    } else if dr == 0 {
        27
    } else if dr > 0 {
        28
    } else {
        29
    }
}

#[derive(Copy, Clone, Debug, Default, Serialize, Deserialize)]
pub struct OrNodeContext {
    pub king_sq: u8,
    pub king_zone: u8,
    pub safe_flights_cnt: u8,
    pub king_empty_ring1: u8,
    pub king_def_ring1: u8,
    pub king_atk_ring1: u8,
    pub king_atk_ring2: u8,
    pub hand_total: u8,
    pub hand_goldish: u8,
    pub hand_major: u8,
    pub hand_minor: u8,
    pub def_hand_total: u8,
    pub n_checks: u8,
    #[serde(skip)]
    pub safe_flights_bb: BitBoard,
}

#[derive(Copy, Clone, Debug, Default, Serialize, Deserialize)]
pub struct OrMoveFeatures {
    pub piece_after: u8,     // 0..14
    pub move_kind: u8,       // 0=Drop, 1=Prom, 2=NoProm, 3=UnpromInZone
    pub cap_kind: u8,        // 0..=4
    pub rel_cell: u8,        // 0..30
    pub dist: u8,            // 1..=7
    pub dist_delta: u8,      // 0=Drop, 1=Closer, 2=Same, 3=Retreat
    pub atk_sup: u8,         // 0, 1, 2
    pub def_nonking_atk: u8, // 0, 1, 2
    pub king_can_cap: u8,    // 0, 1
    pub see_state: u8,       // 0..6
    pub double_or_disc: u8,  // 0=Direct, 1=Discovered, 2=DoubleCheck
    pub sealed_flights: u8,  // 0..=8
    pub rem_flights: u8,     // 0..=8
    pub hand_after_flag: u8, // 0=Board, 1=UsedLastOfKind, 2=MoreOfKind, 3=HandEmptyAfter
}

#[derive(Copy, Clone, Debug, Default, Serialize, Deserialize)]
pub struct AndNodeContext {
    pub king_sq: u8,
    pub king_zone: u8,
    pub is_double_check: u8,
    pub checker_dist: u8,
    pub checker_piece: u8,
    pub checker_supported: u8,
    pub king_empty_ring1: u8,
    pub atk_hand_total: u8,
    pub def_hand_total: u8,
    pub n_evasions: u8,
}

#[derive(Copy, Clone, Debug, Default, Serialize, Deserialize)]
pub struct AndMoveFeatures {
    pub ev_kind: u8,        // 0=NonKingCap, 1=KingCap, 2=KingFlight, 3=BoardInterpose, 4=DropInterpose
    pub mover_or_drop: u8,  // 0..14
    pub cap_cat: u8,        // 0..=4
    pub atk_on_dest: u8,    // 0, 1, 2 (or ring2 attackers for king flight)
    pub def_sup_dest: u8,   // 0, 1, 2
    pub new_liberties: u8,  // 0..=8 (for king moves)
    pub new_king_zone: u8,  // 0..6
    pub king_rank_dir: u8,  // 0=NotKing, 1=PushedBack, 2=Side, 3=ForwardEscape
}

/// Fast OR context + feature extraction sharing a single `FastBoardAttacks` per OR node (~20 ns).
#[inline]
pub fn extract_or_context_fast(
    fpos: &FmrsPos,
    att: &FastBoardAttacks,
    n_checks: usize,
) -> OrNodeContext {
    let p = fpos.pos_aux();
    let atk_turn = fpos.turn();
    let king_sq = fpos.enemy_ou();
    let ring1 = KING_RING1[king_sq];
    let ring2 = KING_RING2_OUTER[king_sq];
    let occ = p.occupied_bb();
    let atk_bb = p.black_bb();
    let def_bb = p.white_bb();

    let empty_ring1_bb = ring1.and_not(occ);
    let safe_bb = BitBoard::from_u128(empty_ring1_bb.u128() & !att.black_e1);
    let hm = fpos.mkey(atk_turn);
    let dm = fpos.mkey(atk_turn.flip());

    OrNodeContext {
        king_sq: king_sq as u8,
        king_zone: KING_ZONE[king_sq],
        safe_flights_cnt: popcnt(safe_bb),
        king_empty_ring1: popcnt(empty_ring1_bb),
        king_def_ring1: popcnt(ring1 & def_bb),
        king_atk_ring1: popcnt(ring1 & atk_bb),
        king_atk_ring2: popcnt(ring2 & atk_bb).min(15),
        hand_total: hm.total().min(15),
        hand_goldish: (hm.ki() + hm.gi()).min(8),
        hand_major: (hm.hi() + hm.ka()).min(4),
        hand_minor: (hm.fu().min(2) + hm.ky() + hm.ke()).min(8),
        def_hand_total: dm.total().min(15),
        n_checks: n_checks.min(31) as u8,
        safe_flights_bb: safe_bb,
    }
}

#[inline]
pub fn extract_or_move_features_fast(
    fpos: &FmrsPos,
    ctx: &OrNodeContext,
    att: &FastBoardAttacks,
    item: &MvItem,
) -> OrMoveFeatures {
    let p = fpos.pos_aux();
    let mv = item.head_move();
    let dest = mv.dest();
    let dest_sq = SQ_TO_FMRS[dest];
    let dest_idx = dest_sq.index();
    let king_sq = ctx.king_sq as usize;
    let king_fmrs = SQ_TO_FMRS[king_sq];
    let dist = DISTANCE[king_sq][dest];
    let occ = p.occupied_bb();
    let dest_bit = BitBoard::from_u128(1u128 << dest_idx);

    let (piece_after, move_kind, dist_delta, occ_after, hand_after_flag, atk_sup, other_checks, direct_hint) =
        if mv.is_drop() {
            let kt = mv.hand_ktype();
            let p_idx = (kt - 1).min(6);
            let occ_a = occ | dest_bit;
            let hm = fpos.mkey(fpos.turn());
            let rem_kind = hm.get(kt).saturating_sub(1);
            let h_flag = if hm.total() <= 1 {
                3
            } else if rem_kind == 0 {
                1
            } else {
                2
            };
            let sup = att.black_count_at(dest_idx).min(2);
            (p_idx, 0u8, 0u8, occ_a, h_flag, sup, false, true)
        } else {
            let src = mv.prev_pos as usize;
            let src_sq = SQ_TO_FMRS[src];
            let src_bit = BitBoard::from_u128(1u128 << src_sq.index());
            let occ_a = occ.and_not(src_bit) | dest_bit;

            let raw_koma = item.src_koma;
            let final_koma = if mv.is_promote() {
                raw_koma + PROMOTED
            } else {
                raw_koma
            };
            let p_idx = koma_to_idx14(final_koma);
            let in_zone = src / 9 <= 2 || dest / 9 <= 2;
            let can_prom = (raw_koma as usize) < KOMA_PROMOTION.len()
                && KOMA_PROMOTION[raw_koma as usize]
                && in_zone;
            let m_kind = if mv.is_promote() {
                1
            } else if can_prom {
                3
            } else {
                2
            };
            let d_prev = DISTANCE[king_sq][src];
            let d_del = if dist < d_prev {
                1
            } else if dist == d_prev {
                2
            } else {
                3
            };
            let sup = att.black_count_at(dest_idx).saturating_sub(1).min(2);
            let aligned = (src % 9 == king_sq % 9)
                || (src / 9 == king_sq / 9)
                || ((src % 9).abs_diff(king_sq % 9) == (src / 9).abs_diff(king_sq / 9));
            let o_chk = if aligned {
                let atk_other_bb = p.black_bb().and_not(src_bit);
                let l = !(lance_reachable(occ_a, Color::WHITE, king_fmrs)
                    & p.bitboard(Color::BLACK, Kind::Lance)
                    & atk_other_bb)
                    .is_empty();
                let b =
                    !(bishop_reachable(occ_a, king_fmrs) & p.bishopish() & atk_other_bb).is_empty();
                let r = !(rook_reachable(occ_a, king_fmrs) & p.rookish() & atk_other_bb).is_empty();
                l || b || r
            } else {
                false
            };
            (p_idx, m_kind, d_del, occ_a, 0u8, sup, o_chk, !aligned)
        };

    let def_nonking_atk = att.white_nk_count_at(dest_idx);
    let king_can_cap = u8::from(dist == 1 && atk_sup == 0);
    let see_state = if dist == 1 {
        if atk_sup >= 1 && def_nonking_atk == 0 {
            0
        } else if atk_sup >= 1 {
            1
        } else {
            2
        }
    } else if def_nonking_atk == 0 {
        3
    } else if atk_sup >= 1 {
        4
    } else {
        5
    };

    let moved_kind = idx14_to_kind(piece_after);
    let moved_attacks = single_piece_attacks(Color::BLACK, moved_kind, dest_sq, occ_after);
    let direct_check = direct_hint || moved_attacks.contains(king_fmrs);
    let double_or_disc = match (direct_check, other_checks) {
        (true, true) => 2,
        (false, true) => 1,
        _ => 0,
    };

    let newly_covered = (moved_attacks | dest_bit) & ctx.safe_flights_bb;
    let sealed = popcnt(newly_covered);
    let rem_flights = ctx.safe_flights_cnt.saturating_sub(sealed);

    OrMoveFeatures {
        piece_after,
        move_kind,
        cap_kind: cap_category(item.cap_koma),
        rel_cell: relative_cell_30(king_sq, dest, false),
        dist,
        dist_delta,
        atk_sup,
        def_nonking_atk,
        king_can_cap,
        see_state,
        double_or_disc,
        sealed_flights: sealed,
        rem_flights,
        hand_after_flag,
    }
}

/// Fast AND context + feature extraction sharing a single `FastBoardAttacks` per AND node.
#[inline]
pub fn extract_and_context_fast(
    fpos: &FmrsPos,
    att: &FastBoardAttacks,
    n_evasions: usize,
) -> AndNodeContext {
    let p = fpos.pos_aux();
    let def_turn = fpos.turn();
    let atk_turn = def_turn.flip();
    let king_sq = fpos.self_ou();
    let occ = p.occupied_bb();
    let empty_r1 = popcnt(KING_RING1[king_sq].and_not(occ));
    let atk_sq = fpos.attack_sq();
    let atk_fmrs = SQ_TO_FMRS[atk_sq];
    let checker_sup = u8::from(att.black_count_at(atk_fmrs.index()) > 0);

    AndNodeContext {
        king_sq: king_sq as u8,
        king_zone: KING_ZONE[king_sq],
        is_double_check: u8::from(fpos.n_oute() >= 2),
        checker_dist: DISTANCE[king_sq][atk_sq],
        checker_piece: koma_to_idx14(fpos.board_at(atk_sq)),
        checker_supported: checker_sup,
        king_empty_ring1: empty_r1,
        atk_hand_total: fpos.mkey(atk_turn).total().min(15),
        def_hand_total: fpos.mkey(def_turn).total().min(15),
        n_evasions: n_evasions.min(31) as u8,
    }
}

#[inline]
pub fn extract_and_move_features_fast(
    fpos: &FmrsPos,
    ctx: &AndNodeContext,
    att: &FastBoardAttacks,
    item: &MvItem,
) -> AndMoveFeatures {
    let p = fpos.pos_aux();
    let mv = item.head_move();
    let dest = mv.dest();
    let dest_sq = SQ_TO_FMRS[dest];
    let dest_idx = dest_sq.index();
    let king_sq = ctx.king_sq as usize;
    let occ = p.occupied_bb();

    if mv.is_drop() {
        let kt = mv.hand_ktype();
        let atk_on = att.black_count_at(dest_idx).min(2);
        let def_sup = (att.white_nk_count_at(dest_idx) + u8::from(DISTANCE[king_sq][dest] == 1)).min(2);
        return AndMoveFeatures {
            ev_kind: 4,
            mover_or_drop: (kt - 1).min(6),
            cap_cat: 0,
            atk_on_dest: atk_on,
            def_sup_dest: def_sup,
            new_liberties: ctx
                .king_empty_ring1
                .saturating_sub(u8::from(DISTANCE[king_sq][dest] == 1)),
            new_king_zone: ctx.king_zone,
            king_rank_dir: 0,
        };
    }

    let src = mv.prev_pos as usize;
    let src_sq = SQ_TO_FMRS[src];
    let src_bit = BitBoard::from_u128(1u128 << src_sq.index());
    let dest_bit = BitBoard::from_u128(1u128 << dest_idx);
    let occ_a = occ.and_not(src_bit) | dest_bit;
    let is_king = src == king_sq;
    let cap_cat = cap_category(item.cap_koma);

    if is_king {
        let ev_kind = if cap_cat > 0 { 1 } else { 2 };
        let atk_a = p.black_bb().and_not(dest_bit);
        let def_other = p.white_bb().and_not(src_bit);
        let new_lib = popcnt(KING_RING1[dest].and_not(occ_a));
        let atk_near = popcnt(KING_RING2_OUTER[dest] & atk_a).min(4);
        let def_near = popcnt(KING_RING1[dest] & def_other).min(2);
        let dr = (dest / 9) as i32 - (king_sq / 9) as i32;
        let rank_dir = if dr > 0 {
            3
        } else if dr < 0 {
            1
        } else {
            2
        };
        AndMoveFeatures {
            ev_kind,
            mover_or_drop: 13,
            cap_cat,
            atk_on_dest: atk_near,
            def_sup_dest: def_near,
            new_liberties: new_lib,
            new_king_zone: KING_ZONE[dest],
            king_rank_dir: rank_dir,
        }
    } else {
        let ev_kind = if cap_cat > 0 { 0 } else { 3 };
        let final_koma = if mv.is_promote() {
            item.src_koma + PROMOTED
        } else {
            item.src_koma
        };
        let atk_on = att.black_count_at(dest_idx).min(2);
        let def_sup = (att.white_nk_count_at(dest_idx).saturating_sub(1)
            + u8::from(DISTANCE[king_sq][dest] == 1))
        .min(2);
        AndMoveFeatures {
            ev_kind,
            mover_or_drop: koma_to_idx14(final_koma),
            cap_cat,
            atk_on_dest: atk_on,
            def_sup_dest: def_sup,
            new_liberties: ctx.king_empty_ring1,
            new_king_zone: ctx.king_zone,
            king_rank_dir: 0,
        }
    }
}

/// Extract OR-node context once before scoring candidate checks.
#[inline]
pub fn extract_or_context(fpos: &FmrsPos, n_checks: usize) -> OrNodeContext {
    let p = fpos.pos_aux();
    let atk_turn = fpos.turn();
    let atk_color = if atk_turn.is_gote() {
        Color::WHITE
    } else {
        Color::BLACK
    };
    let def_color = atk_color.opposite();
    let king_sq = fpos.enemy_ou();
    let ring1 = KING_RING1[king_sq];
    let ring2 = KING_RING2_OUTER[king_sq];
    let occ = p.occupied_bb();
    let atk_bb = p.color_bb(atk_color);
    let def_bb = p.color_bb(def_color);

    let empty_ring1_bb = ring1.and_not(occ);
    let mut safe_bb = BitBoard::EMPTY;
    let mut safe_cnt = 0u8;
    for sq in empty_ring1_bb {
        if count_attacks_capped(p, atk_color, atk_bb, occ, sq, false) == 0 {
            safe_bb.set(sq);
            safe_cnt += 1;
        }
    }

    let hm = fpos.mkey(atk_turn);
    let dm = fpos.mkey(atk_turn.flip());

    OrNodeContext {
        king_sq: king_sq as u8,
        king_zone: KING_ZONE[king_sq],
        safe_flights_cnt: safe_cnt,
        king_empty_ring1: popcnt(empty_ring1_bb),
        king_def_ring1: popcnt(ring1 & def_bb),
        king_atk_ring1: popcnt(ring1 & atk_bb),
        king_atk_ring2: popcnt(ring2 & atk_bb).min(15),
        hand_total: hm.total().min(15),
        hand_goldish: (hm.ki() + hm.gi()).min(8),
        hand_major: (hm.hi() + hm.ka()).min(4),
        hand_minor: (hm.fu().min(2) + hm.ky() + hm.ke()).min(8),
        def_hand_total: dm.total().min(15),
        n_checks: n_checks.min(31) as u8,
        safe_flights_bb: safe_bb,
    }
}

/// Extract tactical & geometric features for a single candidate check `item` at an OR node.
#[inline]
pub fn extract_or_move_features(
    fpos: &FmrsPos,
    ctx: &OrNodeContext,
    item: &MvItem,
) -> OrMoveFeatures {
    let p = fpos.pos_aux();
    let atk_turn = fpos.turn();
    let is_gote = atk_turn.is_gote();
    let atk_color = if is_gote { Color::WHITE } else { Color::BLACK };
    let def_color = atk_color.opposite();

    let mv = item.head_move();
    let dest = mv.dest();
    let dest_sq = SQ_TO_FMRS[dest];
    let king_sq = ctx.king_sq as usize;
    let king_fmrs = SQ_TO_FMRS[king_sq];
    let dist = DISTANCE[king_sq][dest];

    let occ = p.occupied_bb();
    let mut atk_other_bb = p.color_bb(atk_color);
    let def_bb = p.color_bb(def_color).and_not(BitBoard::from_u128(1u128 << dest_sq.index()));

    let (piece_after, move_kind, dist_delta, occ_after, hand_after_flag, was_src_slider_aligned) =
        if mv.is_drop() {
            let kt = mv.hand_ktype();
            let p_idx = (kt - 1).min(6);
            let occ_a = occ | BitBoard::from_u128(1u128 << dest_sq.index());
            let hm = fpos.mkey(atk_turn);
            let rem_kind = hm.get(kt).saturating_sub(1);
            let h_flag = if hm.total() <= 1 {
                3
            } else if rem_kind == 0 {
                1
            } else {
                2
            };
            (p_idx, 0u8, 0u8, occ_a, h_flag, false)
        } else {
            let src = mv.prev_pos as usize;
            let src_sq = SQ_TO_FMRS[src];
            let src_bit = BitBoard::from_u128(1u128 << src_sq.index());
            atk_other_bb = atk_other_bb.and_not(src_bit);
            let occ_a = occ.and_not(src_bit) | BitBoard::from_u128(1u128 << dest_sq.index());

            let raw_koma = item.src_koma;
            let final_koma = if mv.is_promote() {
                raw_koma + PROMOTED
            } else {
                raw_koma
            };
            let p_idx = koma_to_idx14(final_koma);

            let in_zone = if is_gote {
                src / 9 >= 6 || dest / 9 >= 6
            } else {
                src / 9 <= 2 || dest / 9 <= 2
            };
            let can_prom = (raw_koma as usize) < KOMA_PROMOTION.len()
                && KOMA_PROMOTION[raw_koma as usize]
                && in_zone;
            let m_kind = if mv.is_promote() {
                1
            } else if can_prom {
                3 // Unpromoted in promotion zone (不成!)
            } else {
                2
            };

            let d_prev = DISTANCE[king_sq][src];
            let d_del = if dist < d_prev {
                1
            } else if dist == d_prev {
                2
            } else {
                3
            };
            let aligned = (src % 9 == king_sq % 9)
                || (src / 9 == king_sq / 9)
                || ((src % 9).abs_diff(king_sq % 9) == (src / 9).abs_diff(king_sq / 9));
            (p_idx, m_kind, d_del, occ_a, 0u8, aligned)
        };

    let atk_sup = count_attacks_capped(p, atk_color, atk_other_bb, occ_after, dest_sq, false);
    let def_nonking_atk = count_attacks_capped(p, def_color, def_bb, occ_after, dest_sq, true);
    let king_can_cap = u8::from(dist == 1 && atk_sup == 0);

    let see_state = if dist == 1 {
        if atk_sup >= 1 && def_nonking_atk == 0 {
            0
        } else if atk_sup >= 1 {
            1
        } else {
            2
        }
    } else if def_nonking_atk == 0 {
        3
    } else if atk_sup >= 1 {
        4
    } else {
        5
    };

    let moved_kind = idx14_to_kind(piece_after);
    let moved_attacks = single_piece_attacks(atk_color, moved_kind, dest_sq, occ_after);
    let direct_check = moved_attacks.contains(king_fmrs);

    let other_checks = if was_src_slider_aligned {
        let l = !(lance_reachable(occ_after, def_color, king_fmrs)
            & p.bitboard(atk_color, Kind::Lance)
            & atk_other_bb)
            .is_empty();
        let b = !(bishop_reachable(occ_after, king_fmrs) & p.bishopish() & atk_other_bb).is_empty();
        let r = !(rook_reachable(occ_after, king_fmrs) & p.rookish() & atk_other_bb).is_empty();
        l || b || r
    } else {
        false
    };
    let double_or_disc = match (direct_check, other_checks) {
        (true, true) => 2,
        (false, true) => 1,
        _ => 0,
    };

    // Newly sealed flight squares = safe flights either occupied by `dest` or attacked by moved piece
    let newly_covered =
        (moved_attacks | BitBoard::from_u128(1u128 << dest_sq.index())) & ctx.safe_flights_bb;
    let sealed = popcnt(newly_covered);
    let rem_flights = ctx.safe_flights_cnt.saturating_sub(sealed);

    OrMoveFeatures {
        piece_after,
        move_kind,
        cap_kind: cap_category(item.cap_koma),
        rel_cell: relative_cell_30(king_sq, dest, is_gote),
        dist,
        dist_delta,
        atk_sup,
        def_nonking_atk,
        king_can_cap,
        see_state,
        double_or_disc,
        sealed_flights: sealed,
        rem_flights,
        hand_after_flag,
    }
}

/// Extract AND-node context once before scoring candidate evasions.
#[inline]
pub fn extract_and_context(fpos: &FmrsPos, n_evasions: usize) -> AndNodeContext {
    let p = fpos.pos_aux();
    let def_turn = fpos.turn();
    let atk_turn = def_turn.flip();
    let def_color = if def_turn.is_gote() {
        Color::WHITE
    } else {
        Color::BLACK
    };
    let atk_color = def_color.opposite();

    let king_sq = fpos.self_ou();
    let occ = p.occupied_bb();
    let empty_r1 = popcnt(KING_RING1[king_sq].and_not(occ));

    let atk_sq = fpos.attack_sq();
    let atk_fmrs = SQ_TO_FMRS[atk_sq];
    let atk_other = p
        .color_bb(atk_color)
        .and_not(BitBoard::from_u128(1u128 << atk_fmrs.index()));
    let checker_sup = u8::from(count_attacks_capped(p, atk_color, atk_other, occ, atk_fmrs, false) > 0);

    AndNodeContext {
        king_sq: king_sq as u8,
        king_zone: KING_ZONE[king_sq],
        is_double_check: u8::from(fpos.n_oute() >= 2),
        checker_dist: DISTANCE[king_sq][atk_sq],
        checker_piece: koma_to_idx14(fpos.board_at(atk_sq)),
        checker_supported: checker_sup,
        king_empty_ring1: empty_r1,
        atk_hand_total: fpos.mkey(atk_turn).total().min(15),
        def_hand_total: fpos.mkey(def_turn).total().min(15),
        n_evasions: n_evasions.min(31) as u8,
    }
}

/// Extract features for a single candidate evasion `item` at an AND node.
#[inline]
pub fn extract_and_move_features(
    fpos: &FmrsPos,
    ctx: &AndNodeContext,
    item: &MvItem,
) -> AndMoveFeatures {
    let p = fpos.pos_aux();
    let def_turn = fpos.turn();
    let is_gote = def_turn.is_gote();
    let def_color = if is_gote { Color::WHITE } else { Color::BLACK };
    let atk_color = def_color.opposite();

    let mv = item.head_move();
    let dest = mv.dest();
    let dest_sq = SQ_TO_FMRS[dest];
    let king_sq = ctx.king_sq as usize;
    let occ = p.occupied_bb();
    let atk_bb = p.color_bb(atk_color);
    let def_bb = p.color_bb(def_color);

    if mv.is_drop() {
        let kt = mv.hand_ktype();
        let occ_a = occ | BitBoard::from_u128(1u128 << dest_sq.index());
        let atk_on = count_attacks_capped(p, atk_color, atk_bb, occ_a, dest_sq, false);
        let def_sup = count_attacks_capped(p, def_color, def_bb, occ_a, dest_sq, false);
        return AndMoveFeatures {
            ev_kind: 4,
            mover_or_drop: (kt - 1).min(6),
            cap_cat: 0,
            atk_on_dest: atk_on,
            def_sup_dest: def_sup,
            new_liberties: ctx.king_empty_ring1.saturating_sub(u8::from(DISTANCE[king_sq][dest] == 1)),
            new_king_zone: ctx.king_zone,
            king_rank_dir: 0,
        };
    }

    let src = mv.prev_pos as usize;
    let src_sq = SQ_TO_FMRS[src];
    let src_bit = BitBoard::from_u128(1u128 << src_sq.index());
    let dest_bit = BitBoard::from_u128(1u128 << dest_sq.index());
    let occ_a = occ.and_not(src_bit) | dest_bit;
    let atk_a = atk_bb.and_not(dest_bit);
    let def_other = def_bb.and_not(src_bit);

    let is_king = src == king_sq;
    let cap_cat = cap_category(item.cap_koma);

    if is_king {
        let ev_kind = if cap_cat > 0 { 1 } else { 2 };
        let new_lib = popcnt(KING_RING1[dest].and_not(occ_a));
        let atk_near = popcnt(KING_RING2_OUTER[dest] & atk_a).min(4);
        let def_near = popcnt(KING_RING1[dest] & def_other).min(2);
        let kr = (king_sq / 9) as i32;
        let dr = (dest / 9) as i32 - kr;
        // For Gote king (at top of board, row 0..2), moving to higher row (dr > 0) escapes toward center!
        let forward = if is_gote { dr > 0 } else { dr < 0 };
        let backward = if is_gote { dr < 0 } else { dr > 0 };
        let rank_dir = if forward {
            3
        } else if backward {
            1
        } else {
            2
        };
        AndMoveFeatures {
            ev_kind,
            mover_or_drop: 13,
            cap_cat,
            atk_on_dest: atk_near,
            def_sup_dest: def_near,
            new_liberties: new_lib,
            new_king_zone: KING_ZONE[dest],
            king_rank_dir: rank_dir,
        }
    } else {
        let ev_kind = if cap_cat > 0 { 0 } else { 3 };
        let final_koma = if mv.is_promote() {
            item.src_koma + PROMOTED
        } else {
            item.src_koma
        };
        let atk_on = count_attacks_capped(p, atk_color, atk_a, occ_a, dest_sq, false);
        let def_sup = count_attacks_capped(p, def_color, def_other, occ_a, dest_sq, false);
        AndMoveFeatures {
            ev_kind,
            mover_or_drop: koma_to_idx14(final_koma),
            cap_cat,
            atk_on_dest: atk_on,
            def_sup_dest: def_sup,
            new_liberties: ctx.king_empty_ring1,
            new_king_zone: ctx.king_zone,
            king_rank_dir: 0,
        }
    }
}


// ============================================================================
// Quantized L1/L2-Resident Learned Scorers (`GamV2Model` & `TableNnue16Model`)
// ============================================================================

#[inline(always)]
pub fn or_table_indices_v2(ctx: &OrNodeContext, f: &OrMoveFeatures) -> [usize; 10] {
    let kz = (ctx.king_zone.min(5)) as usize;
    let sf = (ctx.safe_flights_cnt.min(5)) as usize;
    let ar1 = (ctx.king_atk_ring1.min(4)) as usize;
    let dr1 = (ctx.king_def_ring1.min(4)) as usize;
    let ar2_b = ((ctx.king_atk_ring2 / 2).min(3)) as usize;
    let ht_b = match ctx.hand_total {
        0 => 0,
        1 => 1,
        2..=3 => 2,
        4..=6 => 3,
        _ => 4,
    };
    let hg_b = (ctx.hand_goldish.min(2)) as usize;
    let hm_b = (ctx.hand_major.min(1)) as usize;
    let dh_b = match ctx.def_hand_total {
        0 => 0,
        1..=2 => 1,
        3..=5 => 2,
        _ => 3,
    };
    let nc_b = match ctx.n_checks {
        0..=2 => 0,
        3..=4 => 1,
        5..=7 => 2,
        8..=12 => 3,
        _ => 4,
    };

    let pa = (f.piece_after.min(13)) as usize;
    let mk = (f.move_kind.min(3)) as usize;
    let ck = (f.cap_kind.min(4)) as usize;
    let rc = (f.rel_cell.min(29)) as usize;
    let dist = (f.dist.saturating_sub(1).min(6)) as usize;
    let dd = (f.dist_delta.min(3)) as usize;
    let asup = (f.atk_sup.min(2)) as usize;
    let datk = (f.def_nonking_atk.min(2)) as usize;
    let kcap = (f.king_can_cap.min(1)) as usize;
    let see = (f.see_state.min(5)) as usize;
    let disc = (f.double_or_disc.min(2)) as usize;
    let seal = (f.sealed_flights.min(4)) as usize;
    let remf = (f.rem_flights.min(5)) as usize;
    let remf4 = (f.rem_flights.min(3)) as usize;
    let hf = (f.hand_after_flag.min(3)) as usize;

    [
        (pa * 30 + rc) * 6 + kz,
        ((pa * 4 + mk) * 6 + see) * 5 + ck,
        ((sf * 5 + seal) * 6 + remf) * 6 + see,
        ((disc * 7 + dist) * 4 + dd) * 3 + asup,
        ((pa * 4 + hf) * 5 + ht_b) * 3 + datk,
        ((see * 5 + ar1) * 5 + dr1) * 4 + ar2_b,
        (pa * 30 + rc) * 4 + remf4,
        (((pa * 4 + mk) * 6 + see) * 3 + hg_b) * 2 + hm_b,
        ((kcap * 6 + remf) * 14 + pa) * 4 + dh_b,
        ((nc_b * 6 + see) * 4 + mk) * 5 + ck,
    ]
}

#[inline(always)]
pub fn and_table_indices(ctx: &AndNodeContext, f: &AndMoveFeatures) -> [usize; 3] {
    let csup = (ctx.checker_supported.min(1)) as usize;
    let cd_b = match ctx.checker_dist {
        0..=1 => 0,
        2 => 1,
        _ => 2,
    };
    let ek = (f.ev_kind.min(4)) as usize;
    let mv = (f.mover_or_drop.min(13)) as usize;
    let cc = (f.cap_cat.min(4)) as usize;
    let aod = (f.atk_on_dest.min(4)) as usize;
    let dsd = (f.def_sup_dest.min(2)) as usize;
    let nl = (f.new_liberties.min(8)) as usize;
    let nz = (f.new_king_zone.min(5)) as usize;
    let rd = (f.king_rank_dir.min(3)) as usize;

    [
        ((ek * 14 + mv) * 5 + cc) * 2 + csup,
        ((ek * 9 + nl) * 6 + nz) * 4 + rd,
        ((ek * 5 + aod) * 3 + dsd) * 3 + cd_b,
    ]
}

/// 23 KB L1-cache-resident additive `i16` interaction tables (OR + AND).
#[derive(Clone)]
pub struct GamV2Weights {
    pub t0: Box<[i16; 2520]>,
    pub t1: Box<[i16; 1680]>,
    pub t2: Box<[i16; 1080]>,
    pub t3: Box<[i16; 252]>,
    pub t4: Box<[i16; 840]>,
    pub t5: Box<[i16; 600]>,
    pub t6: Box<[i16; 1680]>,
    pub t7: Box<[i16; 2016]>,
    pub t8: Box<[i16; 672]>,
    pub t9: Box<[i16; 600]>,
    pub a0: Box<[i16; 700]>,
    pub a1: Box<[i16; 1080]>,
    pub a2: Box<[i16; 225]>,
}

impl GamV2Weights {
    pub fn from_json_str(s: &str) -> anyhow::Result<Self> {
        let v: serde_json::Value = serde_json::from_str(s)?;
        fn load_arr<const N: usize>(obj: &serde_json::Value, key: &str) -> anyhow::Result<Box<[i16; N]>> {
            let arr = obj
                .get(key)
                .and_then(|x| x.as_array())
                .ok_or_else(|| anyhow::anyhow!("missing array {key}"))?;
            anyhow::ensure!(arr.len() == N, "expected len {N} for {key}, got {}", arr.len());
            let mut out = Box::new([0i16; N]);
            for (i, val) in arr.iter().enumerate() {
                out[i] = val.as_i64().unwrap_or(0) as i16;
            }
            Ok(out)
        }
        let or_t = &v["or_tables"];
        let and_t = &v["and_tables"];
        Ok(Self {
            t0: load_arr(or_t, "T0_piece_rel_zone")?,
            t1: load_arr(or_t, "T1_piece_mk_see_cap")?,
            t2: load_arr(or_t, "T2_flights_see")?,
            t3: load_arr(or_t, "T3_disc_dist_sup")?,
            t4: load_arr(or_t, "T4_hand_drop_exchange")?,
            t5: load_arr(or_t, "T5_net_density")?,
            t6: load_arr(or_t, "T6_piece_rel_remfl")?,
            t7: load_arr(or_t, "T7_sac_followup_mat")?,
            t8: load_arr(or_t, "T8_kingcap_rem_defhand")?,
            t9: load_arr(or_t, "T9_nchecks_see_mk")?,
            a0: load_arr(and_t, "A0_ev_mover_cap_sup")?,
            a1: load_arr(and_t, "A1_king_escape_geom")?,
            a2: load_arr(and_t, "A2_exchange_dist")?,
        })
    }

    #[inline(always)]
    pub fn score_or(&self, idx: &[usize; 10]) -> i32 {
        (self.t0[idx[0]] as i32)
            + (self.t1[idx[1]] as i32)
            + (self.t2[idx[2]] as i32)
            + (self.t3[idx[3]] as i32)
            + (self.t4[idx[4]] as i32)
            + (self.t5[idx[5]] as i32)
            + (self.t6[idx[6]] as i32)
            + (self.t7[idx[7]] as i32)
            + (self.t8[idx[8]] as i32)
            + (self.t9[idx[9]] as i32)
    }

    #[inline(always)]
    pub fn score_and(&self, idx: &[usize; 3]) -> i32 {
        (self.a0[idx[0]] as i32) + (self.a1[idx[1]] as i32) + (self.a2[idx[2]] as i32)
    }
}

/// Table-NNUE-16: 10 scalar + 10 `i16x16` vector tables -> CReLU -> 16-dim dot product (~6 ns/move).
#[derive(Clone)]
pub struct TableNnue16Weights {
    pub base_logit_q8: i32,
    pub vec_bias_q8: [i16; 16],
    pub fc_w_q8: [i16; 16],
    pub s_gam: GamV2Weights,
    pub v0: Box<[[i16; 16]; 2520]>,
    pub v1: Box<[[i16; 16]; 1680]>,
    pub v2: Box<[[i16; 16]; 1080]>,
    pub v3: Box<[[i16; 16]; 252]>,
    pub v4: Box<[[i16; 16]; 840]>,
    pub v5: Box<[[i16; 16]; 600]>,
    pub v6: Box<[[i16; 16]; 1680]>,
    pub v7: Box<[[i16; 16]; 2016]>,
    pub v8: Box<[[i16; 16]; 672]>,
    pub v9: Box<[[i16; 16]; 600]>,
}

impl TableNnue16Weights {
    pub fn from_json_str(s: &str) -> anyhow::Result<Self> {
        let v: serde_json::Value = serde_json::from_str(s)?;
        fn load_s<const N: usize>(obj: &serde_json::Value, key: &str) -> anyhow::Result<Box<[i16; N]>> {
            let arr = obj[key].as_array().ok_or_else(|| anyhow::anyhow!("missing {key}"))?;
            let mut out = Box::new([0i16; N]);
            for (i, val) in arr.iter().enumerate() {
                out[i] = val.as_i64().unwrap_or(0) as i16;
            }
            Ok(out)
        }
        fn load_v<const N: usize>(obj: &serde_json::Value, key: &str) -> anyhow::Result<Box<[[i16; 16]; N]>> {
            let arr = obj[key].as_array().ok_or_else(|| anyhow::anyhow!("missing {key}"))?;
            let mut out = vec![[0i16; 16]; N].into_boxed_slice();
            for (i, row) in arr.iter().enumerate() {
                let r = row.as_array().ok_or_else(|| anyhow::anyhow!("bad row"))?;
                for c in 0..16 {
                    out[i][c] = r[c].as_i64().unwrap_or(0) as i16;
                }
            }
            let ptr = Box::into_raw(out) as *mut [[i16; 16]; N];
            Ok(unsafe { Box::from_raw(ptr) })
        }
        let st = &v["scalar_tables"];
        let vt = &v["vec_tables"];
        let at = &v["and_tables"];
        let mut vec_bias_q8 = [0i16; 16];
        for (c, x) in v["vec_bias_q8"].as_array().unwrap().iter().enumerate() {
            vec_bias_q8[c] = x.as_i64().unwrap_or(0) as i16;
        }
        let mut fc_w_q8 = [0i16; 16];
        for (c, x) in v["fc_w_q8"].as_array().unwrap().iter().enumerate() {
            fc_w_q8[c] = x.as_i64().unwrap_or(0) as i16;
        }
        Ok(Self {
            base_logit_q8: v["base_logit_q8"].as_i64().unwrap_or(0) as i32,
            vec_bias_q8,
            fc_w_q8,
            s_gam: GamV2Weights {
                t0: load_s(st, "T0_piece_rel_zone")?,
                t1: load_s(st, "T1_piece_mk_see_cap")?,
                t2: load_s(st, "T2_flights_see")?,
                t3: load_s(st, "T3_disc_dist_sup")?,
                t4: load_s(st, "T4_hand_drop_exchange")?,
                t5: load_s(st, "T5_net_density")?,
                t6: load_s(st, "T6_piece_rel_remfl")?,
                t7: load_s(st, "T7_sac_followup_mat")?,
                t8: load_s(st, "T8_kingcap_rem_defhand")?,
                t9: load_s(st, "T9_nchecks_see_mk")?,
                a0: load_s(at, "A0_ev_mover_cap_sup")?,
                a1: load_s(at, "A1_king_escape_geom")?,
                a2: load_s(at, "A2_exchange_dist")?,
            },
            v0: load_v(vt, "T0_piece_rel_zone")?,
            v1: load_v(vt, "T1_piece_mk_see_cap")?,
            v2: load_v(vt, "T2_flights_see")?,
            v3: load_v(vt, "T3_disc_dist_sup")?,
            v4: load_v(vt, "T4_hand_drop_exchange")?,
            v5: load_v(vt, "T5_net_density")?,
            v6: load_v(vt, "T6_piece_rel_remfl")?,
            v7: load_v(vt, "T7_sac_followup_mat")?,
            v8: load_v(vt, "T8_kingcap_rem_defhand")?,
            v9: load_v(vt, "T9_nchecks_see_mk")?,
        })
    }

    #[inline(always)]
    pub fn score_or(&self, idx: &[usize; 10]) -> i32 {
        let mut s = self.base_logit_q8 + self.s_gam.score_or(idx);
        let mut h = self.vec_bias_q8;
        let r0 = &self.v0[idx[0]];
        let r1 = &self.v1[idx[1]];
        let r2 = &self.v2[idx[2]];
        let r3 = &self.v3[idx[3]];
        let r4 = &self.v4[idx[4]];
        let r5 = &self.v5[idx[5]];
        let r6 = &self.v6[idx[6]];
        let r7 = &self.v7[idx[7]];
        let r8 = &self.v8[idx[8]];
        let r9 = &self.v9[idx[9]];
        for c in 0..16 {
            h[c] = h[c]
                .saturating_add(r0[c])
                .saturating_add(r1[c])
                .saturating_add(r2[c])
                .saturating_add(r3[c])
                .saturating_add(r4[c])
                .saturating_add(r5[c])
                .saturating_add(r6[c])
                .saturating_add(r7[c])
                .saturating_add(r8[c])
                .saturating_add(r9[c]);
        }
        let mut dot: i32 = 0;
        for c in 0..16 {
            let act = (h[c].clamp(0, 256)) as i32;
            dot += act * (self.fc_w_q8[c] as i32);
        }
        s += dot >> 8;
        s
    }

    #[inline(always)]
    pub fn score_and(&self, idx: &[usize; 3]) -> i32 {
        self.s_gam.score_and(idx)
    }
}

/// Hyperparameters mapping quantized logits (`256 = 1.0 logit`) to `df-pn` (`bn_search`) controls.
#[derive(Copy, Clone, Debug)]
pub struct MlSearchParams {
    /// Set `ml_tie` (1..=255) on OR moves so all `(pn, dn)` ties break by ML score.
    pub use_or_tie: bool,
    /// Set `ml_tie` on AND moves so all `(dn, pn)` ties break by ML refutation score.
    pub use_and_tie: bool,
    /// Unsearched OR leaf `pn_0`: `1 + ((gap * or_pn0_mul) >> 8).min(or_pn0_cap)`
    /// (`gap` = `best_score - score_i` in Q8 units, where 256 = 1.0 logit).
    pub or_pn0_mul: u16,
    pub or_pn0_cap: u16,
    /// Optional safe-zone rank `K`: moves with `rank < safe_or_rank` get `pn_delta = 0`,
    /// while moves with `rank >= safe_or_rank && gap >= tail_gap_q8` get `pn_delta = tail_pn_delta`.
    pub safe_or_rank: usize,
    pub tail_gap_q8: i32,
    pub tail_pn_delta: u8,
    /// Unsearched AND leaf `dn_0`: `1 + ((gap * and_dn0_mul) >> 8).min(and_dn0_cap)`.
    pub and_dn0_mul: u16,
    pub and_dn0_cap: u16,
    /// Optional Override of shtsume's `pn = max(pn, length)`: allow high-logit sliding checks
    /// (`gap <= 128`) to keep `pn = 1` instead of penalizing strong distant dragon/horse checks!
    pub allow_strong_distant_check: bool,
    /// Smooth persistent OR->AND edge penalty: for `gap > or_safe_gap_q8`,
    /// `pn_delta = (((gap - or_safe_gap_q8) * or_pn_delta_mul) >> 8).min(or_pn_delta_cap)`.
    pub or_safe_gap_q8: i32,
    pub or_pn_delta_mul: u16,
    pub or_pn_delta_cap: u8,
    /// Smooth persistent AND->OR edge disproof penalty: for `gap > and_safe_gap_q8`,
    /// `dn_delta = (((gap - and_safe_gap_q8) * and_dn_delta_mul) >> 8).min(and_dn_delta_cap)`.
    pub and_safe_gap_q8: i32,
    pub and_dn_delta_mul: u16,
    pub and_dn_delta_cap: u8,
    /// Optional AND->OR open-king escape proof-number offset (`0` = disabled).
    /// When `ev_escape_persistent == true`, applied as persistent `pn_delta`; when `false`,
    /// applied ONLY to unsearched leaves (`!item.search()`, zero depth-sum blowup!).
    pub ev_escape_pn_scale: u8,
    pub ev_escape_persistent: bool,
}

impl Default for MlSearchParams {
    fn default() -> Self {
        Self {
            use_or_tie: true,
            use_and_tie: true,
            or_pn0_mul: 2,
            or_pn0_cap: 8,
            safe_or_rank: 3,
            tail_gap_q8: 900,
            tail_pn_delta: 0,
            and_dn0_mul: 1,
            and_dn0_cap: 4,
            allow_strong_distant_check: false,
            or_safe_gap_q8: 256,
            or_pn_delta_mul: 0,
            or_pn_delta_cap: 0,
            and_safe_gap_q8: 192,
            and_dn_delta_mul: 0,
            and_dn_delta_cap: 0,
            ev_escape_pn_scale: 0,
            ev_escape_persistent: false,
        }
    }
}

#[inline]
fn apply_or_scores(scores: &[i32], list: &mut [MvItem], p: &MlSearchParams) {
    let n = list.len();
    let mut best_s = i32::MIN;
    for &s in scores {
        if s > best_s {
            best_s = s;
        }
    }

    // Optional rank computation only if `tail_pn_delta > 0`
    if p.tail_pn_delta > 0 && n > p.safe_or_rank {
        // Find score threshold of `safe_or_rank`-th best move without allocating!
        let mut top_k = [i32::MIN; 8];
        let k_lim = p.safe_or_rank.min(8);
        for &s in scores {
            for pos in 0..k_lim {
                if s > top_k[pos] {
                    let mut j = k_lim - 1;
                    while j > pos {
                        top_k[j] = top_k[j - 1];
                        j -= 1;
                    }
                    top_k[pos] = s;
                    break;
                }
            }
        }
        let kth_score = top_k[k_lim - 1];
        for (i, item) in list.iter_mut().enumerate() {
            let s = scores[i];
            let gap = best_s - s;
            if s < kth_score && gap >= p.tail_gap_q8 {
                item.pn_delta = p.tail_pn_delta;
            }
        }
    }

    for (i, item) in list.iter_mut().enumerate() {
        let s = scores[i];
        let gap = (best_s - s).max(0);
        if p.use_or_tie {
            // 1-cycle shift tie-breaker (`gap` is Q8 logits, so `gap >> 3` maps [0..2032] -> 1..=255 without `idiv`)
            item.ml_tie = 1 + ((gap >> 3).min(254) as u8);
        }
        if p.or_pn_delta_cap > 0 && p.or_pn_delta_mul > 0 && gap > p.or_safe_gap_q8 {
            let excess = (gap - p.or_safe_gap_q8) as u32;
            let d = ((excess * (p.or_pn_delta_mul as u32)) >> 8)
                .min(p.or_pn_delta_cap as u32) as u8;
            item.pn_delta = item.pn_delta.max(d);
        }
        let mut init_pn0 = (item.sort_key & 0xff).max(1) as u16;
        if p.allow_strong_distant_check && gap <= 96 {
            init_pn0 = 1;
        }
        if p.or_pn0_cap > 0 && p.or_pn0_mul > 0 {
            let extra = (((gap as u32) * (p.or_pn0_mul as u32)) >> 8) as u16;
            let pn0 = 1 + extra.min(p.or_pn0_cap);
            init_pn0 = init_pn0.max(pn0);
        }
        item.sort_key = init_pn0.min(238) as u64;
        if !item.search() {
            item.tdata.pn = item.tdata.pn.max(init_pn0);
        }
    }
}

#[inline]
fn apply_and_scores(scores: &[i32], list: &mut [MvItem], p: &MlSearchParams) {
    let mut best_board_s = i32::MIN;
    let mut best_all_s = i32::MIN;
    for (i, &s) in scores.iter().enumerate() {
        if s > best_all_s {
            best_all_s = s;
        }
        if !list[i].head_move().is_drop() && s > best_board_s {
            best_board_s = s;
        }
    }
    let has_board = best_board_s > i32::MIN;
    let best_s = if has_board { best_board_s } else { best_all_s };

    for (i, item) in list.iter_mut().enumerate() {
        let s = scores[i];
        let gap = (best_s - s).max(0);
        if p.use_and_tie {
            item.ml_tie = 1 + ((gap >> 3).min(126) as u8);
        }
        if p.and_dn_delta_cap > 0 && p.and_dn_delta_mul > 0 && gap > p.and_safe_gap_q8 {
            let excess = (gap - p.and_safe_gap_q8) as u32;
            let d = ((excess * (p.and_dn_delta_mul as u32)) >> 8)
                .min(p.and_dn_delta_cap.min(7) as u32) as u8;
            item.set_dn_delta(d);
        }
        let mut dn0: u16 = 1;
        if p.and_dn0_cap > 0 && p.and_dn0_mul > 0 {
            let extra = (((gap as u32) * (p.and_dn0_mul as u32)) >> 8) as u16;
            dn0 = 1 + extra.min(p.and_dn0_cap);
            if has_board && item.head_move().is_drop() {
                dn0 = dn0.max(2);
            }
            if !item.search() {
                item.tdata.dn = item.tdata.dn.max(dn0);
            }
        }
        item.sort_key = (item.sort_key & 0xffff) | ((dn0.min(255) as u64) << 16);
    }
}

pub struct GamV2Heuristic<'a, const DELTA: bool = false, const FLOOR: bool = false> {
    pub weights: &'a GamV2Weights,
    pub params: MlSearchParams,
    score_buf: Vec<i32>,
}

impl<'a, const DELTA: bool, const FLOOR: bool> GamV2Heuristic<'a, DELTA, FLOOR> {
    pub fn new(weights: &'a GamV2Weights, params: MlSearchParams) -> Self {
        Self {
            weights,
            params,
            score_buf: Vec::with_capacity(64),
        }
    }
}

impl<'a, const DELTA: bool, const FLOOR: bool> crate::futsuu::solver::SearchHeuristic<FmrsPos>
    for GamV2Heuristic<'a, DELTA, FLOOR>
{
    const HAS_EDGE_DELTA: bool = DELTA;
    const HAS_PN_FLOOR: bool = FLOOR;

    #[inline]
    fn score_or_moves(&mut self, pos: &FmrsPos, list: &mut [MvItem]) {
        if list.len() <= 1 || list.iter().any(|x| x.tdata.pn == 0) {
            return;
        }
        let max_fl = if FLOOR || DELTA {
            self.params.or_pn_delta_cap.max(self.params.tail_pn_delta) as u16
        } else {
            0
        };
        if list.iter().all(|x| x.search() && x.tdata.pn >= max_fl) {
            return;
        }
        if pos.turn().is_gote() {
            let ctx = extract_or_context(pos, list.len());
            self.score_buf.clear();
            for item in list.iter() {
                let f = extract_or_move_features(pos, &ctx, item);
                let idx = or_table_indices_v2(&ctx, &f);
                self.score_buf.push(self.weights.score_or(&idx));
            }
        } else {
            let att = FastBoardAttacks::compute(pos.pos_aux());
            let ctx = extract_or_context_fast(pos, &att, list.len());
            self.score_buf.clear();
            for item in list.iter() {
                let f = extract_or_move_features_fast(pos, &ctx, &att, item);
                let idx = or_table_indices_v2(&ctx, &f);
                self.score_buf.push(self.weights.score_or(&idx));
            }
        }
        apply_or_scores(&self.score_buf, list, &self.params);
    }

    #[inline]
    fn score_and_moves(&mut self, pos: &FmrsPos, list: &mut [MvItem]) {
        let want_and = self.params.use_and_tie
            || self.params.and_dn0_cap > 0
            || self.params.and_dn_delta_cap > 0
            || self.params.ev_escape_pn_scale > 0;
        if !want_and || (list.len() <= 1 && self.params.ev_escape_pn_scale == 0) {
            return;
        }
        if list.iter().any(|x| x.tdata.dn == 0) || (!DELTA && list.iter().all(|x| x.search())) {
            return;
        }
        let att = FastBoardAttacks::compute(pos.pos_aux());
        let ctx = if pos.turn().is_gote() {
            extract_and_context_fast(pos, &att, list.len())
        } else {
            extract_and_context(pos, list.len())
        };
        self.score_buf.clear();
        let esc_scale = self.params.ev_escape_pn_scale;
        for item in list.iter_mut() {
            let f = if pos.turn().is_gote() {
                extract_and_move_features_fast(pos, &ctx, &att, item)
            } else {
                extract_and_move_features(pos, &ctx, item)
            };
            if esc_scale > 0 {
                // Fast child-OR escape difficulty lower bound (King escaping into open squares / capturing checker)
                let mut esc: u8 = 0;
                if f.ev_kind == 1 || f.ev_kind == 2 {
                    let dest = item.head_move().dest();
                    let occ_after = pos
                        .pos_aux()
                        .occupied_bb()
                        .and_not(BitBoard::from_u128(
                            1u128 << SQ_TO_FMRS[ctx.king_sq as usize].index(),
                        ))
                        | BitBoard::from_u128(1u128 << SQ_TO_FMRS[dest].index());
                    let new_safe = popcnt(BitBoard::from_u128(
                        KING_RING1[dest].and_not(occ_after).u128() & !att.black_e1,
                    ));
                    if new_safe >= 2 {
                        esc = esc.saturating_add((new_safe - 1).saturating_mul(esc_scale));
                    }
                    if f.new_king_zone >= 4 && new_safe >= 2 {
                        esc = esc.saturating_add(esc_scale);
                    }
                    if f.ev_kind == 1 && f.cap_cat >= 3 {
                        esc = esc.saturating_add(esc_scale);
                    }
                } else if f.ev_kind == 0 && f.cap_cat >= 3 && f.atk_on_dest == 0 {
                    esc = esc.saturating_add(esc_scale.saturating_mul(2));
                }
                if DELTA && self.params.ev_escape_persistent {
                    item.pn_delta = esc.min(32);
                } else {
                    let pn0 = 1u16.saturating_add(esc.min(64) as u16);
                    item.sort_key = (item.sort_key & !0xffff) | (pn0 as u64);
                    if !item.search() {
                        item.tdata.pn = item.tdata.pn.max(pn0);
                    }
                }
            }
            let idx = and_table_indices(&ctx, &f);
            self.score_buf.push(self.weights.score_and(&idx));
        }
        if list.len() > 1 {
            apply_and_scores(&self.score_buf, list, &self.params);
        }
    }
}

pub struct TableNnueHeuristic<'a, const DELTA: bool = false, const FLOOR: bool = false> {
    pub weights: &'a TableNnue16Weights,
    pub params: MlSearchParams,
    score_buf: Vec<i32>,
}

impl<'a, const DELTA: bool, const FLOOR: bool> TableNnueHeuristic<'a, DELTA, FLOOR> {
    pub fn new(weights: &'a TableNnue16Weights, params: MlSearchParams) -> Self {
        Self {
            weights,
            params,
            score_buf: Vec::with_capacity(64),
        }
    }
}

impl<'a, const DELTA: bool, const FLOOR: bool> crate::futsuu::solver::SearchHeuristic<FmrsPos>
    for TableNnueHeuristic<'a, DELTA, FLOOR>
{
    const HAS_EDGE_DELTA: bool = DELTA;
    const HAS_PN_FLOOR: bool = FLOOR;

    #[inline]
    fn score_or_moves(&mut self, pos: &FmrsPos, list: &mut [MvItem]) {
        if list.len() <= 1 || list.iter().any(|x| x.tdata.pn == 0) {
            return;
        }
        let max_fl = if FLOOR || DELTA {
            self.params.or_pn_delta_cap.max(self.params.tail_pn_delta) as u16
        } else {
            0
        };
        if list.iter().all(|x| x.search() && x.tdata.pn >= max_fl) {
            return;
        }
        let att = FastBoardAttacks::compute(pos.pos_aux());
        let ctx = extract_or_context_fast(pos, &att, list.len());
        self.score_buf.clear();
        for item in list.iter() {
            let f = extract_or_move_features_fast(pos, &ctx, &att, item);
            let idx = or_table_indices_v2(&ctx, &f);
            self.score_buf.push(self.weights.score_or(&idx));
        }
        apply_or_scores(&self.score_buf, list, &self.params);
    }

    #[inline]
    fn score_and_moves(&mut self, pos: &FmrsPos, list: &mut [MvItem]) {
        let want_and = self.params.use_and_tie
            || self.params.and_dn0_cap > 0
            || self.params.and_dn_delta_cap > 0
            || self.params.ev_escape_pn_scale > 0;
        if !want_and || (list.len() <= 1 && self.params.ev_escape_pn_scale == 0) {
            return;
        }
        if list.iter().any(|x| x.tdata.dn == 0) || (!DELTA && list.iter().all(|x| x.search())) {
            return;
        }
        let att = FastBoardAttacks::compute(pos.pos_aux());
        let ctx = extract_and_context_fast(pos, &att, list.len());
        self.score_buf.clear();
        let esc_scale = self.params.ev_escape_pn_scale;
        for item in list.iter_mut() {
            let f = extract_and_move_features_fast(pos, &ctx, &att, item);
            if esc_scale > 0 {
                let mut esc: u8 = 0;
                if f.ev_kind == 1 || f.ev_kind == 2 {
                    let dest = item.head_move().dest();
                    let occ_after = pos
                        .pos_aux()
                        .occupied_bb()
                        .and_not(BitBoard::from_u128(
                            1u128 << SQ_TO_FMRS[ctx.king_sq as usize].index(),
                        ))
                        | BitBoard::from_u128(1u128 << SQ_TO_FMRS[dest].index());
                    let new_safe = popcnt(BitBoard::from_u128(
                        KING_RING1[dest].and_not(occ_after).u128() & !att.black_e1,
                    ));
                    if new_safe >= 2 {
                        esc = esc.saturating_add((new_safe - 1).saturating_mul(esc_scale));
                    }
                    if f.new_king_zone >= 4 && new_safe >= 2 {
                        esc = esc.saturating_add(esc_scale);
                    }
                    if f.ev_kind == 1 && f.cap_cat >= 3 {
                        esc = esc.saturating_add(esc_scale);
                    }
                } else if f.ev_kind == 0 && f.cap_cat >= 3 && f.atk_on_dest == 0 {
                    esc = esc.saturating_add(esc_scale.saturating_mul(2));
                }
                if DELTA && self.params.ev_escape_persistent {
                    item.pn_delta = esc.min(32);
                } else {
                    let pn0 = 1u16.saturating_add(esc.min(64) as u16);
                    item.sort_key = (item.sort_key & !0xffff) | (pn0 as u64);
                    if !item.search() {
                        item.tdata.pn = item.tdata.pn.max(pn0);
                    }
                }
            }
            let idx = and_table_indices(&ctx, &f);
            self.score_buf.push(self.weights.score_and(&idx));
        }
        if list.len() > 1 {
            apply_and_scores(&self.score_buf, list, &self.params);
        }
    }
}

// ============================================================================
// Ultra-Fast L1-Resident `FastLutWeights` (12 KB i16, Zero Per-Move Bitboards)
// ============================================================================

#[derive(Deserialize)]
struct FastLutJson {
    or_tables: std::collections::HashMap<String, Vec<i16>>,
    or_fast_see_tables: std::collections::HashMap<String, Vec<i16>>,
    and_tables: std::collections::HashMap<String, Vec<i16>>,
}

#[repr(C, align(64))]
pub struct FastLutWeights {
    pub r0: [i16; 2520],
    pub r1: [i16; 1120],
    pub r2: [i16; 840],
    pub r3: [i16; 600],
    pub f0: [i16; 2520],
    pub f1: [i16; 1680],
    pub f2: [i16; 144],
    pub f3: [i16; 1008],
    pub a0: [i16; 600],
    pub a1: [i16; 350],
    pub a2: [i16; 70],
}

fn copy_tbl<const N: usize>(v: &[i16]) -> [i16; N] {
    assert_eq!(v.len(), N);
    let mut out = [0i16; N];
    out.copy_from_slice(v);
    out
}

impl FastLutWeights {
    pub fn from_json_str(s: &str) -> anyhow::Result<Box<Self>> {
        let j: FastLutJson = serde_json::from_str(s)?;
        Ok(Box::new(Self {
            r0: copy_tbl(&j.or_tables["R0_piece_rel_kz"]),
            r1: copy_tbl(&j.or_tables["R1_piece_mk_cap_dd"]),
            r2: copy_tbl(&j.or_tables["R2_piece_mk_hand"]),
            r3: copy_tbl(&j.or_tables["R3_rel_mk_nchk"]),
            f0: copy_tbl(&j.or_fast_see_tables["F0_piece_rel_kz"]),
            f1: copy_tbl(&j.or_fast_see_tables["F1_piece_mk_see_cap"]),
            f2: copy_tbl(&j.or_fast_see_tables["F2_sf_dd_see"]),
            f3: copy_tbl(&j.or_fast_see_tables["F3_piece_mk_hand_see"]),
            a0: copy_tbl(&j.and_tables["A0_ev_cap_zone_dir"]),
            a1: copy_tbl(&j.and_tables["A1_ev_mover_lib"]),
            a2: copy_tbl(&j.and_tables["A2_ev_cdist_dbl"]),
        }))
    }
}

pub struct FastLutHeuristic<
    'a,
    const ENABLE_OR: bool = true,
    const USE_SEE_OR: bool = false,
    const DELTA: bool = false,
> {
    pub weights: &'a FastLutWeights,
    pub params: MlSearchParams,
}

impl<'a, const ENABLE_OR: bool, const USE_SEE_OR: bool, const DELTA: bool>
    FastLutHeuristic<'a, ENABLE_OR, USE_SEE_OR, DELTA>
{
    #[inline(always)]
    pub fn new(weights: &'a FastLutWeights, params: MlSearchParams) -> Self {
        Self { weights, params }
    }
}

impl<'a, const ENABLE_OR: bool, const USE_SEE_OR: bool, const DELTA: bool>
    crate::futsuu::solver::SearchHeuristic<FmrsPos>
    for FastLutHeuristic<'a, ENABLE_OR, USE_SEE_OR, DELTA>
{
    const HAS_EDGE_DELTA: bool = DELTA;

    #[inline]
    fn score_or_moves(&mut self, pos: &FmrsPos, list: &mut [MvItem]) {
        if !ENABLE_OR || list.len() <= 1 || list.iter().any(|x| x.tdata.pn == 0) {
            return;
        }
        if list.iter().all(|x| x.search()) {
            return;
        }
        let king_sq = pos.enemy_ou();
        let kz = (KING_ZONE[king_sq].min(5)) as usize;
        let hm = pos.mkey(pos.turn());
        let hg_b = ((hm.ki() + hm.gi()).min(2)) as usize;
        let ht_b = match hm.total() {
            0 => 0,
            1 => 1,
            2..=3 => 2,
            4..=6 => 3,
            _ => 4,
        };
        let nc_b = match list.len() {
            0..=2 => 0,
            3..=4 => 1,
            5..=7 => 2,
            8..=12 => 3,
            _ => 4,
        };
        let is_gote = pos.turn().is_gote();
        let n = list.len().min(64);
        let mut scores = [0i32; 64];

        if USE_SEE_OR && !is_gote {
            let att = FastBoardAttacks::compute(pos.pos_aux());
            let empty_r1 = KING_RING1[king_sq].and_not(pos.pos_aux().occupied_bb());
            let sf = (popcnt(BitBoard::from_u128(empty_r1.u128() & !att.black_e1)).min(5)) as usize;
            for (k_i, item) in list[..n].iter().enumerate() {
                let mv = item.head_move();
                let dest = mv.dest();
                let dest_idx = SQ_TO_FMRS[dest].index();
                let dist = DISTANCE[king_sq][dest];
                let (pa, mk, dd, atk_sup) = if mv.is_drop() {
                    let p_idx = ((mv.hand_ktype() - 1).min(6)) as usize;
                    let sup = att.black_count_at(dest_idx).min(2);
                    (p_idx, 0usize, 0usize, sup)
                } else {
                    let src = mv.prev_pos as usize;
                    let raw = item.src_koma;
                    let fk = if mv.is_promote() { raw + PROMOTED } else { raw };
                    let p_idx = (koma_to_idx14(fk).min(13)) as usize;
                    let in_z = src / 9 <= 2 || dest / 9 <= 2;
                    let m_k = if mv.is_promote() {
                        1
                    } else if (raw as usize) < KOMA_PROMOTION.len()
                        && KOMA_PROMOTION[raw as usize]
                        && in_z
                    {
                        3
                    } else {
                        2
                    };
                    let dp = DISTANCE[king_sq][src];
                    let d_d = if dist < dp {
                        1
                    } else if dist == dp {
                        2
                    } else {
                        3
                    };
                    let sup = att.black_count_at(dest_idx).saturating_sub(1).min(2);
                    (p_idx, m_k, d_d, sup)
                };
                let def_nk = att.white_nk_count_at(dest_idx);
                let see = if dist == 1 {
                    if atk_sup >= 1 && def_nk == 0 {
                        0
                    } else if atk_sup >= 1 {
                        1
                    } else {
                        2
                    }
                } else if def_nk == 0 {
                    3
                } else if atk_sup >= 1 {
                    4
                } else {
                    5
                };
                let ck = (cap_category(item.cap_koma).min(4)) as usize;
                let rc = (relative_cell_30(king_sq, dest, false).min(29)) as usize;
                scores[k_i] = unsafe {
                    (*self.weights.f0.get_unchecked((pa * 30 + rc) * 6 + kz) as i32)
                        + (*self
                            .weights
                            .f1
                            .get_unchecked(((pa * 4 + mk) * 6 + see) * 5 + ck)
                            as i32)
                        + (*self.weights.f2.get_unchecked((sf * 4 + dd) * 6 + see) as i32)
                        + (*self
                            .weights
                            .f3
                            .get_unchecked(((pa * 4 + mk) * 3 + hg_b) * 6 + see)
                            as i32)
                };
            }
        } else {
            for (k_i, item) in list[..n].iter().enumerate() {
                let mv = item.head_move();
                let dest = mv.dest();
                let dist = DISTANCE[king_sq][dest];
                let (pa, mk, dd) = if mv.is_drop() {
                    (((mv.hand_ktype() - 1).min(6)) as usize, 0usize, 0usize)
                } else {
                    let src = mv.prev_pos as usize;
                    let raw = item.src_koma;
                    let fk = if mv.is_promote() { raw + PROMOTED } else { raw };
                    let p_idx = (koma_to_idx14(fk).min(13)) as usize;
                    let in_z = if is_gote {
                        src / 9 >= 6 || dest / 9 >= 6
                    } else {
                        src / 9 <= 2 || dest / 9 <= 2
                    };
                    let m_k = if mv.is_promote() {
                        1
                    } else if (raw as usize) < KOMA_PROMOTION.len()
                        && KOMA_PROMOTION[raw as usize]
                        && in_z
                    {
                        3
                    } else {
                        2
                    };
                    let dp = DISTANCE[king_sq][src];
                    let d_d = if dist < dp {
                        1
                    } else if dist == dp {
                        2
                    } else {
                        3
                    };
                    (p_idx, m_k, d_d)
                };
                let ck = (cap_category(item.cap_koma).min(4)) as usize;
                let rc = (relative_cell_30(king_sq, dest, is_gote).min(29)) as usize;
                scores[k_i] = unsafe {
                    (*self.weights.r0.get_unchecked((pa * 30 + rc) * 6 + kz) as i32)
                        + (*self
                            .weights
                            .r1
                            .get_unchecked(((pa * 4 + mk) * 5 + ck) * 4 + dd)
                            as i32)
                        + (*self
                            .weights
                            .r2
                            .get_unchecked(((pa * 4 + mk) * 3 + hg_b) * 5 + ht_b)
                            as i32)
                        + (*self.weights.r3.get_unchecked((rc * 4 + mk) * 5 + nc_b) as i32)
                };
            }
        }
        apply_or_scores(&scores[..n], &mut list[..n], &self.params);
    }

    #[inline]
    fn score_and_moves(&mut self, pos: &FmrsPos, list: &mut [MvItem]) {
        if list.len() <= 1 || list.iter().any(|x| x.tdata.dn == 0) {
            return;
        }
        if !DELTA && list.iter().all(|x| x.search()) {
            return;
        }
        let king_sq = pos.self_ou();
        let kz = (KING_ZONE[king_sq].min(5)) as usize;
        let atk_sq = pos.attack_sq();
        let cdist = (DISTANCE[king_sq][atk_sq].saturating_sub(1).min(6)) as usize;
        let dbl = usize::from(pos.n_oute() >= 2);
        let occ = pos.pos_aux().occupied_bb();
        let empty_r1 = popcnt(KING_RING1[king_sq].and_not(occ));
        let is_gote = pos.turn().is_gote();

        let n = list.len().min(64);
        let mut scores = [0i32; 64];
        for (k_i, item) in list[..n].iter().enumerate() {
            let mv = item.head_move();
            let dest = mv.dest();
            let (ev, mover, cc, nz, rd, nl) = if mv.is_drop() {
                let kt = ((mv.hand_ktype() - 1).min(6)) as usize;
                let nlib = (empty_r1.saturating_sub(u8::from(DISTANCE[king_sq][dest] == 1)).min(4))
                    as usize;
                (4usize, kt, 0usize, kz, 0usize, nlib)
            } else {
                let src = mv.prev_pos as usize;
                let c_cat = (cap_category(item.cap_koma).min(4)) as usize;
                if src == king_sq {
                    let ev_k = if c_cat > 0 { 1usize } else { 2usize };
                    let occ_a = occ
                        .and_not(BitBoard::from_u128(1u128 << SQ_TO_FMRS[src].index()))
                        | BitBoard::from_u128(1u128 << SQ_TO_FMRS[dest].index());
                    let nlib = (popcnt(KING_RING1[dest].and_not(occ_a)).min(4)) as usize;
                    let dr = (dest / 9) as i32 - (king_sq / 9) as i32;
                    let fwd = if is_gote { dr > 0 } else { dr < 0 };
                    let bwd = if is_gote { dr < 0 } else { dr > 0 };
                    let r_d = if fwd {
                        3usize
                    } else if bwd {
                        1usize
                    } else {
                        2usize
                    };
                    (ev_k, 13usize, c_cat, (KING_ZONE[dest].min(5)) as usize, r_d, nlib)
                } else {
                    let ev_k = if c_cat > 0 { 0usize } else { 3usize };
                    let fk = if mv.is_promote() {
                        item.src_koma + PROMOTED
                    } else {
                        item.src_koma
                    };
                    (
                        ev_k,
                        (koma_to_idx14(fk).min(13)) as usize,
                        c_cat,
                        kz,
                        0usize,
                        (empty_r1.min(4)) as usize,
                    )
                }
            };
            scores[k_i] = unsafe {
                (*self
                    .weights
                    .a0
                    .get_unchecked(((ev * 5 + cc) * 6 + nz) * 4 + rd) as i32)
                    + (*self.weights.a1.get_unchecked((ev * 14 + mover) * 5 + nl) as i32)
                    + (*self.weights.a2.get_unchecked((ev * 7 + cdist) * 2 + dbl) as i32)
            };
        }
        apply_and_scores(&scores[..n], &mut list[..n], &self.params);
    }
}

// ============================================================================
// Distilled L1-Resident `FastLutV2Weights` (17.5 KB i16: 12.8 KB OR + 4.7 KB AND)
// ============================================================================

#[derive(Deserialize)]
struct FastLutV2Json {
    or_reg_tables: std::collections::HashMap<String, Vec<i16>>,
    or_fast_v2_tables: std::collections::HashMap<String, Vec<i16>>,
    and_reg_tables: std::collections::HashMap<String, Vec<i16>>,
    and_fast_v2_tables: std::collections::HashMap<String, Vec<i16>>,
}

#[repr(C, align(64))]
pub struct FastLutV2Weights {
    pub r0: [i16; 2520],
    pub r1: [i16; 1120],
    pub r2: [i16; 840],
    pub r3: [i16; 600],
    pub f0: [i16; 2520],
    pub f1: [i16; 1680],
    pub f2: [i16; 432],
    pub f3: [i16; 1008],
    pub f4: [i16; 900],
    pub ar0: [i16; 600],
    pub ar1: [i16; 350],
    pub ar2: [i16; 70],
    pub a0: [i16; 700],
    pub a1: [i16; 1080],
    pub a2: [i16; 225],
    pub a3: [i16; 420],
}

impl FastLutV2Weights {
    pub fn from_json_str(s: &str) -> anyhow::Result<Box<Self>> {
        let j: FastLutV2Json = serde_json::from_str(s)?;
        let at = &j.and_fast_v2_tables;
        Ok(Box::new(Self {
            r0: copy_tbl(&j.or_reg_tables["R0_piece_rel_kz"]),
            r1: copy_tbl(&j.or_reg_tables["R1_piece_mk_cap_dd"]),
            r2: copy_tbl(&j.or_reg_tables["R2_piece_mk_hand"]),
            r3: copy_tbl(&j.or_reg_tables["R3_rel_mk_nchk"]),
            f0: copy_tbl(&j.or_fast_v2_tables["F0_piece_rel_kz"]),
            f1: copy_tbl(&j.or_fast_v2_tables["F1_piece_mk_see_cap"]),
            f2: copy_tbl(&j.or_fast_v2_tables["F2_sf_dd_see_ar1"]),
            f3: copy_tbl(&j.or_fast_v2_tables["F3_piece_mk_hg_see"]),
            f4: copy_tbl(&j.or_fast_v2_tables["F4_rel_see_nchk"]),
            ar0: copy_tbl(&j.and_reg_tables["A0_ev_cap_zone_dir"]),
            ar1: copy_tbl(&j.and_reg_tables["A1_ev_mover_lib"]),
            ar2: copy_tbl(&j.and_reg_tables["A2_ev_cdist_dbl"]),
            a0: copy_tbl(&at["A0_ev_mover_cap_csup"]),
            a1: copy_tbl(&at["A1_ev_lib_zone_dir"]),
            a2: copy_tbl(&at["A2_ev_aod_dsd_cdist"]),
            a3: copy_tbl(&at["A3_ev_cpiece_csup_nev"]),
        }))
    }
}

pub struct FastLutV2Heuristic<
    'a,
    const ENABLE_OR: bool = true,
    const USE_SEE_OR: bool = true,
    const TACTICAL_AND: bool = true,
    const DELTA: bool = false,
    const FIRST_VISIT_OR_ONLY: bool = false,
> {
    pub weights: &'a FastLutV2Weights,
    pub params: MlSearchParams,
}

impl<
        'a,
        const ENABLE_OR: bool,
        const USE_SEE_OR: bool,
        const TACTICAL_AND: bool,
        const DELTA: bool,
        const FIRST_VISIT_OR_ONLY: bool,
    > FastLutV2Heuristic<'a, ENABLE_OR, USE_SEE_OR, TACTICAL_AND, DELTA, FIRST_VISIT_OR_ONLY>
{
    #[inline(always)]
    pub fn new(weights: &'a FastLutV2Weights, params: MlSearchParams) -> Self {
        Self { weights, params }
    }
}

impl<
        'a,
        const ENABLE_OR: bool,
        const USE_SEE_OR: bool,
        const TACTICAL_AND: bool,
        const DELTA: bool,
        const FIRST_VISIT_OR_ONLY: bool,
    > crate::futsuu::solver::SearchHeuristic<FmrsPos>
    for FastLutV2Heuristic<'a, ENABLE_OR, USE_SEE_OR, TACTICAL_AND, DELTA, FIRST_VISIT_OR_ONLY>
{
    const HAS_EDGE_DELTA: bool = DELTA;

    #[inline]
    fn score_or_moves(&mut self, pos: &FmrsPos, list: &mut [MvItem]) {
        if !ENABLE_OR || list.len() <= 1 || list.iter().any(|x| x.tdata.pn == 0) {
            return;
        }
        if FIRST_VISIT_OR_ONLY {
            if list.iter().any(|x| x.search()) {
                return;
            }
        } else if list.iter().all(|x| x.search()) {
            return;
        }
        let king_sq = pos.enemy_ou();
        let kz = (KING_ZONE[king_sq].min(5)) as usize;
        let hm = pos.mkey(pos.turn());
        let hg_b = ((hm.ki() + hm.gi()).min(2)) as usize;
        let nc_b = match list.len() {
            0..=2 => 0,
            3..=4 => 1,
            5..=7 => 2,
            8..=12 => 3,
            _ => 4,
        };
        let is_gote = pos.turn().is_gote();
        let n = list.len().min(64);
        let mut scores = [0i32; 64];

        if USE_SEE_OR && !is_gote {
            let p = pos.pos_aux();
            let (be1, be2, we1) = FastBoardAttacks::compute_see_masks(p);
            let ring1 = KING_RING1[king_sq];
            let empty_r1 = ring1.and_not(p.occupied_bb());
            let sf = ((empty_r1.u128() & !be1).count_ones().min(5)) as usize;
            let ar1 = (popcnt(ring1 & p.black_bb()).min(2)) as usize;

            for (k_i, item) in list[..n].iter().enumerate() {
                let mv = item.head_move();
                let dest = mv.dest();
                let dest_idx = SQ_TO_FMRS[dest].index();
                let dist = DISTANCE[king_sq][dest];
                let (pa, mk, dd, atk_sup_ge1) = if mv.is_drop() {
                    let p_idx = ((mv.hand_ktype() - 1).min(6)) as usize;
                    let sup = ((be1 >> dest_idx) & 1) != 0;
                    (p_idx, 0usize, 0usize, sup)
                } else {
                    let src = mv.prev_pos as usize;
                    let raw = item.src_koma;
                    let fk = if mv.is_promote() { raw + PROMOTED } else { raw };
                    let p_idx = (koma_to_idx14(fk).min(13)) as usize;
                    let in_z = src / 9 <= 2 || dest / 9 <= 2;
                    let m_k = if mv.is_promote() {
                        1
                    } else if (raw as usize) < KOMA_PROMOTION.len()
                        && KOMA_PROMOTION[raw as usize]
                        && in_z
                    {
                        3
                    } else {
                        2
                    };
                    let dp = DISTANCE[king_sq][src];
                    let d_d = if dist < dp {
                        1
                    } else if dist == dp {
                        2
                    } else {
                        3
                    };
                    let sup = ((be2 >> dest_idx) & 1) != 0;
                    (p_idx, m_k, d_d, sup)
                };
                let def_nk_eq0 = ((we1 >> dest_idx) & 1) == 0;
                let see = if dist == 1 {
                    if atk_sup_ge1 && def_nk_eq0 {
                        0
                    } else if atk_sup_ge1 {
                        1
                    } else {
                        2
                    }
                } else if def_nk_eq0 {
                    3
                } else if atk_sup_ge1 {
                    4
                } else {
                    5
                };
                let ck = (cap_category(item.cap_koma).min(4)) as usize;
                let rc = (relative_cell_30(king_sq, dest, false).min(29)) as usize;
                scores[k_i] = unsafe {
                    (*self.weights.f0.get_unchecked((pa * 30 + rc) * 6 + kz) as i32)
                        + (*self
                            .weights
                            .f1
                            .get_unchecked(((pa * 4 + mk) * 6 + see) * 5 + ck)
                            as i32)
                        + (*self
                            .weights
                            .f2
                            .get_unchecked(((sf * 4 + dd) * 6 + see) * 3 + ar1)
                            as i32)
                        + (*self
                            .weights
                            .f3
                            .get_unchecked(((pa * 4 + mk) * 3 + hg_b) * 6 + see)
                            as i32)
                        + (*self.weights.f4.get_unchecked((rc * 6 + see) * 5 + nc_b) as i32)
                };
            }
        } else {
            let ht_b = match hm.total() {
                0 => 0,
                1 => 1,
                2..=3 => 2,
                4..=6 => 3,
                _ => 4,
            };
            for (k_i, item) in list[..n].iter().enumerate() {
                let mv = item.head_move();
                let dest = mv.dest();
                let dist = DISTANCE[king_sq][dest];
                let (pa, mk, dd) = if mv.is_drop() {
                    (((mv.hand_ktype() - 1).min(6)) as usize, 0usize, 0usize)
                } else {
                    let src = mv.prev_pos as usize;
                    let raw = item.src_koma;
                    let fk = if mv.is_promote() { raw + PROMOTED } else { raw };
                    let p_idx = (koma_to_idx14(fk).min(13)) as usize;
                    let in_z = if is_gote {
                        src / 9 >= 6 || dest / 9 >= 6
                    } else {
                        src / 9 <= 2 || dest / 9 <= 2
                    };
                    let m_k = if mv.is_promote() {
                        1
                    } else if (raw as usize) < KOMA_PROMOTION.len()
                        && KOMA_PROMOTION[raw as usize]
                        && in_z
                    {
                        3
                    } else {
                        2
                    };
                    let dp = DISTANCE[king_sq][src];
                    let d_d = if dist < dp {
                        1
                    } else if dist == dp {
                        2
                    } else {
                        3
                    };
                    (p_idx, m_k, d_d)
                };
                let ck = (cap_category(item.cap_koma).min(4)) as usize;
                let rc = (relative_cell_30(king_sq, dest, is_gote).min(29)) as usize;
                scores[k_i] = unsafe {
                    (*self.weights.r0.get_unchecked((pa * 30 + rc) * 6 + kz) as i32)
                        + (*self
                            .weights
                            .r1
                            .get_unchecked(((pa * 4 + mk) * 5 + ck) * 4 + dd)
                            as i32)
                        + (*self
                            .weights
                            .r2
                            .get_unchecked(((pa * 4 + mk) * 3 + hg_b) * 5 + ht_b)
                            as i32)
                        + (*self.weights.r3.get_unchecked((rc * 4 + mk) * 5 + nc_b) as i32)
                };
            }
        }
        apply_or_scores(&scores[..n], &mut list[..n], &self.params);
    }

    #[inline]
    fn score_and_moves(&mut self, pos: &FmrsPos, list: &mut [MvItem]) {
        if list.len() <= 1 || list.iter().any(|x| x.tdata.dn == 0) {
            return;
        }
        if !DELTA && list.iter().all(|x| x.search()) {
            return;
        }
        let p = pos.pos_aux();
        let king_sq = pos.self_ou();
        let kz = (KING_ZONE[king_sq].min(5)) as usize;
        let atk_sq = pos.attack_sq();
        let cdist_raw = DISTANCE[king_sq][atk_sq];
        let occ = p.occupied_bb();
        let empty_r1 = popcnt(KING_RING1[king_sq].and_not(occ));
        let is_gote = pos.turn().is_gote();
        let n = list.len().min(64);
        let mut scores = [0i32; 64];

        if TACTICAL_AND && is_gote {
            let atk_fmrs = SQ_TO_FMRS[atk_sq];
            let csup = usize::from(has_attack_on(p, Color::BLACK, atk_fmrs));
            let cd_b = if cdist_raw <= 1 {
                0usize
            } else if cdist_raw == 2 {
                1usize
            } else {
                2usize
            };
            let cpiece = (koma_to_idx14(pos.board_at(atk_sq)).min(13)) as usize;
            let nev_b = match list.len() {
                0..=2 => 0usize,
                3..=5 => 1usize,
                _ => 2usize,
            };
            let black_bb = p.black_bb();
            let white_bb = p.white_bb();
            let king_bit = BitBoard::from_u128(1u128 << SQ_TO_FMRS[king_sq].index());

            for (k_i, item) in list[..n].iter().enumerate() {
                let mv = item.head_move();
                let dest = mv.dest();
                let dest_fmrs = SQ_TO_FMRS[dest];
                let (ek, mv_idx, cc, aod, dsd, nl, nz, rd) = if mv.is_drop() {
                    let kt = ((mv.hand_ktype() - 1).min(6)) as usize;
                    let king_adj = u8::from(DISTANCE[king_sq][dest] == 1);
                    let nlib = (empty_r1.saturating_sub(king_adj).min(8)) as usize;
                    let a_on = usize::from(has_attack_on(p, Color::BLACK, dest_fmrs));
                    let w_nk = u8::from(has_attack_on_masked(
                        p,
                        Color::WHITE,
                        white_bb,
                        occ,
                        dest_fmrs,
                        true,
                    ));
                    let d_sup = ((w_nk + king_adj).min(2)) as usize;
                    (4usize, kt, 0usize, a_on, d_sup, nlib, kz, 0usize)
                } else {
                    let src = mv.prev_pos as usize;
                    let c_cat = (cap_category(item.cap_koma).min(4)) as usize;
                    if src == king_sq {
                        let ev_k = if c_cat > 0 { 1usize } else { 2usize };
                        let dest_bit = BitBoard::from_u128(1u128 << dest_fmrs.index());
                        let occ_a = occ.and_not(king_bit) | dest_bit;
                        let nlib = (popcnt(KING_RING1[dest].and_not(occ_a)).min(8)) as usize;
                        let atk_near =
                            (popcnt(KING_RING2_OUTER[dest] & black_bb.and_not(dest_bit)).min(4))
                                as usize;
                        let def_near =
                            (popcnt(KING_RING1[dest] & white_bb.and_not(king_bit)).min(2))
                                as usize;
                        let dr = (dest / 9) as i32 - (king_sq / 9) as i32;
                        let r_d = if dr > 0 {
                            3usize
                        } else if dr < 0 {
                            1usize
                        } else {
                            2usize
                        };
                        (
                            ev_k,
                            13usize,
                            c_cat,
                            atk_near,
                            def_near,
                            nlib,
                            (KING_ZONE[dest].min(5)) as usize,
                            r_d,
                        )
                    } else {
                        let ev_k = if c_cat > 0 { 0usize } else { 3usize };
                        let fk = if mv.is_promote() {
                            item.src_koma + PROMOTED
                        } else {
                            item.src_koma
                        };
                        let a_on = if ev_k == 0 {
                            csup
                        } else {
                            usize::from(has_attack_on(p, Color::BLACK, dest_fmrs))
                        };
                        let src_bit = BitBoard::from_u128(1u128 << SQ_TO_FMRS[src].index());
                        let w_nk = u8::from(has_attack_on_masked(
                            p,
                            Color::WHITE,
                            white_bb.and_not(src_bit),
                            occ,
                            dest_fmrs,
                            true,
                        ));
                        let king_adj = u8::from(DISTANCE[king_sq][dest] == 1);
                        let d_sup = ((w_nk + king_adj).min(2)) as usize;
                        (
                            ev_k,
                            (koma_to_idx14(fk).min(13)) as usize,
                            c_cat,
                            a_on,
                            d_sup,
                            (empty_r1.min(8)) as usize,
                            kz,
                            0usize,
                        )
                    }
                };
                scores[k_i] = unsafe {
                    (*self
                        .weights
                        .a0
                        .get_unchecked(((ek * 14 + mv_idx) * 5 + cc) * 2 + csup)
                        as i32)
                        + (*self
                            .weights
                            .a1
                            .get_unchecked(((ek * 9 + nl) * 6 + nz) * 4 + rd)
                            as i32)
                        + (*self
                            .weights
                            .a2
                            .get_unchecked(((ek * 5 + aod) * 3 + dsd) * 3 + cd_b)
                            as i32)
                        + (*self
                            .weights
                            .a3
                            .get_unchecked(((ek * 14 + cpiece) * 2 + csup) * 3 + nev_b)
                            as i32)
                };
            }
        } else {
            let cdist = (cdist_raw.saturating_sub(1).min(6)) as usize;
            let dbl = usize::from(pos.n_oute() >= 2);
            for (k_i, item) in list[..n].iter().enumerate() {
                let mv = item.head_move();
                let dest = mv.dest();
                let (ev, mover, cc, nz, rd, nl) = if mv.is_drop() {
                    let kt = ((mv.hand_ktype() - 1).min(6)) as usize;
                    let nlib =
                        (empty_r1.saturating_sub(u8::from(DISTANCE[king_sq][dest] == 1)).min(4))
                            as usize;
                    (4usize, kt, 0usize, kz, 0usize, nlib)
                } else {
                    let src = mv.prev_pos as usize;
                    let c_cat = (cap_category(item.cap_koma).min(4)) as usize;
                    if src == king_sq {
                        let ev_k = if c_cat > 0 { 1usize } else { 2usize };
                        let occ_a = occ
                            .and_not(BitBoard::from_u128(1u128 << SQ_TO_FMRS[src].index()))
                            | BitBoard::from_u128(1u128 << SQ_TO_FMRS[dest].index());
                        let nlib = (popcnt(KING_RING1[dest].and_not(occ_a)).min(4)) as usize;
                        let dr = (dest / 9) as i32 - (king_sq / 9) as i32;
                        let fwd = if is_gote { dr > 0 } else { dr < 0 };
                        let bwd = if is_gote { dr < 0 } else { dr > 0 };
                        let r_d = if fwd {
                            3usize
                        } else if bwd {
                            1usize
                        } else {
                            2usize
                        };
                        (ev_k, 13usize, c_cat, (KING_ZONE[dest].min(5)) as usize, r_d, nlib)
                    } else {
                        let ev_k = if c_cat > 0 { 0usize } else { 3usize };
                        let fk = if mv.is_promote() {
                            item.src_koma + PROMOTED
                        } else {
                            item.src_koma
                        };
                        (
                            ev_k,
                            (koma_to_idx14(fk).min(13)) as usize,
                            c_cat,
                            kz,
                            0usize,
                            (empty_r1.min(4)) as usize,
                        )
                    }
                };
                scores[k_i] = unsafe {
                    (*self
                        .weights
                        .ar0
                        .get_unchecked(((ev * 5 + cc) * 6 + nz) * 4 + rd)
                        as i32)
                        + (*self.weights.ar1.get_unchecked((ev * 14 + mover) * 5 + nl) as i32)
                        + (*self.weights.ar2.get_unchecked((ev * 7 + cdist) * 2 + dbl) as i32)
                };
            }
        }
        apply_and_scores(&scores[..n], &mut list[..n], &self.params);
    }
}

/// Default built-in `FmrsPos` AND-node refutation scorer using static L1-resident
/// `DEFAULT_LUT_WEIGHTS` (`2.0 KB` zero-magic-bitboard `ar0..ar2`, ~4 ns/node, `dn0(4, 16)`).
#[inline]
pub fn score_fmrs_and_default(pos: &FmrsPos, list: &mut [MvItem]) {
    if list.len() <= 1 || list.iter().any(|x| x.tdata.dn == 0) || list.iter().all(|x| x.search()) {
        return;
    }
    let w = &crate::futsuu::default_weights::DEFAULT_LUT_WEIGHTS;
    let p = pos.pos_aux();
    let king_sq = pos.self_ou();
    let kz = (KING_ZONE[king_sq].min(5)) as usize;
    let atk_sq = pos.attack_sq();
    let cdist = (DISTANCE[king_sq][atk_sq].saturating_sub(1).min(6)) as usize;
    let dbl = usize::from(pos.n_oute() >= 2);
    let occ = p.occupied_bb();
    let empty_r1 = popcnt(KING_RING1[king_sq].and_not(occ));
    let is_gote = pos.turn().is_gote();
    let king_bit = BitBoard::from_u128(1u128 << SQ_TO_FMRS[king_sq].index());

    let n = list.len().min(64);
    let mut scores = [0i32; 64];
    let mut best_board_s = i32::MIN;
    let mut best_all_s = i32::MIN;

    for (k_i, item) in list[..n].iter().enumerate() {
        let mv = item.head_move();
        let dest = mv.dest();
        let is_drop = mv.is_drop();
        let (ev, mover, cc, nz, rd, nl) = if is_drop {
            let kt = ((mv.hand_ktype() - 1).min(6)) as usize;
            let nlib =
                (empty_r1.saturating_sub(u8::from(DISTANCE[king_sq][dest] == 1)).min(4)) as usize;
            (4usize, kt, 0usize, kz, 0usize, nlib)
        } else {
            let src = mv.prev_pos as usize;
            let c_cat = (cap_category(item.cap_koma).min(4)) as usize;
            if src == king_sq {
                let ev_k = if c_cat > 0 { 1usize } else { 2usize };
                let occ_a = occ.and_not(king_bit)
                    | BitBoard::from_u128(1u128 << SQ_TO_FMRS[dest].index());
                let nlib = (popcnt(KING_RING1[dest].and_not(occ_a)).min(4)) as usize;
                let dr = (dest / 9) as i32 - (king_sq / 9) as i32;
                let fwd = if is_gote { dr > 0 } else { dr < 0 };
                let bwd = if is_gote { dr < 0 } else { dr > 0 };
                let r_d = if fwd {
                    3usize
                } else if bwd {
                    1usize
                } else {
                    2usize
                };
                (ev_k, 13usize, c_cat, (KING_ZONE[dest].min(5)) as usize, r_d, nlib)
            } else {
                let ev_k = if c_cat > 0 { 0usize } else { 3usize };
                let fk = if mv.is_promote() {
                    item.src_koma + PROMOTED
                } else {
                    item.src_koma
                };
                (
                    ev_k,
                    (koma_to_idx14(fk).min(13)) as usize,
                    c_cat,
                    kz,
                    0usize,
                    (empty_r1.min(4)) as usize,
                )
            }
        };
        let s = unsafe {
            (*w.ar0.get_unchecked(((ev * 5 + cc) * 6 + nz) * 4 + rd) as i32)
                + (*w.ar1.get_unchecked((ev * 14 + mover) * 5 + nl) as i32)
                + (*w.ar2.get_unchecked((ev * 7 + cdist) * 2 + dbl) as i32)
        };
        scores[k_i] = s;
        if s > best_all_s {
            best_all_s = s;
        }
        if !is_drop && s > best_board_s {
            best_board_s = s;
        }
    }
    let has_board = best_board_s > i32::MIN;
    let best_s = if has_board { best_board_s } else { best_all_s };

    for (i, item) in list[..n].iter_mut().enumerate() {
        let gap = (best_s - scores[i]).max(0);
        item.ml_tie = 1 + ((gap >> 3).min(126) as u8);
        let extra = (((gap as u32) * 4) >> 8) as u16;
        let mut dn0 = 1 + extra.min(16);
        if has_board && item.head_move().is_drop() {
            dn0 = dn0.max(2);
        }
        if !item.search() {
            item.tdata.dn = item.tdata.dn.max(dn0);
        }
        item.sort_key = (item.sort_key & 0xffff) | ((dn0 as u64) << 16);
    }
}
