//! Core types for the Rust port of shtsume (MIT License, Copyright (c) 2022-2025 棋人(Hkijin)).

use smallvec::SmallVec;

pub const N_SQUARE: usize = 81;
pub const N_RANK: usize = 9;
pub const N_FILE: usize = 9;
pub const N_KOMAS: usize = 32;
pub const PROMOTED: u8 = 8;
pub const HAND: u8 = 100;

pub const TSUME_MAX_DEPTH: u16 = 2000;
pub const INFINATE: u16 = i16::MAX as u16; // 32767
pub const PROOF_MAX: u16 = 256;
pub const DISPROOF_MAX: u16 = 2048;
pub const PRE_PROOF_MAX: u16 = 240;
pub const PRE_DISPROOF_MAX: u16 = 1024;
pub const MAKE_TREE_PN_PLUS: u16 = 1;
pub const LEAF_PN_DEFAULT: u16 = 4;
pub const MAKE_REPEAT_DEFAULT: u8 = 2;

pub const ALL_DEPTH: i16 = -1;
/// Cards per MB of `hash_mb` budget (~53 bytes/card total across 28B `MCardNode` + `ZFolderNode` + bucket).
pub const MCARDS_PER_MBYTE: usize = 16384;
pub const GC_DELETE_RATE: usize = 20;
pub const GC_TSUMI_RATE: usize = 50;
pub const GC_DELETE_OFFSET: u16 = 5;

// Piece types (KType)
pub const OU: u8 = 0;
pub const FU: u8 = 1;
pub const KY: u8 = 2;
pub const KE: u8 = 3;
pub const GI: u8 = 4;
pub const KI: u8 = 5;
pub const KA: u8 = 6;
pub const HI: u8 = 7;

// Koma constants (exact match with shogi.h)
pub const SPC: u8 = 0;
pub const SFU: u8 = 1;
pub const SKY: u8 = 2;
pub const SKE: u8 = 3;
pub const SGI: u8 = 4;
pub const SKI: u8 = 5;
pub const SKA: u8 = 6;
pub const SHI: u8 = 7;
pub const SOU: u8 = 8;
pub const STO: u8 = 9;
pub const SNY: u8 = 10;
pub const SNK: u8 = 11;
pub const SNG: u8 = 12;
pub const SUM: u8 = 14;
pub const SRY: u8 = 15;

pub const GFU: u8 = 17;
pub const GKY: u8 = 18;
pub const GKE: u8 = 19;
pub const GGI: u8 = 20;
pub const GKI: u8 = 21;
pub const GKA: u8 = 22;
pub const GHI: u8 = 23;
pub const GOU: u8 = 24;
pub const GTO: u8 = 25;
pub const GNY: u8 = 26;
pub const GNK: u8 = 27;
pub const GNG: u8 = 28;
pub const GUM: u8 = 30;
pub const GRY: u8 = 31;

/// True for non-sliding (step) pieces; false for KY/KA/HI/UM/RY (exact match with g_is_skoma in global.c).
pub const IS_SKOMA: [bool; N_KOMAS] = [
    false, // SPC
    true,  // SFU
    false, // SKY
    true,  // SKE
    true,  // SGI
    true,  // SKI
    false, // SKA
    false, // SHI
    true,  // SOU
    true,  // STO
    true,  // SNY
    true,  // SNK
    true,  // SNG
    false, // 13
    false, // SUM
    false, // SRY
    false, // 16
    true,  // GFU
    false, // GKY
    true,  // GKE
    true,  // GGI
    true,  // GKI
    false, // GKA
    false, // GHI
    true,  // GOU
    true,  // GTO
    true,  // GNY
    true,  // GNK
    true,  // GNG
    false, // 29
    false, // GUM
    false, // GRY
];

pub const KOMA_PROMOTION: [bool; N_KOMAS] = [
    false, // SPC
    true,  // SFU
    true,  // SKY
    true,  // GKE? SKE
    true,  // SGI
    false, // SKI
    true,  // SKA
    true,  // SHI
    false, // SOU
    false, false, false, false, false, false, false, false, // 9..16
    true,  // GFU
    true,  // GKY
    true,  // GKE
    true,  // GGI
    false, // GKI
    true,  // GKA
    true,  // GHI
    false, // GOU
    false, false, false, false, false, false, false, // 25..31
];

#[inline(always)]
pub const fn sq_file(sq: usize) -> usize {
    sq % 9
}

#[inline(always)]
pub const fn sq_rank(sq: usize) -> usize {
    sq / 9
}

/// Distance table matching `init_distance()` in shtsume's `base.c`.
pub static DISTANCE: [[u8; N_SQUARE]; N_SQUARE] = {
    let mut table = [[1u8; N_SQUARE]; N_SQUARE];
    let mut a = 0;
    while a < N_SQUARE {
        let af = a % 9;
        let ar = a / 9;
        let mut b = 0;
        while b < N_SQUARE {
            let bf = b % 9;
            let br = b / 9;
            let df = af.abs_diff(bf);
            let dr = ar.abs_diff(br);
            let far = (af == bf && dr > 1) || (ar == br && df > 1) || (df > 1 && dr > 1);
            if far {
                let d = if df > dr { df } else { dr };
                table[a][b] = if d > 7 { 7 } else { d as u8 };
            }
            b += 1;
        }
        a += 1;
    }
    table
};

#[inline(always)]
pub const fn is_sente_koma(k: u8) -> bool {
    k != 0 && k < 16
}

#[inline(always)]
pub const fn is_gote_koma(k: u8) -> bool {
    k > 16
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Turn {
    Sente = 0,
    Gote = 1,
}

impl Turn {
    #[inline(always)]
    pub const fn flip(self) -> Self {
        match self {
            Turn::Sente => Turn::Gote,
            Turn::Gote => Turn::Sente,
        }
    }

    #[inline(always)]
    pub const fn index(self) -> usize {
        self as usize
    }

    #[inline(always)]
    pub const fn is_gote(self) -> bool {
        matches!(self, Turn::Gote)
    }
}

/// 2-byte move representation identical to shtsume's `move_t`.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Move {
    pub prev_pos: u8,
    pub new_pos: u8,
}

impl Move {
    #[inline(always)]
    pub const fn normal(src: u8, dest: u8) -> Self {
        Self {
            prev_pos: src,
            new_pos: dest,
        }
    }

    #[inline(always)]
    pub const fn promote(src: u8, dest: u8) -> Self {
        Self {
            prev_pos: src,
            new_pos: dest | 0x80,
        }
    }

    #[inline(always)]
    pub const fn drop(ktype: u8, dest: u8) -> Self {
        Self {
            prev_pos: HAND + ktype,
            new_pos: dest,
        }
    }

    #[inline(always)]
    pub const fn is_drop(self) -> bool {
        self.prev_pos > HAND
    }

    #[inline(always)]
    pub const fn is_board_move(self) -> bool {
        (self.prev_pos as usize) < N_SQUARE
    }

    #[inline(always)]
    pub const fn hand_ktype(self) -> u8 {
        self.prev_pos - HAND
    }

    #[inline(always)]
    pub const fn dest(self) -> usize {
        (self.new_pos & 0x7f) as usize
    }

    #[inline(always)]
    pub const fn is_promote(self) -> bool {
        (self.new_pos & 0x80) != 0
    }

    /// Format as USI/SFEN move string (e.g. "2g2f", "G*5b", "3c3b+").
    pub fn to_sfen(self) -> String {
        let df = (self.dest() % 9) as u8;
        let dr = (self.dest() / 9) as u8;
        let d_file = (b'1' + df) as char;
        let d_rank = (b'a' + dr) as char;
        if self.is_drop() {
            let ch = match self.hand_ktype() {
                FU => 'P',
                KY => 'L',
                KE => 'N',
                GI => 'S',
                KI => 'G',
                KA => 'B',
                HI => 'R',
                _ => '?',
            };
            format!("{}*{}{}", ch, d_file, d_rank)
        } else {
            let sf = self.prev_pos % 9;
            let sr = self.prev_pos / 9;
            let s_file = (b'1' + sf) as char;
            let s_rank = (b'a' + sr) as char;
            if self.is_promote() {
                format!("{}{}{}{}+", s_file, s_rank, d_file, d_rank)
            } else {
                format!("{}{}{}{}", s_file, s_rank, d_file, d_rank)
            }
        }
    }
}

/// SWAR 64-bit packed hand counts: 8-bit lane per `ktype` (`FU=1..=HI=7` at byte `ktype`, byte 0 = 0).
///
/// Using bit 7 of each active byte (`0x8080_8080_8080_8000`) as a borrow guard bit enables:
/// - O(1) branchless dominance comparison (`compare`) in 3 integer ops
/// - O(1) branchless `inc` / `dec` / `get` without `match`
/// - O(1) `total()` via a single `wrapping_mul(0x0101_0101_0101_0101) >> 56`
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct MKey(pub u64);

const SWAR_GUARD: u64 = 0x8080_8080_8080_8000;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum MKeyCmp {
    Equal,
    Super,
    Infer,
    Other,
}

impl MKey {
    pub const EMPTY: Self = Self(0);

    #[inline(always)]
    pub const fn new(fu: u8, ky: u8, ke: u8, gi: u8, ki: u8, ka: u8, hi: u8) -> Self {
        Self(
            ((fu as u64) << 8)
                | ((ky as u64) << 16)
                | ((ke as u64) << 24)
                | ((gi as u64) << 32)
                | ((ki as u64) << 40)
                | ((ka as u64) << 48)
                | ((hi as u64) << 56),
        )
    }

    #[inline(always)]
    pub const fn fu(self) -> u8 {
        ((self.0 >> 8) & 0x7f) as u8
    }
    #[inline(always)]
    pub const fn ky(self) -> u8 {
        ((self.0 >> 16) & 0x7f) as u8
    }
    #[inline(always)]
    pub const fn ke(self) -> u8 {
        ((self.0 >> 24) & 0x7f) as u8
    }
    #[inline(always)]
    pub const fn gi(self) -> u8 {
        ((self.0 >> 32) & 0x7f) as u8
    }
    #[inline(always)]
    pub const fn ki(self) -> u8 {
        ((self.0 >> 40) & 0x7f) as u8
    }
    #[inline(always)]
    pub const fn ka(self) -> u8 {
        ((self.0 >> 48) & 0x7f) as u8
    }
    #[inline(always)]
    pub const fn hi(self) -> u8 {
        ((self.0 >> 56) & 0x7f) as u8
    }

    #[inline(always)]
    pub fn set_fu(&mut self, v: u8) {
        self.0 = (self.0 & !(0xff << 8)) | ((v as u64) << 8);
    }
    #[inline(always)]
    pub fn set_ky(&mut self, v: u8) {
        self.0 = (self.0 & !(0xff << 16)) | ((v as u64) << 16);
    }
    #[inline(always)]
    pub fn set_ke(&mut self, v: u8) {
        self.0 = (self.0 & !(0xff << 24)) | ((v as u64) << 24);
    }
    #[inline(always)]
    pub fn set_gi(&mut self, v: u8) {
        self.0 = (self.0 & !(0xff << 32)) | ((v as u64) << 32);
    }
    #[inline(always)]
    pub fn set_ki(&mut self, v: u8) {
        self.0 = (self.0 & !(0xff << 40)) | ((v as u64) << 40);
    }
    #[inline(always)]
    pub fn set_ka(&mut self, v: u8) {
        self.0 = (self.0 & !(0xff << 48)) | ((v as u64) << 48);
    }
    #[inline(always)]
    pub fn set_hi(&mut self, v: u8) {
        self.0 = (self.0 & !(0xff << 56)) | ((v as u64) << 56);
    }

    #[inline(always)]
    pub const fn get(self, ktype: u8) -> u8 {
        ((self.0 >> ((ktype as u32) * 8)) & 0x7f) as u8
    }

    #[inline(always)]
    pub fn inc(&mut self, ktype: u8) {
        debug_assert!((FU..=HI).contains(&ktype));
        self.0 += 1u64 << ((ktype as u32) * 8);
    }

    #[inline(always)]
    pub fn dec(&mut self, ktype: u8) {
        debug_assert!((FU..=HI).contains(&ktype));
        debug_assert!(self.get(ktype) > 0);
        self.0 -= 1u64 << ((ktype as u32) * 8);
    }

    #[inline(always)]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    #[inline(always)]
    pub const fn total(self) -> u8 {
        (self.0.wrapping_mul(0x0101_0101_0101_0101) >> 56) as u8
    }

    #[inline(always)]
    pub const fn ge_all(self, other: Self) -> bool {
        (((self.0 | SWAR_GUARD).wrapping_sub(other.0)) & SWAR_GUARD) == SWAR_GUARD
    }

    #[inline(always)]
    pub fn compare(self, other: Self) -> MKeyCmp {
        if self.0 == other.0 {
            return MKeyCmp::Equal;
        }
        if self.ge_all(other) {
            return MKeyCmp::Super;
        }
        if other.ge_all(self) {
            return MKeyCmp::Infer;
        }
        MKeyCmp::Other
    }

    #[inline(always)]
    pub fn max(self, other: Self) -> Self {
        Self::new(
            self.fu().max(other.fu()),
            self.ky().max(other.ky()),
            self.ke().max(other.ke()),
            self.gi().max(other.gi()),
            self.ki().max(other.ki()),
            self.ka().max(other.ka()),
            self.hi().max(other.hi()),
        )
    }

    #[inline(always)]
    pub fn min(self, other: Self) -> Self {
        Self::new(
            self.fu().min(other.fu()),
            self.ky().min(other.ky()),
            self.ke().min(other.ke()),
            self.gi().min(other.gi()),
            self.ki().min(other.ki()),
            self.ka().min(other.ka()),
            self.hi().min(other.hi()),
        )
    }
}

#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct TData {
    pub pn: u16,
    pub dn: u16,
    pub sh: u16,
}

impl TData {
    pub const INIT: Self = Self {
        pn: 1,
        dn: 1,
        sh: 0,
    };
    pub const TSUMI: Self = Self {
        pn: 0,
        dn: INFINATE,
        sh: 0,
    };
    pub const FUDUMI: Self = Self {
        pn: INFINATE,
        dn: 0,
        sh: 0,
    };
}

/// Lightweight key snapshot for TT lookup/update without cloning the full position.
#[derive(Copy, Clone, Debug)]
pub struct KeyState {
    pub zkey: u64,
    pub mkey: [MKey; 2],
    pub turn: Turn,
    pub count: u16,
}

impl KeyState {
    #[inline(always)]
    pub fn mkey_for(&self, tn: Turn) -> MKey {
        self.mkey[tn.index()]
    }
}

const FLAG_HINC: u8 = 1 << 0;
const FLAG_INC: u8 = 1 << 1;
const FLAG_SEARCH: u8 = 1 << 2;
const FLAG_CU: u8 = 1 << 3;
const FLAG_PR: u8 = 1 << 4;

/// Inline up to 8 moves (16 bytes = exact size of `(ptr, cap)` on 64-bit, so zero extra struct size!).
pub type MoveChain = SmallVec<[Move; 8]>;

/// Compact equivalent to `mvlist_t` + `mlist_t` chain in shtsume (40 bytes).
/// - `head` is stored inline as a 2-byte `Move` (zero indirection on >99.9% single-move items).
/// - `tail` is `None` (`0usize`) unless chained interpositions / GHI folding attached extra moves.
/// - `sort_key` caches the 64-bit branchless comparison key so `O(N log N)` sorting and
///   `reorder_first` compare a single `u64` per step.
#[derive(Clone, Debug, Default)]
pub struct MvItem {
    pub sort_key: u64,
    pub mkey: MKey,
    pub def_need: MKey,
    tail: Option<Box<MoveChain>>,
    pub tdata: TData,
    pub head: Move,
    flags: u8,
    pub length: u8,
    pub nouse: u8,
    pub nouse2: u8,
    pub src_koma: u8,
    pub cap_koma: u8,
    pub pn_delta: u8,
    pub ml_tie: u8,
}

impl MvItem {
    #[inline(always)]
    pub const fn single_with_hints(mv: Move, src_koma: u8, cap_koma: u8) -> Self {
        Self {
            sort_key: 0,
            mkey: MKey::EMPTY,
            def_need: MKey::EMPTY,
            tail: None,
            tdata: TData::INIT,
            head: mv,
            flags: 0,
            length: 0,
            nouse: 0,
            nouse2: 0,
            src_koma,
            cap_koma,
            pn_delta: 0,
            ml_tie: 0,
        }
    }

    #[inline]
    pub fn from_moves_with_hints(mut moves: MoveChain, src_koma: u8, cap_koma: u8) -> Self {
        let head = moves[0];
        let tail = if moves.len() > 1 {
            moves.remove(0);
            Some(Box::new(moves))
        } else {
            None
        };
        Self {
            sort_key: 0,
            mkey: MKey::EMPTY,
            def_need: MKey::EMPTY,
            tail,
            tdata: TData::INIT,
            head,
            flags: 0,
            length: 0,
            nouse: 0,
            nouse2: 0,
            src_koma,
            cap_koma,
            pn_delta: 0,
            ml_tie: 0,
        }
    }

    #[inline(always)]
    pub const fn empty_root() -> Self {
        Self {
            sort_key: 0,
            mkey: MKey::EMPTY,
            def_need: MKey::EMPTY,
            tail: None,
            tdata: TData::INIT,
            head: Move {
                prev_pos: 0,
                new_pos: 0,
            },
            flags: 0,
            length: 0,
            nouse: 0,
            nouse2: 0,
            src_koma: 0,
            cap_koma: 0,
            pn_delta: 0,
            ml_tie: 0,
        }
    }

    #[inline(always)]
    pub const fn head_move(&self) -> Move {
        self.head
    }

    #[inline(always)]
    pub const fn has_tail(&self) -> bool {
        self.tail.is_some()
    }

    #[inline]
    pub fn take_tail(&mut self) -> Option<MoveChain> {
        self.tail.take().map(|b| *b)
    }

    #[inline]
    pub fn fold_moves_from(&mut self, other: MvItem) {
        let dst = self.tail.get_or_insert_with(|| Box::new(MoveChain::new()));
        dst.push(other.head);
        if let Some(other_tail) = other.tail {
            dst.extend(*other_tail);
        }
    }

    pub fn all_moves(&self) -> MoveChain {
        let mut out = MoveChain::new();
        out.push(self.head);
        if let Some(ref t) = self.tail {
            out.extend_from_slice(t);
        }
        out
    }

    #[inline(always)]
    pub const fn hinc(&self) -> bool {
        (self.flags & FLAG_HINC) != 0
    }
    #[inline(always)]
    pub fn set_hinc(&mut self, v: bool) {
        if v {
            self.flags |= FLAG_HINC;
        } else {
            self.flags &= !FLAG_HINC;
        }
    }

    #[inline(always)]
    pub const fn inc(&self) -> bool {
        (self.flags & FLAG_INC) != 0
    }
    #[inline(always)]
    pub fn set_inc(&mut self, v: bool) {
        if v {
            self.flags |= FLAG_INC;
        } else {
            self.flags &= !FLAG_INC;
        }
    }

    #[inline(always)]
    pub const fn search(&self) -> bool {
        (self.flags & FLAG_SEARCH) != 0
    }
    #[inline(always)]
    pub fn set_search(&mut self, v: bool) {
        if v {
            self.flags |= FLAG_SEARCH;
        } else {
            self.flags &= !FLAG_SEARCH;
        }
    }

    #[inline(always)]
    pub const fn cu(&self) -> bool {
        (self.flags & FLAG_CU) != 0
    }
    #[inline(always)]
    pub fn set_cu(&mut self, v: bool) {
        if v {
            self.flags |= FLAG_CU;
        } else {
            self.flags &= !FLAG_CU;
        }
    }

    #[inline(always)]
    pub const fn pr(&self) -> bool {
        (self.flags & FLAG_PR) != 0
    }
    #[inline(always)]
    pub fn set_pr(&mut self, v: bool) {
        if v {
            self.flags |= FLAG_PR;
        } else {
            self.flags &= !FLAG_PR;
        }
    }

    #[inline(always)]
    pub const fn dn_delta(&self) -> u8 {
        self.flags >> 5
    }
    #[inline(always)]
    pub fn set_dn_delta(&mut self, d: u8) {
        self.flags = (self.flags & 0x1f) | ((d.min(7)) << 5);
    }
}

#[derive(Clone, Debug)]
pub struct SearchConfig {
    /// Hash table size in MB (default: 64 MB for fast tests/benchmarks; shtsume default is 256 MB)
    pub hash_mb: usize,
    /// Minimum pn threshold during make_tree (default: 4)
    pub mt_min_pn: u16,
    /// Number of make_tree passes (default: 2)
    pub n_make_tree: u8,
    /// Additional search level (default: 0)
    pub search_level: u8,
    /// Optional wall-clock time limit in seconds
    pub time_limit_secs: Option<f64>,
    /// Optional node limit
    pub node_limit: Option<u64>,
    /// Enable fast bitboard 1-ply mate detection at frontier OR nodes (default: false)
    pub use_1ply_mate: bool,
    /// Enable learned L1 lookup-table move ordering & `dn` Focus-Lock on supported backends (default: true)
    pub use_ml_ordering: bool,
    /// Optional 1+epsilon shift `k` (`+ (pn >> k)`) at OR nodes (`0` = disabled)
    pub eps_pn_shift: u8,
    /// Optional 1+epsilon shift `k` (`+ (dn >> k)`) at AND nodes (`0` = backend default)
    pub eps_dn_shift: u8,
    /// Simplified mate mode: skips futile-interposition (無駄合) filtering and
    /// unused-piece / non-uniqueness (駒余り・非限定) checks, finding any attacker mate
    /// while maximizing defender evasion length (攻方任意・受方最長).
    pub simple_mate: bool,
    /// Number of worker threads (`1` = single-threaded default; `0` = auto-detect all available
    /// CPU threads; `N >= 2` = parallel shared-TBase df-pn search with `N` threads).
    pub threads: usize,
    /// Enable exact 81-bit `u128` Non-Interference (`NiCache`) proof footprint reuse (`B1-pn0`).
    pub use_ni_cache: bool,
    /// Enable online periodic cycle detection + soft-corridor guidance (`B2`).
    pub use_cycle_skip: bool,
}

impl SearchConfig {
    #[inline]
    pub fn resolved_threads(&self) -> usize {
        if self.threads == 0 {
            std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(1)
        } else {
            self.threads
        }
    }
}

impl Default for SearchConfig {
    fn default() -> Self {
        Self {
            hash_mb: 64,
            mt_min_pn: LEAF_PN_DEFAULT,
            n_make_tree: MAKE_REPEAT_DEFAULT,
            search_level: 0,
            time_limit_secs: None,
            node_limit: None,
            use_1ply_mate: false,
            use_ml_ordering: true,
            eps_pn_shift: 0,
            eps_dn_shift: 0,
            simple_mate: false,
            threads: 1,
            use_ni_cache: true,
            use_cycle_skip: false,
        }
    }
}

#[derive(Clone, Debug)]
pub struct SearchResult {
    /// `Some(true)` = mate, `Some(false)` = no mate, `None` = unknown (suspended/timeout)
    pub mate: Option<bool>,
    pub mate_steps: u16,
    pub redundant: bool,
    pub pv: Vec<Move>,
    pub nodes: u64,
    pub elapsed_secs: f64,
    pub gc_num: u16,
    pub gc_max_level: u16,
    pub root_pn: u16,
    pub error: bool,
}

impl SearchResult {
    pub fn nps(&self) -> f64 {
        if self.elapsed_secs > 0.0 {
            self.nodes as f64 / self.elapsed_secs
        } else {
            0.0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_swar_mkey_ops() {
        let mut a = MKey::new(3, 1, 0, 2, 4, 1, 2);
        assert_eq!(a.total(), 13);
        assert_eq!(a.get(FU), 3);
        assert_eq!(a.get(KE), 0);
        assert_eq!(a.get(HI), 2);

        a.inc(KE);
        assert_eq!(a.get(KE), 1);
        assert_eq!(a.total(), 14);
        a.dec(FU);
        assert_eq!(a.get(FU), 2);
        assert_eq!(a.total(), 13);

        let b = MKey::new(2, 1, 1, 2, 4, 1, 2);
        assert_eq!(a.compare(b), MKeyCmp::Equal);

        let sub = MKey::new(1, 0, 1, 2, 3, 0, 2);
        assert_eq!(a.compare(sub), MKeyCmp::Super);
        assert_eq!(sub.compare(a), MKeyCmp::Infer);

        let incomp = MKey::new(5, 0, 0, 0, 0, 0, 0);
        assert_eq!(a.compare(incomp), MKeyCmp::Other);
        assert_eq!(incomp.compare(a), MKeyCmp::Other);

        let full = MKey::new(18, 4, 4, 4, 4, 2, 2);
        assert_eq!(full.total(), 38);
        assert_eq!(full.compare(MKey::EMPTY), MKeyCmp::Super);
        assert_eq!(MKey::EMPTY.compare(full), MKeyCmp::Infer);
    }
}
