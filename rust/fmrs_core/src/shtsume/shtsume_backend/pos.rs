//! `ShtsumePos` — faithful Rust port of `sdata_t` and `base.c` from `shtsume`.

use anyhow::bail;

use crate::shtsume::shtsume_backend::bitboard::{lslp, rslp, SBitBoard, BB_EMPTY, TABLES};
use crate::shtsume::types::{
    is_sente_koma, sq_file, sq_rank, KeyState, MKey, Move, Turn, FU, GFU, GGI, GHI, GI, GKA, GKE, GKI, GKY, GNG, GNK, GNY, GOU, GRY, GTO, GUM, HAND, HI, KA, KE, KI, KY, N_SQUARE,
    PROMOTED, SFU, SGI, SHI, SKA, SKE, SKI, SKY, SNG, SNK, SNY, SOU, SPC, SRY, STO, SUM,
};

/// Exact Rust counterpart of `sdata_t` in `shtsume/source/shogi.h`.
#[derive(Clone)]
pub struct ShtsumePos {
    pub count: u16,
    pub turn: Turn,
    pub board: [u8; N_SQUARE],
    pub mkey: [MKey; 2],
    pub zkey: u64,
    pub fflag: u32,
    pub ou: [u8; 2],

    pub occupied: SBitBoard,
    pub voccupied: SBitBoard,
    pub roccupied: SBitBoard,
    pub loccupied: SBitBoard,
    pub occup: [SBitBoard; 2],

    pub bb_fu: [SBitBoard; 2],
    pub bb_ky: [SBitBoard; 2],
    pub bb_ke: [SBitBoard; 2],
    pub bb_gi: [SBitBoard; 2],
    pub bb_ki: [SBitBoard; 2],
    pub bb_ka: [SBitBoard; 2],
    pub bb_hi: [SBitBoard; 2],
    pub bb_to: [SBitBoard; 2],
    pub bb_ny: [SBitBoard; 2],
    pub bb_nk: [SBitBoard; 2],
    pub bb_ng: [SBitBoard; 2],
    pub bb_um: [SBitBoard; 2],
    pub bb_ry: [SBitBoard; 2],

    pub bb_tk: [SBitBoard; 2], // KI, TO, NY, NK, NG
    pub bb_uk: [SBitBoard; 2], // KA, UM
    pub bb_rh: [SBitBoard; 2], // HI, RY

    pub effect: [SBitBoard; 2],

    pub n_oute: u8,
    pub attack: [u8; 2],
    pub pinned: [u8; N_SQUARE],
}

impl Default for ShtsumePos {
    fn default() -> Self {
        Self {
            count: 0,
            turn: Turn::Sente,
            board: [SPC; N_SQUARE],
            mkey: [MKey::EMPTY; 2],
            zkey: 0,
            fflag: 0,
            ou: [HAND; 2],
            occupied: BB_EMPTY,
            voccupied: BB_EMPTY,
            roccupied: BB_EMPTY,
            loccupied: BB_EMPTY,
            occup: [BB_EMPTY; 2],
            bb_fu: [BB_EMPTY; 2],
            bb_ky: [BB_EMPTY; 2],
            bb_ke: [BB_EMPTY; 2],
            bb_gi: [BB_EMPTY; 2],
            bb_ki: [BB_EMPTY; 2],
            bb_ka: [BB_EMPTY; 2],
            bb_hi: [BB_EMPTY; 2],
            bb_to: [BB_EMPTY; 2],
            bb_ny: [BB_EMPTY; 2],
            bb_nk: [BB_EMPTY; 2],
            bb_ng: [BB_EMPTY; 2],
            bb_um: [BB_EMPTY; 2],
            bb_ry: [BB_EMPTY; 2],
            bb_tk: [BB_EMPTY; 2],
            bb_uk: [BB_EMPTY; 2],
            bb_rh: [BB_EMPTY; 2],
            effect: [BB_EMPTY; 2],
            n_oute: 0,
            attack: [0; 2],
            pinned: [0; N_SQUARE],
        }
    }
}

impl ShtsumePos {
    pub fn from_sfen(sfen: &str) -> anyhow::Result<Self> {
        let mut parts = sfen.split_whitespace();
        let board_str = parts.next().ok_or_else(|| anyhow::anyhow!("missing board"))?;
        let turn_str = parts.next().ok_or_else(|| anyhow::anyhow!("missing turn"))?;
        let hand_str = parts.next().ok_or_else(|| anyhow::anyhow!("missing hands"))?;

        let mut board = [SPC; N_SQUARE];
        let ranks: Vec<&str> = board_str.split('/').collect();
        if ranks.len() != 9 {
            bail!("expected 9 ranks in SFEN, got {}", ranks.len());
        }
        for (rank, row_str) in ranks.iter().enumerate() {
            // SFEN lists files from 9 down to 1 (file index 8 down to 0)
            let mut file_rev = 0usize;
            let bytes = row_str.as_bytes();
            let mut i = 0;
            while i < bytes.len() {
                let b = bytes[i];
                if b.is_ascii_digit() {
                    file_rev += (b - b'0') as usize;
                    i += 1;
                } else {
                    let promoted = if b == b'+' {
                        i += 1;
                        if i >= bytes.len() {
                            bail!("dangling '+' in SFEN");
                        }
                        true
                    } else {
                        false
                    };
                    let ch = bytes[i];
                    i += 1;
                    let koma = match (ch, promoted) {
                        (b'P', false) => SFU,
                        (b'L', false) => SKY,
                        (b'N', false) => SKE,
                        (b'S', false) => SGI,
                        (b'G', false) => SKI,
                        (b'B', false) => SKA,
                        (b'R', false) => SHI,
                        (b'K', false) => SOU,
                        (b'P', true) => STO,
                        (b'L', true) => SNY,
                        (b'N', true) => SNK,
                        (b'S', true) => SNG,
                        (b'B', true) => SUM,
                        (b'R', true) => SRY,
                        (b'p', false) => GFU,
                        (b'l', false) => GKY,
                        (b'n', false) => GKE,
                        (b's', false) => GGI,
                        (b'g', false) => GKI,
                        (b'b', false) => GKA,
                        (b'r', false) => GHI,
                        (b'k', false) => GOU,
                        (b'p', true) => GTO,
                        (b'l', true) => GNY,
                        (b'n', true) => GNK,
                        (b's', true) => GNG,
                        (b'b', true) => GUM,
                        (b'r', true) => GRY,
                        _ => bail!("invalid piece char '{}' in SFEN", ch as char),
                    };
                    if file_rev >= 9 {
                        bail!("rank {} overflow in SFEN", rank);
                    }
                    let file = 8 - file_rev;
                    board[rank * 9 + file] = koma;
                    file_rev += 1;
                }
            }
            if file_rev != 9 {
                bail!("rank {} has {} squares in SFEN", rank, file_rev);
            }
        }

        let turn = match turn_str {
            "b" => Turn::Sente,
            "w" => Turn::Gote,
            _ => bail!("invalid turn '{}'", turn_str),
        };

        let mut mkey = [MKey::EMPTY; 2];
        if hand_str != "-" {
            let mut n: u8 = 0;
            for b in hand_str.bytes() {
                if b.is_ascii_digit() {
                    n = n * 10 + (b - b'0');
                } else {
                    let cnt = if n == 0 { 1 } else { n };
                    n = 0;
                    let (side, ktype) = match b {
                        b'P' => (0, FU),
                        b'L' => (0, KY),
                        b'N' => (0, KE),
                        b'S' => (0, GI),
                        b'G' => (0, KI),
                        b'B' => (0, KA),
                        b'R' => (0, HI),
                        b'p' => (1, FU),
                        b'l' => (1, KY),
                        b'n' => (1, KE),
                        b's' => (1, GI),
                        b'g' => (1, KI),
                        b'b' => (1, KA),
                        b'r' => (1, HI),
                        _ => bail!("invalid hand char '{}' in SFEN", b as char),
                    };
                    for _ in 0..cnt {
                        mkey[side].inc(ktype);
                    }
                }
            }
        }

        let mut pos = Self {
            board,
            turn,
            mkey,
            ..Default::default()
        };
        pos.initialize();
        Ok(pos)
    }

    pub fn initialize(&mut self) {
        let t = &*TABLES;
        self.zkey = 0;
        self.fflag = 0;
        self.ou = [HAND; 2];
        self.occupied = BB_EMPTY;
        self.voccupied = BB_EMPTY;
        self.roccupied = BB_EMPTY;
        self.loccupied = BB_EMPTY;
        self.occup = [BB_EMPTY; 2];
        self.bb_fu = [BB_EMPTY; 2];
        self.bb_ky = [BB_EMPTY; 2];
        self.bb_ke = [BB_EMPTY; 2];
        self.bb_gi = [BB_EMPTY; 2];
        self.bb_ki = [BB_EMPTY; 2];
        self.bb_ka = [BB_EMPTY; 2];
        self.bb_hi = [BB_EMPTY; 2];
        self.bb_to = [BB_EMPTY; 2];
        self.bb_ny = [BB_EMPTY; 2];
        self.bb_nk = [BB_EMPTY; 2];
        self.bb_ng = [BB_EMPTY; 2];
        self.bb_um = [BB_EMPTY; 2];
        self.bb_ry = [BB_EMPTY; 2];
        self.bb_tk = [BB_EMPTY; 2];
        self.bb_uk = [BB_EMPTY; 2];
        self.bb_rh = [BB_EMPTY; 2];

        for sq in 0..N_SQUARE {
            let koma = self.board[sq];
            if koma == SPC {
                continue;
            }
            self.zkey ^= t.zkey_seed[koma as usize * N_SQUARE + sq];
            if koma == SFU {
                self.fflag |= 1 << sq_file(sq);
            } else if koma == GFU {
                self.fflag |= 1 << (sq_file(sq) + 9);
            }
            self.xor_occ(sq);
            if is_sente_koma(koma) {
                self.occup[0].xor_assign(t.bpos[sq]);
            } else {
                self.occup[1].xor_assign(t.bpos[sq]);
            }
            self.xor_piece_bb(koma, sq);
        }
        if self.turn.is_gote() {
            self.zkey ^= t.zkey_turn;
        }
        self.create_effect();
        self.oute_check();
        self.create_pin();
    }

    #[inline(always)]
    pub fn xor_occ(&mut self, sq: usize) {
        let t = &*TABLES;
        self.occupied.xor_assign(t.bpos[sq]);
        self.voccupied.xor_assign(t.vpos[sq]);
        self.roccupied.xor_assign(t.rpos[sq]);
        self.loccupied.xor_assign(t.lpos[sq]);
    }

    #[inline(always)]
    pub fn xor_piece_bb(&mut self, koma: u8, sq: usize) {
        let bp = TABLES.bpos[sq];
        match koma {
            SFU => self.bb_fu[0].xor_assign(bp),
            SKY => self.bb_ky[0].xor_assign(bp),
            SKE => self.bb_ke[0].xor_assign(bp),
            SGI => self.bb_gi[0].xor_assign(bp),
            SKI => {
                self.bb_ki[0].xor_assign(bp);
                self.bb_tk[0].xor_assign(bp);
            }
            SKA => {
                self.bb_ka[0].xor_assign(bp);
                self.bb_uk[0].xor_assign(bp);
            }
            SHI => {
                self.bb_hi[0].xor_assign(bp);
                self.bb_rh[0].xor_assign(bp);
            }
            SOU => self.ou[0] = sq as u8,
            STO => {
                self.bb_to[0].xor_assign(bp);
                self.bb_tk[0].xor_assign(bp);
            }
            SNY => {
                self.bb_ny[0].xor_assign(bp);
                self.bb_tk[0].xor_assign(bp);
            }
            SNK => {
                self.bb_nk[0].xor_assign(bp);
                self.bb_tk[0].xor_assign(bp);
            }
            SNG => {
                self.bb_ng[0].xor_assign(bp);
                self.bb_tk[0].xor_assign(bp);
            }
            SUM => {
                self.bb_um[0].xor_assign(bp);
                self.bb_uk[0].xor_assign(bp);
            }
            SRY => {
                self.bb_ry[0].xor_assign(bp);
                self.bb_rh[0].xor_assign(bp);
            }
            GFU => self.bb_fu[1].xor_assign(bp),
            GKY => self.bb_ky[1].xor_assign(bp),
            GKE => self.bb_ke[1].xor_assign(bp),
            GGI => self.bb_gi[1].xor_assign(bp),
            GKI => {
                self.bb_ki[1].xor_assign(bp);
                self.bb_tk[1].xor_assign(bp);
            }
            GKA => {
                self.bb_ka[1].xor_assign(bp);
                self.bb_uk[1].xor_assign(bp);
            }
            GHI => {
                self.bb_hi[1].xor_assign(bp);
                self.bb_rh[1].xor_assign(bp);
            }
            GOU => self.ou[1] = sq as u8,
            GTO => {
                self.bb_to[1].xor_assign(bp);
                self.bb_tk[1].xor_assign(bp);
            }
            GNY => {
                self.bb_ny[1].xor_assign(bp);
                self.bb_tk[1].xor_assign(bp);
            }
            GNK => {
                self.bb_nk[1].xor_assign(bp);
                self.bb_tk[1].xor_assign(bp);
            }
            GNG => {
                self.bb_ng[1].xor_assign(bp);
                self.bb_tk[1].xor_assign(bp);
            }
            GUM => {
                self.bb_um[1].xor_assign(bp);
                self.bb_uk[1].xor_assign(bp);
            }
            GRY => {
                self.bb_ry[1].xor_assign(bp);
                self.bb_rh[1].xor_assign(bp);
            }
            _ => {}
        }
    }

    #[inline(always)]
    pub fn effect_of(&self, sq: usize, koma: u8) -> SBitBoard {
        TABLES.effect_tbl(
            sq,
            koma,
            self.occupied,
            self.voccupied,
            self.roccupied,
            self.loccupied,
        )
    }

    pub fn create_effect(&mut self) {
        let t = &*TABLES;
        for side in 0..2 {
            let mut eff = BB_EMPTY;
            // FU
            let mut bb = self.bb_fu[side];
            while let Some(sq) = bb.pop_lsb() {
                eff.or_assign(if side == 0 {
                    t.eff_sfu[sq]
                } else {
                    t.eff_gfu[sq]
                });
            }
            // KY
            let mut bb = self.bb_ky[side];
            while let Some(sq) = bb.pop_lsb() {
                eff.or_assign(if side == 0 {
                    t.effect_sky(sq, self.voccupied)
                } else {
                    t.effect_gky(sq, self.voccupied)
                });
            }
            // KE
            let mut bb = self.bb_ke[side];
            while let Some(sq) = bb.pop_lsb() {
                eff.or_assign(if side == 0 {
                    t.eff_ske[sq]
                } else {
                    t.eff_gke[sq]
                });
            }
            // GI
            let mut bb = self.bb_gi[side];
            while let Some(sq) = bb.pop_lsb() {
                eff.or_assign(if side == 0 {
                    t.eff_sgi[sq]
                } else {
                    t.eff_ggi[sq]
                });
            }
            // TK (KI, TO, NY, NK, NG)
            let mut bb = self.bb_tk[side];
            while let Some(sq) = bb.pop_lsb() {
                eff.or_assign(if side == 0 {
                    t.eff_ski[sq]
                } else {
                    t.eff_gki[sq]
                });
            }
            // KA
            let mut bb = self.bb_ka[side];
            while let Some(sq) = bb.pop_lsb() {
                eff.or_assign(t.effect_ka(sq, self.roccupied, self.loccupied));
            }
            // HI
            let mut bb = self.bb_hi[side];
            while let Some(sq) = bb.pop_lsb() {
                eff.or_assign(t.effect_hi(sq, self.occupied, self.voccupied));
            }
            // UM
            let mut bb = self.bb_um[side];
            while let Some(sq) = bb.pop_lsb() {
                eff.or_assign(t.effect_ka(sq, self.roccupied, self.loccupied));
                eff.or_assign(t.eff_ou[sq]);
            }
            // RY
            let mut bb = self.bb_ry[side];
            while let Some(sq) = bb.pop_lsb() {
                eff.or_assign(t.effect_hi(sq, self.occupied, self.voccupied));
                eff.or_assign(t.eff_ou[sq]);
            }
            // OU
            let ou = self.ou[side] as usize;
            if ou < N_SQUARE {
                eff.or_assign(t.eff_ou[ou]);
            }
            self.effect[side] = eff;
        }
    }

    pub fn oute_check(&mut self) {
        self.n_oute = 0;
        self.attack = [0, 0];
        let side = self.turn.index();
        let opp = 1 - side;
        let ou = self.ou[side] as usize;
        if ou >= N_SQUARE || !self.effect[opp].test(ou) {
            return;
        }
        let t = &*TABLES;
        let mut attackers = BB_EMPTY;
        if side == 0 {
            attackers.or_assign(t.eff_sfu[ou].and(self.bb_fu[1]));
            attackers.or_assign(t.effect_sky(ou, self.voccupied).and(self.bb_ky[1]));
            attackers.or_assign(t.eff_ske[ou].and(self.bb_ke[1]));
            attackers.or_assign(t.eff_sgi[ou].and(self.bb_gi[1]));
            attackers.or_assign(t.eff_ski[ou].and(self.bb_tk[1]));
            attackers.or_assign(t.effect_ka(ou, self.roccupied, self.loccupied).and(self.bb_uk[1]));
            attackers.or_assign(t.effect_hi(ou, self.occupied, self.voccupied).and(self.bb_rh[1]));
            attackers.or_assign(t.eff_ou[ou].and(self.bb_um[1].or(self.bb_ry[1])));
        } else {
            attackers.or_assign(t.eff_gfu[ou].and(self.bb_fu[0]));
            attackers.or_assign(t.effect_gky(ou, self.voccupied).and(self.bb_ky[0]));
            attackers.or_assign(t.eff_gke[ou].and(self.bb_ke[0]));
            attackers.or_assign(t.eff_ggi[ou].and(self.bb_gi[0]));
            attackers.or_assign(t.eff_gki[ou].and(self.bb_tk[0]));
            attackers.or_assign(t.effect_ka(ou, self.roccupied, self.loccupied).and(self.bb_uk[0]));
            attackers.or_assign(t.effect_hi(ou, self.occupied, self.voccupied).and(self.bb_rh[0]));
            attackers.or_assign(t.eff_ou[ou].and(self.bb_um[0].or(self.bb_ry[0])));
        }
        while let Some(sq) = attackers.pop_lsb() {
            if (self.n_oute as usize) < 2 {
                self.attack[self.n_oute as usize] = sq as u8;
                self.n_oute += 1;
            }
        }
    }

    pub fn create_pin(&mut self) {
        self.pinned = [0; N_SQUARE];
        let side = self.turn.index();
        let opp = 1 - side;
        let ou = self.ou[side] as usize;
        if ou >= N_SQUARE {
            return;
        }
        let mut pins = [0u8; N_SQUARE];
        self.compute_pin_for_king(ou, opp, side, &mut pins);
        self.pinned = pins;
    }

    /// Compute pin line IDs for pieces of `pinned_side` pinned against `king_sq` by sliders of `slider_side`.
    pub fn compute_pin_for_king(
        &self,
        king_sq: usize,
        slider_side: usize,
        pinned_side: usize,
        out: &mut [u8; N_SQUARE],
    ) {
        let t = &*TABLES;
        let kf = sq_file(king_sq);
        let kr = sq_rank(king_sq);
        let k_rs = rslp(king_sq);
        let k_ls = lslp(king_sq);
        let own_occ = self.occup[pinned_side];

        // Lance
        let mut bb = self.bb_ky[slider_side];
        while let Some(src) = bb.pop_lsb() {
            if sq_file(src) == kf {
                let k_eff = if slider_side == 0 {
                    t.effect_sky(src, self.voccupied)
                } else {
                    t.effect_gky(src, self.voccupied)
                };
                let o_eff = if slider_side == 0 {
                    t.effect_gky(king_sq, self.voccupied)
                } else {
                    t.effect_sky(king_sq, self.voccupied)
                };
                if let Some(pin_sq) = k_eff.and(o_eff).and(own_occ).min_pos() {
                    out[pin_sq] = (kf + 10) as u8;
                }
            }
        }

        // Bishop / Horse
        let mut bb = self.bb_uk[slider_side];
        while let Some(src) = bb.pop_lsb() {
            let same_r = rslp(src) == k_rs;
            let same_l = lslp(src) == k_ls;
            if same_r || same_l {
                let k_eff = t.effect_ka(src, self.roccupied, self.loccupied);
                let o_eff = t.effect_ka(king_sq, self.roccupied, self.loccupied);
                if let Some(pin_sq) = k_eff.and(o_eff).and(own_occ).min_pos() {
                    out[pin_sq] = if same_r {
                        (k_rs + 36) as u8
                    } else {
                        (k_ls + 19) as u8
                    };
                }
            }
        }

        // Rook / Dragon
        let mut bb = self.bb_rh[slider_side];
        while let Some(src) = bb.pop_lsb() {
            let same_r = sq_rank(src) == kr;
            let same_f = sq_file(src) == kf;
            if same_r || same_f {
                let k_eff = t.effect_hi(src, self.occupied, self.voccupied);
                let o_eff = t.effect_hi(king_sq, self.occupied, self.voccupied);
                if let Some(pin_sq) = k_eff.and(o_eff).and(own_occ).min_pos() {
                    out[pin_sq] = if same_r {
                        (kr + 1) as u8
                    } else {
                        (kf + 10) as u8
                    };
                }
            }
        }
    }

    /// Compute discovered-check pins (own pieces blocking own sliders against enemy king).
    pub fn compute_discpin(&self) -> [u8; N_SQUARE] {
        let mut disc = [0u8; N_SQUARE];
        let side = self.turn.index();
        let opp = 1 - side;
        let enemy_ou = self.ou[opp] as usize;
        if enemy_ou >= N_SQUARE {
            return disc;
        }
        self.compute_pin_for_king(enemy_ou, side, side, &mut disc);
        disc
    }

    /// Safe king escape squares (exact counterpart of `evasion_bb` in `bitboard.c`).
    pub fn evasion_bb(&self) -> SBitBoard {
        let side = self.turn.index();
        let opp = 1 - side;
        let ou = self.ou[side] as usize;
        if ou >= N_SQUARE {
            return BB_EMPTY;
        }
        let t = &*TABLES;
        let mut cand = t.eff_ou[ou].and_not(self.occup[side]).and_not(self.effect[opp]);
        if cand.is_empty() {
            return BB_EMPTY;
        }
        // Temporarily remove king from occupancy so sliding attacks through the king are detected!
        let occ = self.occupied.xor(t.bpos[ou]);
        let vocc = self.voccupied.xor(t.vpos[ou]);
        let rocc = self.roccupied.xor(t.rpos[ou]);
        let locc = self.loccupied.xor(t.lpos[ou]);

        let mut safe = BB_EMPTY;
        while let Some(dest) = cand.pop_lsb() {
            // Check sliding attackers from `opp` through the removed king square
            let attacked = if opp == 0 {
                t.effect_gky(dest, vocc).and(self.bb_ky[0]).is_nonzero()
                    || t.effect_ka(dest, rocc, locc).and(self.bb_uk[0]).is_nonzero()
                    || t.effect_hi(dest, occ, vocc).and(self.bb_rh[0]).is_nonzero()
            } else {
                t.effect_sky(dest, vocc).and(self.bb_ky[1]).is_nonzero()
                    || t.effect_ka(dest, rocc, locc).and(self.bb_uk[1]).is_nonzero()
                    || t.effect_hi(dest, occ, vocc).and(self.bb_rh[1]).is_nonzero()
            };
            if !attacked {
                safe.set(dest);
            }
        }
        safe
    }

    /// Fast key-only forward transition (`sdata_key_forward`).
    pub fn key_forward(&self, mv: Move) -> KeyState {
        let t = &*TABLES;
        let side = self.turn.index();
        let mut zkey = self.zkey ^ t.zkey_turn;
        let mut mkey = self.mkey;
        let dest = mv.dest();

        if mv.is_drop() {
            let ktype = mv.hand_ktype();
            let koma = if side == 0 { ktype } else { ktype + 16 };
            zkey ^= t.zkey_seed[koma as usize * N_SQUARE + dest];
            mkey[side].dec(ktype);
        } else {
            let src = mv.prev_pos as usize;
            let koma = self.board[src];
            let new_koma = if mv.is_promote() {
                koma + PROMOTED
            } else {
                koma
            };
            zkey ^= t.zkey_seed[koma as usize * N_SQUARE + src];
            zkey ^= t.zkey_seed[new_koma as usize * N_SQUARE + dest];
            let cap = self.board[dest];
            if cap != SPC {
                zkey ^= t.zkey_seed[cap as usize * N_SQUARE + dest];
                mkey[side].inc(cap % PROMOTED);
            }
        }

        KeyState {
            zkey,
            mkey,
            turn: self.turn.flip(),
            count: self.count + 1,
        }
    }

    /// Full move execution (`sdata_move_forward`).
    pub fn move_forward(&self, mv: Move) -> Self {
        let mut next = self.clone();
        next.apply_move_in_place(mv);
        next
    }

    pub fn apply_move_in_place(&mut self, mv: Move) {
        let t = &*TABLES;
        let side = self.turn.index();
        let opp = 1 - side;
        let dest = mv.dest();

        if mv.is_drop() {
            let ktype = mv.hand_ktype();
            let koma = if side == 0 { ktype } else { ktype + 16 };
            self.board[dest] = koma;
            self.zkey ^= t.zkey_seed[koma as usize * N_SQUARE + dest];
            self.mkey[side].dec(ktype);
            if ktype == FU {
                self.fflag |= 1 << (sq_file(dest) + side * 9);
            }
            self.xor_occ(dest);
            self.occup[side].xor_assign(t.bpos[dest]);
            self.xor_piece_bb(koma, dest);
        } else {
            let src = mv.prev_pos as usize;
            let koma = self.board[src];
            let new_koma = if mv.is_promote() {
                koma + PROMOTED
            } else {
                koma
            };
            let cap = self.board[dest];

            self.board[src] = SPC;
            self.board[dest] = new_koma;

            self.zkey ^= t.zkey_seed[koma as usize * N_SQUARE + src];
            self.zkey ^= t.zkey_seed[new_koma as usize * N_SQUARE + dest];

            self.xor_occ(src);
            self.occup[side].xor_assign(t.bpos[src]);
            self.xor_piece_bb(koma, src);

            if cap != SPC {
                self.zkey ^= t.zkey_seed[cap as usize * N_SQUARE + dest];
                self.occup[opp].xor_assign(t.bpos[dest]);
                self.xor_piece_bb(cap, dest);
                if cap == SFU {
                    self.fflag &= !(1 << sq_file(dest));
                } else if cap == GFU {
                    self.fflag &= !(1 << (sq_file(dest) + 9));
                }
                self.mkey[side].inc(cap % PROMOTED);
            } else {
                self.xor_occ(dest);
            }

            self.occup[side].xor_assign(t.bpos[dest]);
            self.xor_piece_bb(new_koma, dest);

            if mv.is_promote() {
                if koma == SFU {
                    self.fflag &= !(1 << sq_file(src));
                } else if koma == GFU {
                    self.fflag &= !(1 << (sq_file(src) + 9));
                }
            }
        }

        self.turn = self.turn.flip();
        self.zkey ^= t.zkey_turn;
        self.count += 1;

        self.create_effect();
        self.oute_check();
        self.create_pin();
    }

    #[inline(always)]
    pub fn fu_drop_ok(&self, dest: usize) -> bool {
        let side = self.turn.index();
        let has_fu = (self.fflag & (1 << (sq_file(dest) + side * 9))) != 0;
        if has_fu {
            return false;
        }
        if side == 0 {
            dest > 8
        } else {
            dest < 72
        }
    }

    /// Returns true if a board piece of `self.turn` can legally capture on `dest` (ignoring king).
    pub fn has_non_king_capture_on(&self, dest: usize) -> bool {
        let side = self.turn.index();
        let t = &*TABLES;
        let mut attackers = if side == 0 {
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
        while let Some(src) = attackers.pop_lsb() {
            let pin = self.pinned[src] as usize;
            if pin == 0 || t.line_mask[pin].test(dest) {
                return true;
            }
        }
        false
    }

    /// Returns true if the pawn drop `mv` is LEGAL (i.e. NOT drop-pawn mate / 打ち歩詰め).
    /// Safe to call even when `self.mkey[side].fu() == 0` (e.g. from `drop_check`).
    pub fn fu_tsume_check(&self, mv: Move) -> bool {
        let mut child = self.clone();
        let side = child.turn.index();
        let dest = mv.dest();
        let koma = if side == 0 { SFU } else { GFU };
        let t = &*TABLES;
        child.board[dest] = koma;
        child.zkey ^= t.zkey_seed[koma as usize * N_SQUARE + dest] ^ t.zkey_turn;
        child.fflag |= 1 << (sq_file(dest) + side * 9);
        child.xor_occ(dest);
        child.occup[side].xor_assign(t.bpos[dest]);
        child.xor_piece_bb(koma, dest);
        child.turn = child.turn.flip();
        child.count += 1;
        child.create_effect();
        child.oute_check();
        child.create_pin();
        // If defender's king can escape or any defender piece can capture the dropped pawn, it's legal
        if child.evasion_bb().is_nonzero() {
            return true;
        }
        child.has_non_king_capture_on(dest)
    }

    /// Returns true if the current position (in check) is immediate checkmate (no king move,
    /// no capture of checking piece, and no possible interposition).
    pub fn is_immediate_mate(&self) -> bool {
        if self.n_oute == 0 {
            return false;
        }
        if self.evasion_bb().is_nonzero() {
            return false;
        }
        if self.n_oute > 1 {
            return true;
        }
        let atk = self.attack[0] as usize;
        if self.has_non_king_capture_on(atk) {
            return false;
        }
        // If sliding check, check if any square strictly between king and checker can be blocked
        let atk_koma = self.board[atk];
        if crate::shtsume::types::IS_SKOMA[atk_koma as usize] {
            return true;
        }
        let between = self.between_king_and_attacker();
        if between.is_empty() {
            return true;
        }
        // Any hand piece can drop on between?
        let side = self.turn.index();
        let hand = self.mkey[side];
        let mut bb = between;
        while let Some(dest) = bb.pop_lsb() {
            if hand.hi() > 0
                || hand.ka() > 0
                || hand.ki() > 0
                || hand.gi() > 0
                || (hand.ke() > 0 && if side == 0 { dest > 17 } else { dest < 63 })
                || (hand.ky() > 0 && if side == 0 { dest > 8 } else { dest < 72 })
                || (hand.fu() > 0 && self.fu_drop_ok(dest))
            {
                return false;
            }
            if self.has_non_king_capture_on(dest) {
                return false;
            }
        }
        true
    }

    /// Squares strictly between side-to-move's king and `attack[0]` (when `attack[0]` is a slider).
    pub fn between_king_and_attacker(&self) -> SBitBoard {
        let side = self.turn.index();
        let ou = self.ou[side] as usize;
        let atk = self.attack[0] as usize;
        let atk_koma = self.board[atk];
        let t = &*TABLES;
        let line = match atk_koma {
            SKY | GKY | SHI | GHI | SRY | GRY => {
                if sq_rank(ou) == sq_rank(atk) || sq_file(ou) == sq_file(atk) {
                    t.effect_hi(ou, self.occupied, self.voccupied)
                        .and(t.effect_hi(atk, self.occupied, self.voccupied))
                } else {
                    BB_EMPTY
                }
            }
            SKA | GKA | SUM | GUM => {
                if rslp(ou) == rslp(atk) || lslp(ou) == lslp(atk) {
                    t.effect_ka(ou, self.roccupied, self.loccupied)
                        .and(t.effect_ka(atk, self.roccupied, self.loccupied))
                } else {
                    BB_EMPTY
                }
            }
            _ => BB_EMPTY,
        };
        line.and_not(self.occupied)
    }
}
