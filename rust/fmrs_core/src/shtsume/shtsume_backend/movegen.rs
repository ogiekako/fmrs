//! Check & evasion move generation and `PositionBackend` impl for `ShtsumePos`
//! (ported from shtsume's `ncheck.c`, `nevasion.c`, and `ntbase.c`).

use smallvec::SmallVec;

use crate::shtsume::backend::{HsOracle, PositionBackend};
use crate::shtsume::shtsume_backend::bitboard::{SBitBoard, BB_EMPTY, TABLES};
use crate::shtsume::shtsume_backend::pos::ShtsumePos;
use crate::shtsume::types::{
    sq_rank, KeyState, MKey, Move, MoveChain, MvItem, Turn, DISTANCE, FU, GFU, GGI, GHI, GI, GKA,
    GKE, GKI, GKY, GRY, GUM, HI, IS_SKOMA, KA, KE, KI, KOMA_PROMOTION, KY, N_SQUARE, SFU, SGI, SHI,
    SKA, SKE, SKI, SKY, SRY, SUM,
};

#[inline(always)]
fn can_normal_move(koma: u8, dest: usize) -> bool {
    match koma {
        SFU | SKY => dest > 8,
        SKE => dest > 17,
        GFU | GKY => dest < 72,
        GKE => dest < 63,
        _ => true,
    }
}

#[inline(always)]
fn in_prom_zone(side: usize, sq: usize) -> bool {
    if side == 0 {
        sq < 27
    } else {
        sq >= 54
    }
}

impl ShtsumePos {
    /// Emit all legal non-king board moves of `side` to `dest` (respecting `pinned`).
    fn push_moves_to<const N: usize>(&self, dest: usize, out: &mut SmallVec<[Move; N]>) {
        let side = self.turn.index();
        let t = &*TABLES;
        let mut srcs = if side == 0 {
            t.eff_gfu[dest]
                .and(self.bb_fu[0])
                .or(t.effect_gky(dest, self.voccupied).and(self.bb_ky[0]))
                .or(t.eff_gke[dest].and(self.bb_ke[0]))
                .or(t.eff_ggi[dest].and(self.bb_gi[0]))
                .or(t.eff_gki[dest].and(self.bb_tk[0]))
                .or(t.effect_ka(dest, self.roccupied, self.loccupied).and(self.bb_uk[0]))
                .or(t.effect_hi(dest, self.occupied, self.voccupied).and(self.bb_rh[0]))
                .or(t.eff_ou[dest].and(self.bb_um[0].or(self.bb_ry[0])))
        } else {
            t.eff_sfu[dest]
                .and(self.bb_fu[1])
                .or(t.effect_sky(dest, self.voccupied).and(self.bb_ky[1]))
                .or(t.eff_ske[dest].and(self.bb_ke[1]))
                .or(t.eff_sgi[dest].and(self.bb_gi[1]))
                .or(t.eff_ski[dest].and(self.bb_tk[1]))
                .or(t.effect_ka(dest, self.roccupied, self.loccupied).and(self.bb_uk[1]))
                .or(t.effect_hi(dest, self.occupied, self.voccupied).and(self.bb_rh[1]))
                .or(t.eff_ou[dest].and(self.bb_um[1].or(self.bb_ry[1])))
        };

        while let Some(src) = srcs.pop_lsb() {
            let pin = self.pinned[src] as usize;
            if pin != 0 && !t.line_mask[pin].test(dest) {
                continue;
            }
            let koma = self.board[src];
            let can_prom =
                KOMA_PROMOTION[koma as usize] && (in_prom_zone(side, src) || in_prom_zone(side, dest));
            if can_prom {
                out.push(Move::promote(src as u8, dest as u8));
            }
            if can_normal_move(koma, dest) {
                out.push(Move::normal(src as u8, dest as u8));
            }
        }
    }

    /// Generate legal checking moves when own king is currently in check (`evasion_check`).
    fn generate_evasion_check(&self) -> Vec<MvItem> {
        let mut candidates: SmallVec<[Move; 32]> = SmallVec::new();
        let side = self.turn.index();
        let ou = self.ou[side] as usize;

        // 1. King moves
        let mut kb = self.evasion_bb();
        while let Some(dest) = kb.pop_lsb() {
            candidates.push(Move::normal(ou as u8, dest as u8));
        }

        if self.n_oute == 1 {
            let atk = self.attack[0] as usize;
            // 2. Captures of checking piece
            self.push_moves_to(atk, &mut candidates);

            // 3. Interpositions if sliding check
            if !IS_SKOMA[self.board[atk] as usize] {
                let mut between = self.between_king_and_attacker();
                let hand = self.mkey[side];
                while let Some(dest) = between.pop_lsb() {
                    self.push_moves_to(dest, &mut candidates);
                    for ktype in [FU, KY, KE, GI, KI, KA, HI] {
                        if hand.get(ktype) == 0 {
                            continue;
                        }
                        let ok = match ktype {
                            FU => self.fu_drop_ok(dest),
                            KY => {
                                if side == 0 {
                                    dest > 8
                                } else {
                                    dest < 72
                                }
                            }
                            KE => {
                                if side == 0 {
                                    dest > 17
                                } else {
                                    dest < 63
                                }
                            }
                            _ => true,
                        };
                        if ok {
                            candidates.push(Move::drop(ktype, dest as u8));
                        }
                    }
                }
            }
        }

        let mut out = Vec::new();
        for mv in candidates {
            let child = self.move_forward(mv);
            if child.n_oute > 0 {
                if mv.is_drop() && mv.hand_ktype() == FU && child.is_immediate_mate() {
                    continue;
                }
                let src_k = if mv.is_board_move() {
                    self.board[mv.prev_pos as usize]
                } else {
                    0
                };
                out.push(MvItem::single_with_hints(mv, src_k, self.board[mv.dest()]));
            }
        }
        out
    }

    /// Futile interposition check on `dest` (matches `invalid_drops` in `ntbase.c`).
    fn is_futile_drop_square<O: HsOracle>(&self, dest: usize, oracle: &mut O) -> bool {
        let def_side = self.turn.index();
        let atk_side = 1 - def_side;
        let t = &*TABLES;

        // Compute pins for attacker's king (if on board)
        let mut tpin = [0u8; N_SQUARE];
        let atk_ou = self.ou[atk_side] as usize;
        if atk_ou < N_SQUARE {
            self.compute_pin_for_king(atk_ou, def_side, atk_side, &mut tpin);
        }

        // All attacker pieces attacking `dest`
        let mut srcs = if atk_side == 0 {
            t.eff_gfu[dest]
                .and(self.bb_fu[0])
                .or(t.effect_gky(dest, self.voccupied).and(self.bb_ky[0]))
                .or(t.eff_gke[dest].and(self.bb_ke[0]))
                .or(t.eff_ggi[dest].and(self.bb_gi[0]))
                .or(t.eff_gki[dest].and(self.bb_tk[0]))
                .or(t.effect_ka(dest, self.roccupied, self.loccupied).and(self.bb_uk[0]))
                .or(t.effect_hi(dest, self.occupied, self.voccupied).and(self.bb_rh[0]))
                .or(t.eff_ou[dest].and(self.bb_um[0].or(self.bb_ry[0])))
        } else {
            t.eff_sfu[dest]
                .and(self.bb_fu[1])
                .or(t.effect_sky(dest, self.voccupied).and(self.bb_ky[1]))
                .or(t.eff_ske[dest].and(self.bb_ke[1]))
                .or(t.eff_sgi[dest].and(self.bb_gi[1]))
                .or(t.eff_ski[dest].and(self.bb_tk[1]))
                .or(t.effect_ka(dest, self.roccupied, self.loccupied).and(self.bb_uk[1]))
                .or(t.effect_hi(dest, self.occupied, self.voccupied).and(self.bb_rh[1]))
                .or(t.eff_ou[dest].and(self.bb_um[1].or(self.bb_ry[1])))
        };

        let atk_turn = self.turn.flip();

        while let Some(src) = srcs.pop_lsb() {
            let pin = tpin[src] as usize;
            if pin != 0 && !t.line_mask[pin].test(dest) {
                continue;
            }
            let koma = self.board[src];
            let can_prom = KOMA_PROMOTION[koma as usize]
                && (in_prom_zone(atk_side, src) || in_prom_zone(atk_side, dest));
            let can_norm = can_normal_move(koma, dest);

            for &(do_try, prom) in &[(can_prom, true), (can_norm, false)] {
                if !do_try {
                    continue;
                }
                let mut test = self.clone();
                // Simulate attacker moving `src -> dest`
                test.turn = atk_turn;
                test.zkey ^= t.zkey_turn;
                let mv = if prom {
                    Move::promote(src as u8, dest as u8)
                } else {
                    Move::normal(src as u8, dest as u8)
                };
                test.apply_move_in_place(mv);
                test.count = self.count + 2;

                if test.n_oute == 0 {
                    continue;
                }
                if test.is_immediate_mate() || oracle.hs_lookup(&test, atk_turn) {
                    return true;
                }
            }
        }
        false
    }
}

impl PositionBackend for ShtsumePos {
    fn from_sfen(sfen: &str) -> anyhow::Result<Self> {
        ShtsumePos::from_sfen(sfen)
    }

    #[inline(always)]
    fn turn(&self) -> Turn {
        self.turn
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
    fn key_forward(&self, mv: Move) -> KeyState {
        ShtsumePos::key_forward(self, mv)
    }

    #[inline(always)]
    fn move_forward(&self, mv: Move) -> Self {
        ShtsumePos::move_forward(self, mv)
    }

    #[inline(always)]
    fn board_at(&self, sq: usize) -> u8 {
        self.board[sq]
    }

    #[inline(always)]
    fn enemy_ou(&self) -> usize {
        self.ou[self.turn.flip().index()] as usize
    }

    #[inline(always)]
    fn self_ou(&self) -> usize {
        self.ou[self.turn.index()] as usize
    }

    #[inline(always)]
    fn n_oute(&self) -> u8 {
        self.n_oute
    }

    #[inline(always)]
    fn attack_sq(&self) -> usize {
        self.attack[0] as usize
    }

    #[inline(always)]
    fn self_effect_contains(&self, sq: usize) -> bool {
        self.effect[self.turn.index()].test(sq)
    }

    fn symmetry_check(&self) -> bool {
        let ou = self.enemy_ou();
        if ou >= N_SQUARE || (ou % 9) != 4 {
            return false;
        }
        for rank in 0..9 {
            for file in 0..4 {
                if self.board[rank * 9 + file] != self.board[rank * 9 + (8 - file)] {
                    return false;
                }
            }
        }
        true
    }

    fn drop_check(&self, ktype: u8) -> bool {
        let side = self.turn.index();
        let opp = 1 - side;
        let ou = self.ou[opp] as usize;
        if ou >= N_SQUARE {
            return false;
        }
        let opp_koma = if opp == 0 { ktype } else { ktype + 16 };
        let mut bb = self.effect_of(ou, opp_koma).and_not(self.occupied);
        if ktype == FU {
            while let Some(dest) = bb.pop_lsb() {
                if self.fu_drop_ok(dest) && self.fu_tsume_check(Move::drop(FU, dest as u8)) {
                    return true;
                }
            }
            return false;
        }
        bb.is_nonzero()
    }

    fn generate_check_fast(&self, out: &mut Vec<MvItem>) {
        if self.n_oute > 0 {
            out.extend(self.generate_evasion_check());
            return;
        }

        let side = self.turn.index();
        let opp = 1 - side;
        let ou = self.ou[opp] as usize;
        if ou >= N_SQUARE {
            return;
        }

        let t = &*TABLES;
        let hand = self.mkey[side];
        out.reserve(64);

        // Quick no-attacker check (matches ncheck.c lines 81-85)
        if self.occup[side].is_empty()
            && hand.hi() == 0
            && hand.ka() == 0
            && hand.ke() == 0
            && hand.ky() == 0
        {
            return;
        }

        let disc = self.compute_discpin();

        // --- 1. Hand drops ---
        let (opp_fu, opp_ky, opp_ke, opp_gi, opp_ki, opp_ka, opp_hi) = if side == 0 {
            (GFU, GKY, GKE, GGI, GKI, GKA, GHI)
        } else {
            (SFU, SKY, SKE, SGI, SKI, SKA, SHI)
        };

        if hand.fu() > 0 {
            let mut bb = self.effect_of(ou, opp_fu).and_not(self.occupied);
            while let Some(dest) = bb.pop_lsb() {
                if self.fu_drop_ok(dest) {
                    let mv = Move::drop(FU, dest as u8);
                    if self.fu_tsume_check(mv) {
                        out.push(MvItem::single_with_hints(mv, 0, 0));
                    }
                }
            }
        }
        if hand.ky() > 0 {
            let mut bb = self.effect_of(ou, opp_ky).and_not(self.occupied);
            while let Some(dest) = bb.pop_lsb() {
                out.push(MvItem::single_with_hints(Move::drop(KY, dest as u8), 0, 0));
            }
        }
        if hand.ke() > 0 {
            let mut bb = self.effect_of(ou, opp_ke).and_not(self.occupied);
            while let Some(dest) = bb.pop_lsb() {
                out.push(MvItem::single_with_hints(Move::drop(KE, dest as u8), 0, 0));
            }
        }
        if hand.gi() > 0 {
            let mut bb = self.effect_of(ou, opp_gi).and_not(self.occupied);
            while let Some(dest) = bb.pop_lsb() {
                out.push(MvItem::single_with_hints(Move::drop(GI, dest as u8), 0, 0));
            }
        }
        if hand.ki() > 0 {
            let mut bb = self.effect_of(ou, opp_ki).and_not(self.occupied);
            while let Some(dest) = bb.pop_lsb() {
                out.push(MvItem::single_with_hints(Move::drop(KI, dest as u8), 0, 0));
            }
        }
        if hand.ka() > 0 {
            let mut bb = self.effect_of(ou, opp_ka).and_not(self.occupied);
            while let Some(dest) = bb.pop_lsb() {
                out.push(MvItem::single_with_hints(Move::drop(KA, dest as u8), 0, 0));
            }
        }
        if hand.hi() > 0 {
            let mut bb = self.effect_of(ou, opp_hi).and_not(self.occupied);
            while let Some(dest) = bb.pop_lsb() {
                out.push(MvItem::single_with_hints(Move::drop(HI, dest as u8), 0, 0));
            }
        }

        // --- 2. Board moves ---
        let own_occ = self.occup[side];
        let prom_zone = t.prom_zone[side];

        let ou_fu = self.effect_of(ou, opp_fu);
        let ou_ky = self.effect_of(ou, opp_ky);
        let ou_ke = self.effect_of(ou, opp_ke);
        let ou_gi = self.effect_of(ou, opp_gi);
        let ou_ki = self.effect_of(ou, opp_ki);
        let ou_ka = self.effect_of(ou, opp_ka);
        let ou_hi = self.effect_of(ou, opp_hi);
        let ou_um = ou_ka.or(t.eff_ou[ou]);
        let ou_ry = ou_hi.or(t.eff_ou[ou]);

        let mut emit_piece_group =
            |mut srcs: SBitBoard, koma: u8, direct_norm: SBitBoard, direct_prom: Option<SBitBoard>| {
                while let Some(src) = srcs.pop_lsb() {
                    let pin = self.pinned[src] as usize;
                    let mut move_bb = self.effect_of(src, koma).and_not(own_occ);
                    if pin != 0 {
                        move_bb.and_assign(t.line_mask[pin]);
                    }
                    if move_bb.is_empty() {
                        continue;
                    }
                    let dpin = disc[src] as usize;
                    let disc_bb = if dpin != 0 {
                        move_bb.and_not(t.line_mask[dpin])
                    } else {
                        BB_EMPTY
                    };

                    // Promotions
                    if let Some(dp) = direct_prom {
                        let prom_targets = if in_prom_zone(side, src) {
                            move_bb
                        } else {
                            move_bb.and(prom_zone)
                        };
                        let mut pbb = prom_targets.and(dp.or(disc_bb));
                        while let Some(dest) = pbb.pop_lsb() {
                            out.push(MvItem::single_with_hints(
                                Move::promote(src as u8, dest as u8),
                                self.board[src],
                                self.board[dest],
                            ));
                        }
                    }

                    // Non-promotions
                    let mut nbb = move_bb.and(direct_norm.or(disc_bb));
                    while let Some(dest) = nbb.pop_lsb() {
                        if can_normal_move(koma, dest) {
                            out.push(MvItem::single_with_hints(
                                Move::normal(src as u8, dest as u8),
                                self.board[src],
                                self.board[dest],
                            ));
                        }
                    }
                }
            };

        let (k_fu, k_ky, k_ke, k_gi, k_ki, k_ka, k_hi, k_um, k_ry) = if side == 0 {
            (SFU, SKY, SKE, SGI, SKI, SKA, SHI, SUM, SRY)
        } else {
            (GFU, GKY, GKE, GGI, GKI, GKA, GHI, GUM, GRY)
        };

        emit_piece_group(self.bb_fu[side], k_fu, ou_fu, Some(ou_ki));
        emit_piece_group(self.bb_ky[side], k_ky, ou_ky, Some(ou_ki));
        emit_piece_group(self.bb_ke[side], k_ke, ou_ke, Some(ou_ki));
        emit_piece_group(self.bb_gi[side], k_gi, ou_gi, Some(ou_ki));
        emit_piece_group(self.bb_tk[side], k_ki, ou_ki, None);
        emit_piece_group(self.bb_ka[side], k_ka, ou_ka, Some(ou_um));
        emit_piece_group(self.bb_hi[side], k_hi, ou_hi, Some(ou_ry));
        emit_piece_group(self.bb_um[side], k_um, ou_um, None);
        emit_piece_group(self.bb_ry[side], k_ry, ou_ry, None);

        // Own king moving to give discovered check
        let self_ou = self.ou[side] as usize;
        if self_ou < N_SQUARE && disc[self_ou] != 0 {
            let ou_koma = self.board[self_ou];
            let mut kbb = self
                .evasion_bb()
                .and_not(t.line_mask[disc[self_ou] as usize]);
            while let Some(dest) = kbb.pop_lsb() {
                out.push(MvItem::single_with_hints(
                    Move::normal(self_ou as u8, dest as u8),
                    ou_koma,
                    self.board[dest],
                ));
            }
        }

        // Handle rare case where a sliding piece moves along a ray and removes itself as a blocker
        // (already covered since `src` was a blocker only if `disc[src] != 0`, and moving off the line
        // uncovers the slider behind it). Also note: if a sliding piece itself is `src` and moves
        // to `dest`, `ou_ka`/`ou_hi` computed with `self.occupied` includes `src` itself as a blocker
        // along the ray from `ou` to `dest` ONLY if `src` lies strictly between `ou` and `dest`!
        // That can happen if e.g. a Rook/Bishop/Lance steps *away* from `ou` along the same ray,
        // which would mean it was ALREADY giving check (impossible in legal OR node).
        // Therefore `direct_norm`/`direct_prom` is exact!
        let _ = sq_rank(ou);
    }

    fn generate_evasion_with<O: HsOracle>(&self, oracle: &mut O, out: &mut Vec<MvItem>) -> (bool, bool) {
        let side = self.turn.index();
        let ou = self.ou[side] as usize;
        let base = out.len();
        out.reserve(32);

        // 1. King moves
        let ou_koma = self.board[ou];
        let mut kb = self.evasion_bb();
        while let Some(dest) = kb.pop_lsb() {
            out.push(MvItem::single_with_hints(
                Move::normal(ou as u8, dest as u8),
                ou_koma,
                self.board[dest],
            ));
        }

        // Double check: only king moves are legal
        if self.n_oute > 1 {
            return (false, false);
        }

        let atk = self.attack[0] as usize;
        let atk_koma = self.board[atk];
        // 2. Captures of the checking piece
        let mut cap_moves: SmallVec<[Move; 8]> = SmallVec::new();
        self.push_moves_to(atk, &mut cap_moves);
        for mv in cap_moves {
            out.push(MvItem::single_with_hints(
                mv,
                self.board[mv.prev_pos as usize],
                atk_koma,
            ));
        }

        // Non-sliding check: no interpositions possible
        if IS_SKOMA[atk_koma as usize] {
            return (false, false);
        }

        // 3. Interpositions along squares strictly between `ou` and `atk`
        let mut between_bb = self.between_king_and_attacker();
        if between_bb.is_empty() {
            return (false, false);
        }

        let mut between_sqs: SmallVec<[usize; 7]> = SmallVec::new();
        while let Some(sq) = between_bb.pop_lsb() {
            between_sqs.push(sq);
        }
        between_sqs.sort_by_key(|&sq| DISTANCE[ou][sq]);

        let has_king_or_cap = out.len() > base;
        let hand = self.mkey[side];
        let mut invalid_drops = false;
        let mut drop_chain: MoveChain = MoveChain::new();
        let mut inter_moves: SmallVec<[Move; 16]> = SmallVec::new();

        for &dest in &between_sqs {
            // Interposition drops
            let has_any_drop = hand.hi() > 0
                || hand.ka() > 0
                || hand.ki() > 0
                || hand.gi() > 0
                || (hand.ke() > 0 && if side == 0 { dest > 17 } else { dest < 63 })
                || (hand.ky() > 0 && if side == 0 { dest > 8 } else { dest < 72 })
                || (hand.fu() > 0 && self.fu_drop_ok(dest));

            if has_any_drop {
                let futile = !has_king_or_cap
                    && !self.effect[side].test(dest)
                    && self.is_futile_drop_square(dest, oracle);
                if futile {
                    invalid_drops = true;
                } else {
                    for ktype in [FU, KY, KE, GI, KI, KA, HI] {
                        if hand.get(ktype) == 0 {
                            continue;
                        }
                        let ok = match ktype {
                            FU => self.fu_drop_ok(dest),
                            KY => {
                                if side == 0 {
                                    dest > 8
                                } else {
                                    dest < 72
                                }
                            }
                            KE => {
                                if side == 0 {
                                    dest > 17
                                } else {
                                    dest < 63
                                }
                            }
                            _ => true,
                        };
                        if ok {
                            drop_chain.push(Move::drop(ktype, dest as u8));
                        }
                    }
                }
            }

            // Interposition moves
            self.push_moves_to(dest, &mut inter_moves);
        }

        if !drop_chain.is_empty() {
            out.push(MvItem::from_moves_with_hints(drop_chain, 0, 0));
        }

        if !inter_moves.is_empty() {
            if out.len() == base {
                // Pattern B: chain interposition moves sequentially
                let first_src = self.board[inter_moves[0].prev_pos as usize];
                out.push(MvItem::from_moves_with_hints(
                    inter_moves.into_iter().collect(),
                    first_src,
                    0,
                ));
            } else {
                // Pattern A: individual interposition moves
                for mv in inter_moves {
                    out.push(MvItem::single_with_hints(
                        mv,
                        self.board[mv.prev_pos as usize],
                        0,
                    ));
                }
            }
        }

        (invalid_drops, false)
    }

    fn generate_evasion_simple_fast(&self, out: &mut Vec<MvItem>) {
        let side = self.turn.index();
        let ou = self.ou[side] as usize;
        if ou >= N_SQUARE {
            return;
        }
        let base = out.len();
        out.reserve(32);

        let ou_koma = self.board[ou];
        let mut kb = self.evasion_bb();
        while let Some(dest) = kb.pop_lsb() {
            out.push(MvItem::single_with_hints(
                Move::normal(ou as u8, dest as u8),
                ou_koma,
                self.board[dest],
            ));
        }

        if self.n_oute > 1 {
            return;
        }

        let atk = self.attack[0] as usize;
        let atk_koma = self.board[atk];
        let mut cap_moves: SmallVec<[Move; 8]> = SmallVec::new();
        self.push_moves_to(atk, &mut cap_moves);
        for mv in cap_moves {
            out.push(MvItem::single_with_hints(
                mv,
                self.board[mv.prev_pos as usize],
                atk_koma,
            ));
        }

        if IS_SKOMA[atk_koma as usize] {
            return;
        }

        let mut between_bb = self.between_king_and_attacker();
        if between_bb.is_empty() {
            return;
        }

        let mut between_sqs: SmallVec<[usize; 7]> = SmallVec::new();
        while let Some(sq) = between_bb.pop_lsb() {
            between_sqs.push(sq);
        }
        between_sqs.sort_by_key(|&sq| DISTANCE[ou][sq]);

        let hand = self.mkey[side];
        let mut drop_chain: MoveChain = MoveChain::new();
        let mut inter_moves: SmallVec<[Move; 16]> = SmallVec::new();

        for &dest in &between_sqs {
            for ktype in [FU, KY, KE, GI, KI, KA, HI] {
                if hand.get(ktype) == 0 {
                    continue;
                }
                let ok = match ktype {
                    FU => self.fu_drop_ok(dest),
                    KY => {
                        if side == 0 {
                            dest > 8
                        } else {
                            dest < 72
                        }
                    }
                    KE => {
                        if side == 0 {
                            dest > 17
                        } else {
                            dest < 63
                        }
                    }
                    _ => true,
                };
                if ok {
                    drop_chain.push(Move::drop(ktype, dest as u8));
                }
            }
            self.push_moves_to(dest, &mut inter_moves);
        }

        if !drop_chain.is_empty() {
            out.push(MvItem::from_moves_with_hints(drop_chain, 0, 0));
        }

        if !inter_moves.is_empty() {
            if out.len() == base {
                let first_src = self.board[inter_moves[0].prev_pos as usize];
                out.push(MvItem::from_moves_with_hints(
                    inter_moves.into_iter().collect(),
                    first_src,
                    0,
                ));
            } else {
                for mv in inter_moves {
                    out.push(MvItem::single_with_hints(
                        mv,
                        self.board[mv.prev_pos as usize],
                        0,
                    ));
                }
            }
        }
    }
}
