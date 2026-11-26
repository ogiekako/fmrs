//! Exact 81-bit `u128` Non-Interference (非干渉) Proof Footprint Cache (`NiCache`).
//!
//! Architecture (SoA 8-way with carry-free SWAR tag filter + 63-square window header):
//! 1. `bucket_tags: Vec<u64>` (32–64 KB, L1/L2-resident): 8x8-bit carry-free SWAR tag filter
//!    rejecting empty/unmatched King-ring buckets in ~2 ns without constructing `BoardPlanes`.
//! 2. `headers: Vec<NiHeaderBucket>`: compact 48B/way hot headers storing `(att_min, def_max, sh,
//!    ring_hi)` plus a 7-column (63-square) `(win_mask, win_black, win_white)` bitboard window
//!    centered on the Defender King, rejecting >99.9% of same-King-cage misses in L1 cache without
//!    touching cold 96-byte `BoardPlanes`.
//! 3. `cold: Vec<NiColdBucket>`: full 81-bit `BoardPlanes` + `mask` verified with 100% exactness
//!    before returning any `pn == 0` proof.

use std::sync::atomic::{fence, AtomicBool, AtomicU32, AtomicU64, Ordering};

use crate::futsuu::types::{MKey, TData, FU, GI, HI, INFINATE, KA, KE, KI, KY};

/// Allocate a zero-initialized `Box<[T]>` via OS demand-zero pages (`std::alloc::alloc_zeroed`)
/// without faulting or dirtying pages on the allocating thread.
///
/// # Safety
/// Every all-zero byte pattern of length `size_of::<T>()` must be a valid value of `T`.
pub(crate) unsafe fn alloc_zeroed_slice<T>(len: usize) -> Box<[T]> {
    assert!(len > 0 && std::mem::size_of::<T>() > 0);
    let layout = std::alloc::Layout::array::<T>(len).expect("invalid slice layout");
    let ptr = unsafe { std::alloc::alloc_zeroed(layout) as *mut T };
    if ptr.is_null() {
        std::alloc::handle_alloc_error(layout);
    }
    unsafe { Box::from_raw(std::ptr::slice_from_raw_parts_mut(ptr, len)) }
}

pub const MAX_HAND: MKey = MKey::new(18, 4, 4, 4, 4, 2, 2);
pub const MIN_NI_INSERT_WORK: u64 = 8;
pub const MIN_NI_INSERT_SH: u16 = 3;

/// Exact 81-square bitmask (`bits 0..81`).
pub const NI_SQ_MASK: u128 = (1u128 << 81) - 1;
/// Requires White has 0 `Bishop` or `ProBishop` on the board (`bit 0`).
pub const REQ_NO_W_BISH: u16 = 1 << 0;
/// Requires White has 0 `Rook` or `ProRook` on the board (`bit 1`).
pub const REQ_NO_W_ROOK: u16 = 1 << 1;
/// Requires White has at most 1 `Bishop`/`ProBishop` on the board (`bit 11`).
pub const REQ_AT_MOST_1_W_BISH: u16 = 1 << 11;
/// Requires White has at most 1 `Rook`/`ProRook` on the board (`bit 12`).
pub const REQ_AT_MOST_1_W_ROOK: u16 = 1 << 12;
/// Requires White has at most 1 unpromoted `Lance` in every column (`bit 13`).
pub const REQ_AT_MOST_1_W_LANCE_PER_COL: u16 = 1 << 13;

const NI_REQ_SHIFT: u32 = 81;
const NI_PM_SHIFT: u32 = 96;
const NI_CAP_SHIFT: u32 = 112;
const NI_LANCE_COLS_MASK: u16 = 0x07fc;
const NI_CAP_MASK: u16 = 0x07ff;
const NI_SLIDER_MASK: u16 = 0x3fff;

/// Bit `2 + col` (`col = 0..9`): requires White has 0 unpromoted `Lance`s on board in column `col`.
#[inline(always)]
pub const fn req_no_w_lance_col(col: usize) -> u16 {
    1u16 << (2 + col)
}

/// Pack 81-bit `sq_mask`, 14-bit `req_absent` (`bits 81..95`), and 14-bit `node_pm` (`bits 96..110`)
/// into a single `u128` footprint mask.
#[inline(always)]
pub const fn pack_ni_mask(sq_mask: u128, req_absent: u16, node_pm: u16) -> u128 {
    pack_ni_mask_cap(sq_mask, req_absent, node_pm, 0)
}

/// Pack `sq_mask`, `req_absent`, `node_pm`, plus 11-bit `cap_unique` (`bits 112..123`) for an OR
/// move that captures the unique on-board White slider of that class at `dest` (`dest` in `sq_mask`).
#[inline(always)]
pub const fn pack_ni_mask_cap(
    sq_mask: u128,
    req_absent: u16,
    node_pm: u16,
    cap_unique: u16,
) -> u128 {
    (sq_mask & NI_SQ_MASK)
        | (((req_absent & NI_SLIDER_MASK) as u128) << NI_REQ_SHIFT)
        | (((node_pm & NI_SLIDER_MASK) as u128) << NI_PM_SHIFT)
        | (((cap_unique & NI_CAP_MASK) as u128) << NI_CAP_SHIFT)
}

/// Upgrade child `REQ_AT_MOST_1_W_*` constraints when propagating upward across a White slider drop
/// (`B*sq` / `R*sq` / `L*sq`), since the drop increases on-board White slider count by +1.
#[inline(always)]
pub const fn upgrade_ni_mask_for_white_drop(
    child_mask: u128,
    hand_ktype: u8,
    dest_col: usize,
) -> u128 {
    let req = ((child_mask >> NI_REQ_SHIFT) as u16) & NI_SLIDER_MASK;
    let mut new_req = req;
    if hand_ktype == KA && (req & REQ_AT_MOST_1_W_BISH) != 0 {
        new_req = (new_req & !REQ_AT_MOST_1_W_BISH) | REQ_NO_W_BISH;
    } else if hand_ktype == HI && (req & REQ_AT_MOST_1_W_ROOK) != 0 {
        new_req = (new_req & !REQ_AT_MOST_1_W_ROOK) | REQ_NO_W_ROOK;
    } else if hand_ktype == KY && (req & REQ_AT_MOST_1_W_LANCE_PER_COL) != 0 {
        new_req |= req_no_w_lance_col(dest_col);
    }
    if new_req == req {
        child_mask
    } else {
        (child_mask & !((NI_SLIDER_MASK as u128) << NI_REQ_SHIFT))
            | ((new_req as u128) << NI_REQ_SHIFT)
    }
}

/// Resolve capture-transition slider constraints and verify `(req_absent & node_pm) == 0`.
#[inline(always)]
fn resolve_ni_mask(packed: u128) -> Option<(u128, u16)> {
    let sq_mask = packed & NI_SQ_MASK;
    let mut req_absent = ((packed >> NI_REQ_SHIFT) as u16) & NI_SLIDER_MASK;
    let node_pm = ((packed >> NI_PM_SHIFT) as u16) & NI_SLIDER_MASK;
    let cap_unique = ((packed >> NI_CAP_SHIFT) as u16) & NI_CAP_MASK;
    let captured_req = req_absent & cap_unique;
    if captured_req != 0 {
        if (captured_req & REQ_NO_W_BISH) != 0 {
            req_absent = (req_absent & !REQ_NO_W_BISH) | REQ_AT_MOST_1_W_BISH;
        }
        if (captured_req & REQ_NO_W_ROOK) != 0 {
            req_absent = (req_absent & !REQ_NO_W_ROOK) | REQ_AT_MOST_1_W_ROOK;
        }
        if (captured_req & NI_LANCE_COLS_MASK) != 0 {
            req_absent = (req_absent & !(captured_req & NI_LANCE_COLS_MASK))
                | REQ_AT_MOST_1_W_LANCE_PER_COL;
        }
    }
    if (req_absent & node_pm) != 0 {
        return None;
    }
    Some((sq_mask, req_absent))
}

/// Computation of the 14-bit White board slider presence bitmask:
/// - Bit 0: `w_bish != 0` (>= 1 White `Bishop` or `ProBishop`)
/// - Bit 1: `w_rook != 0` (>= 1 White `Rook` or `ProRook`)
/// - Bits `2..11`: >= 1 unpromoted White `Lance` in column `col = 0..9`
/// - Bit 11: >= 2 White `Bishop`/`ProBishop` on board
/// - Bit 12: >= 2 White `Rook`/`ProRook` on board
/// - Bit 13: >= 2 unpromoted White `Lance`s in the same column
#[inline(always)]
pub fn white_slider_mask_from_bbs(w_bish: u128, w_rook: u128, w_lance: u128) -> u16 {
    let mut pm = u16::from(w_bish != 0)
        | (u16::from(w_rook != 0) << 1)
        | (u16::from((w_bish & w_bish.wrapping_sub(1)) != 0) << 11)
        | (u16::from((w_rook & w_rook.wrapping_sub(1)) != 0) << 12);
    if w_lance != 0 {
        let mut rem = w_lance;
        let mut seen_cols = 0u16;
        while rem != 0 {
            let sq = rem.trailing_zeros();
            rem &= rem - 1;
            let col_bit = 1u16 << ((sq * 57) >> 9);
            if (seen_cols & col_bit) != 0 {
                pm |= REQ_AT_MOST_1_W_LANCE_PER_COL;
            }
            seen_cols |= col_bit;
        }
        pm |= seen_cols << 2;
    }
    pm
}

/// 6-bitplane representation of all 81 board squares in `fmrs` `Square::index()` bit order.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct BoardPlanes {
    pub black: u128,
    pub white: u128,
    pub prom: u128,
    pub k0: u128,
    pub k1: u128,
    pub k2: u128,
}

impl BoardPlanes {
    /// Exact 81-bit mask of all squares where `self` and `other` differ.
    #[inline(always)]
    pub fn diff(&self, other: &Self) -> u128 {
        (self.black ^ other.black)
            | (self.white ^ other.white)
            | (self.prom ^ other.prom)
            | (self.k0 ^ other.k0)
            | (self.k1 ^ other.k1)
            | (self.k2 ^ other.k2)
    }

    /// True iff `self` and `other` have identical pieces on every square in `mask`.
    /// Short-circuits in pairs so mismatches exit after `black`/`white`.
    #[inline(always)]
    pub fn eq_on_mask(&self, other: &Self, mask: u128) -> bool {
        (((self.black ^ other.black) | (self.white ^ other.white)) & mask) == 0
            && (((self.prom ^ other.prom) | (self.k0 ^ other.k0)) & mask) == 0
            && (((self.k1 ^ other.k1) | (self.k2 ^ other.k2)) & mask) == 0
    }

    /// Branchless, loop-free 64-bit hash of the board restricted to `ring_mask` plus `tag`.
    #[inline(always)]
    pub fn masked_hash(&self, ring_mask: u128, tag: u64) -> u64 {
        #[inline(always)]
        fn mix128(h: u64, x: u128) -> u64 {
            let lo = x as u64;
            let hi = (x >> 64) as u64;
            let h = (h ^ lo).wrapping_mul(0x9e37_79b9_7f4a_7c15);
            (h ^ hi).wrapping_mul(0xbf58_476d_1ce4_e5b9)
        }
        let mut h = tag.wrapping_mul(0x94d0_49bb_1331_11eb);
        h = mix128(h, self.black & ring_mask);
        h = mix128(h, self.white & ring_mask);
        h = mix128(h, self.prom & ring_mask);
        h = mix128(h, self.k0 & ring_mask);
        h = mix128(h, self.k1 & ring_mask);
        h = mix128(h, self.k2 & ring_mask);
        h ^ (h >> 31)
    }

    /// Exact 11-bit White board slider presence bitmask computed directly from `BoardPlanes`
    /// (`KindBitBoard` encoding: `bishopish = k1 & k2 & !k0`, `rookish = k0 & k1 & k2`,
    /// unpromoted `Lance = k1 & !(prom | k0 | k2)`).
    #[inline(always)]
    pub fn white_slider_present_mask(&self) -> u16 {
        let w_k12 = self.white & self.k1 & self.k2;
        let w_bish = w_k12 & !self.k0;
        let w_rook = w_k12 & self.k0;
        let w_lance = self.white & self.k1 & !(self.prom | self.k0 | self.k2);
        white_slider_mask_from_bbs(w_bish, w_rook, w_lance)
    }
}

impl NiFootprint {
    #[inline(always)]
    pub const fn sq_mask(&self) -> u128 {
        self.mask & NI_SQ_MASK
    }

    #[inline(always)]
    pub const fn req_absent(&self) -> u16 {
        ((self.mask >> NI_REQ_SHIFT) as u16) & NI_SLIDER_MASK
    }
}

/// Exact non-interference proof footprint returned alongside a proven (`pn == 0`) node.
#[derive(Copy, Clone, Debug, Default)]
pub struct NiFootprint {
    pub valid: bool,
    pub mask: u128,
    pub att_min: MKey,
    pub def_max: MKey,
    pub sh: u16,
    pub hinc: bool,
    pub nouse: u8,
}

#[inline(always)]
pub fn mkey_set(m: &mut MKey, ktype: u8, v: u8) {
    let shift = (ktype as u32) * 8;
    m.0 = (m.0 & !(0xffu64 << shift)) | ((v as u64) << shift);
}

/// Increment a bounded upper limit (`def_max`) by 1 when a defender drop consumes `ktype`.
#[inline(always)]
pub fn mkey_bound_inc(mut m: MKey, ktype: u8) -> MKey {
    let cur = m.get(ktype);
    let cap = MAX_HAND.get(ktype);
    if cur < cap {
        mkey_set(&mut m, ktype, (cur + 1).min(cap));
    }
    m
}

/// Decrement a bounded upper limit (`def_max`) by 1 when a defender capture gains `ktype`.
#[inline(always)]
pub fn mkey_bound_dec_if_bounded(mut m: MKey, ktype: u8) -> MKey {
    let cur = m.get(ktype);
    let cap = MAX_HAND.get(ktype);
    if cur < cap {
        mkey_set(&mut m, ktype, cur.saturating_sub(1));
    }
    m
}

/// Decrement a lower bound (`att_min`) by 1 saturating at 0 (when an attacker capture gains `ktype`).
#[inline(always)]
pub fn mkey_min_dec_sat(mut m: MKey, ktype: u8) -> MKey {
    let cur = m.get(ktype);
    if cur > 0 {
        mkey_set(&mut m, ktype, cur - 1);
    }
    m
}

/// Convert a 2-hand proof bound `(att_min, def_max)` into a 1-hand `TBase` `att_mkey`
/// valid on a board whose total hand pieces are `att_cur + def_cur`.
#[inline(always)]
pub fn proof_tbase_mkey(att_min: MKey, def_max: MKey, att_cur: MKey, def_cur: MKey) -> MKey {
    let mut out = att_min;
    for k in [FU, KY, KE, GI, KI, KA, HI] {
        let tot = att_cur.get(k).saturating_add(def_cur.get(k));
        let req_from_def = tot.saturating_sub(def_max.get(k));
        let v = out.get(k).max(req_from_def);
        mkey_set(&mut out, k, v);
    }
    out
}

/// Result of a successful `NiCache` lookup.
#[derive(Copy, Clone, Debug)]
pub struct NiHit {
    pub mask: u128,
    pub att_min: MKey,
    pub def_max: MKey,
    pub tdata: TData,
    pub hinc: bool,
    pub nouse: u8,
}

const WAYS: usize = 8;
const META_WAYS: usize = 4;
const SWAR_7F: u64 = 0x7f7f_7f7f_7f7f_7f7f;
const SWAR_01: u64 = 0x0101_0101_0101_0101;

#[inline(always)]
fn pack_mkeys_with_req(att_min: MKey, def_max: MKey, req_absent: u16) -> (u64, u64) {
    debug_assert_eq!(att_min.0 & 0xff, 0);
    debug_assert_eq!(def_max.0 & 0xff, 0);
    (
        (att_min.0 & !0xff) | ((req_absent & 0xff) as u64),
        (def_max.0 & !0xff) | (((req_absent >> 8) & 0xff) as u64),
    )
}

#[inline(always)]
fn unpack_mkeys_with_req(raw_att: u64, raw_def: u64) -> (MKey, MKey, u16) {
    let req = ((raw_att & 0xff) as u16) | (((raw_def & 0xff) as u16) << 8);
    (MKey(raw_att & !0xff), MKey(raw_def & !0xff), req)
}

#[inline(always)]
fn meta_set_idx(zkey: u64, set_mask: usize) -> usize {
    ((zkey ^ (zkey >> 27)) as usize) & set_mask
}

/// 48-byte hot per-way header kept separate from 128-byte cold `BoardPlanes`.
#[repr(C)]
#[derive(Copy, Clone, Default)]
struct NiWayHeader {
    win_mask: u64,    // (sq_mask >> win_shift) as u64
    win_black: u64,   // ((planes.black & sq_mask) >> win_shift) as u64
    win_white: u64,   // ((planes.white & sq_mask) >> win_shift) as u64
    att_min_req: u64, // att_min.0 | (req_absent & 0xff)
    def_max_req: u64, // def_max.0 | ((req_absent >> 8) & 0xff)
    ring_hi: u32,     // (ring_hash >> 32) as u32
    sh: u16,          // 2B
    work: u16,        // 2B (clamped subtree nodes saved)
}
const _: () = assert!(std::mem::size_of::<NiWayHeader>() == 48);

#[derive(Copy, Clone, Default)]
struct NiHeaderBucket {
    ways: [NiWayHeader; WAYS],
}

#[derive(Copy, Clone, Default)]
struct NiWayCold {
    planes: BoardPlanes, // 96B
    mask: u128,          // 16B (81-bit sq_mask)
    nouse: u8,           // 1B
    hinc: u8,            // 1B
}

#[derive(Copy, Clone, Default)]
struct NiColdBucket {
    ways: [NiWayCold; WAYS],
}

/// 4-way set-associative metadata slot keyed by `zkey` so AND/OR parent nodes can recover the
/// `NiFootprint` of children even when multiple `MKey`s or sibling evasions share a set.
#[derive(Copy, Clone, Default)]
struct MetaSlot {
    zkey: u64,
    sq_mask: u128,
    att_min: MKey,
    def_max: MKey,
    req_absent: u16,
    sh: u16,
    epoch: u16,
    nouse: u8,
    hinc: u8,
}

#[derive(Copy, Clone, Default)]
struct MetaSet {
    ways: [MetaSlot; META_WAYS],
    rr: u8,
    mru: u8,
}

#[derive(Copy, Clone, Debug, Default)]
pub struct NiStats {
    pub lookups: u64,
    pub tag_matches: u64,
    pub cold_checks: u64,
    pub pn0_hits: u64,
    pub inserts_pn0: u64,
    pub mask_bits_sum: u64,
    pub mask_bits_cnt: u64,
}

#[inline(always)]
fn ring_tag8(ring_hash: u64) -> u8 {
    let t = (ring_hash >> 32) as u8;
    if t == 0 { 1 } else { t }
}

#[inline(always)]
fn ring_win_shift(ring_hash: u64) -> u32 {
    (((ring_hash >> 28) & 3) as u32) * 9
}

/// Carry-free 8-lane exact byte equality: sets bit `7 + 8*w` iff byte `w` of `tags` == `tag8`.
/// Zero inter-byte borrow, so `tag8 == 1` never bleeds into empty (`0`) ways!
#[inline(always)]
fn swar_eq_bytes64(tags: u64, tag8: u8) -> u64 {
    let pat = (tag8 as u64).wrapping_mul(SWAR_01);
    let x = tags ^ pat;
    let y = (x & SWAR_7F).wrapping_add(SWAR_7F);
    !(y | x | SWAR_7F)
}

pub struct NiCache {
    /// L1/L2-resident 8x8-bit non-zero tag filter (`0` = empty way).
    bucket_tags: Vec<u64>,
    headers: Vec<NiHeaderBucket>,
    cold: Vec<NiColdBucket>,
    bucket_mask: usize,
    meta: Vec<MetaSet>,
    meta_mask: usize,
    meta_epoch: u16,
    pub stats: NiStats,
}

impl NiCache {
    /// Create a proof-only SoA `NiCache` with `num_buckets` (power of 2) 8-way buckets
    /// and `meta_entries` (power of 2) 4-way set-associative `zkey -> footprint` metadata slots.
    pub fn new(num_buckets: usize, meta_entries: usize) -> Self {
        let nb = num_buckets.next_power_of_two().max(64);
        let nm_sets = (meta_entries / META_WAYS).next_power_of_two().max(256);
        // SAFETY: u64, NiHeaderBucket, NiColdBucket, and MetaSet are POD-like structs
        // whose Default representation is all-zero bytes.
        unsafe {
            Self {
                bucket_tags: alloc_zeroed_slice::<u64>(nb).into_vec(),
                headers: alloc_zeroed_slice::<NiHeaderBucket>(nb).into_vec(),
                cold: alloc_zeroed_slice::<NiColdBucket>(nb).into_vec(),
                bucket_mask: nb - 1,
                meta: alloc_zeroed_slice::<MetaSet>(nm_sets).into_vec(),
                meta_mask: nm_sets - 1,
                meta_epoch: 1,
                stats: NiStats::default(),
            }
        }
    }

    /// O(1)-amortized clear: only zeros the compact `bucket_tags` array (32–64 KB)
    /// and bumps `meta_epoch`, avoiding multi-MB clears between short problems.
    pub fn clear(&mut self) {
        self.bucket_tags.fill(0);
        self.meta_epoch = self.meta_epoch.wrapping_add(1);
        if self.meta_epoch == 0 {
            self.meta_epoch = 1;
            for set in &mut self.meta {
                for m in &mut set.ways {
                    m.epoch = 0;
                }
            }
        }
        self.stats = NiStats::default();
    }

    #[inline(always)]
    pub fn record_meta(&mut self, zkey: u64, fp: &NiFootprint) {
        if !fp.valid {
            return;
        }
        let Some((sq_mask, req_absent)) = resolve_ni_mask(fp.mask) else {
            return;
        };
        let ep = self.meta_epoch;
        let set = &mut self.meta[meta_set_idx(zkey, self.meta_mask)];
        let new_hinc = u8::from(fp.hinc);
        let new_slot = MetaSlot {
            zkey,
            sq_mask,
            att_min: fp.att_min,
            def_max: fp.def_max,
            req_absent,
            sh: fp.sh,
            epoch: ep,
            nouse: fp.nouse,
            hinc: new_hinc,
        };

        let mut target_way: Option<usize> = None;
        let mut empty_way: Option<usize> = None;
        for (way, m) in set.ways.iter().enumerate() {
            if m.epoch != ep {
                if empty_way.is_none() {
                    empty_way = Some(way);
                }
                continue;
            }
            if m.zkey == zkey && m.att_min.0 == fp.att_min.0 && m.def_max.0 == fp.def_max.0 {
                target_way = Some(way);
                break;
            }
        }
        let target = match target_way.or(empty_way) {
            Some(w) => w,
            None => {
                let w = (set.rr & 3) as usize;
                set.rr = set.rr.wrapping_add(1);
                w
            }
        };
        set.ways[target] = new_slot;
        set.mru = target as u8;
    }

    #[inline(always)]
    pub fn lookup_meta(&self, zkey: u64, att_cur: MKey, def_cur: MKey) -> Option<NiFootprint> {
        let ep = self.meta_epoch;
        let set = &self.meta[meta_set_idx(zkey, self.meta_mask)];
        let mut best: Option<NiFootprint> = None;
        for step in 0..META_WAYS {
            let way = (set.mru.wrapping_sub(step as u8) & 3) as usize;
            let m = &set.ways[way];
            if m.epoch != ep || m.zkey != zkey {
                continue;
            }
            if !att_cur.ge_all(m.att_min) || !m.def_max.ge_all(def_cur) {
                continue;
            }
            let fp = NiFootprint {
                valid: true,
                mask: pack_ni_mask(m.sq_mask, m.req_absent, 0),
                att_min: m.att_min,
                def_max: m.def_max,
                sh: m.sh,
                hinc: m.hinc != 0,
                nouse: m.nouse,
            };
            if best.as_ref().is_none_or(|b| {
                (!fp.hinc && b.hinc)
                    || (fp.hinc == b.hinc && fp.sh < b.sh)
                    || (fp.hinc == b.hinc
                        && fp.sh == b.sh
                        && fp.sq_mask().count_ones() < b.sq_mask().count_ones())
            }) {
                best = Some(fp);
            }
        }
        best
    }

    /// Insert a sound proof `NiFootprint` into the 8-way bucket keyed by `ring_hash`.
    /// Requires `work >= MIN_NI_INSERT_WORK` and `fp.sh >= MIN_NI_INSERT_SH` so trivial
    /// 2-node proofs do not thrash valuable multi-ply local proofs.
    #[inline]
    pub fn insert(&mut self, ring_hash: u64, planes: &BoardPlanes, fp: &NiFootprint, work: u64) {
        if !fp.valid || work < MIN_NI_INSERT_WORK || fp.sh < MIN_NI_INSERT_SH {
            return;
        }
        let Some((sq_mask, req_absent)) = resolve_ni_mask(fp.mask) else {
            return;
        };
        if (req_absent & planes.white_slider_present_mask()) != 0 {
            return;
        }
        let bits = sq_mask.count_ones();
        if bits >= 68 {
            return;
        }
        let work_u16 = work.min(u16::MAX as u64) as u16;
        self.stats.mask_bits_sum += bits as u64;
        self.stats.mask_bits_cnt += 1;
        self.stats.inserts_pn0 += 1;

        let idx = (ring_hash as usize) & self.bucket_mask;
        let tag8 = ring_tag8(ring_hash);
        let ring_hi = (ring_hash >> 32) as u32;
        let win_shift = ring_win_shift(ring_hash);
        let win_mask = (sq_mask >> win_shift) as u64;
        let win_black = ((planes.black & sq_mask) >> win_shift) as u64;
        let win_white = ((planes.white & sq_mask) >> win_shift) as u64;
        let (att_min_req, def_max_req) = pack_mkeys_with_req(fp.att_min, fp.def_max, req_absent);

        let tags = self.bucket_tags[idx];
        let hdrs = &mut self.headers[idx].ways;
        let colds = &mut self.cold[idx].ways;

        let mut empty_way: Option<usize> = None;
        let mut min_work = u16::MAX;
        let mut min_way = 0usize;
        for way in 0..WAYS {
            let wt = ((tags >> (way * 8)) & 0xff) as u8;
            if wt == 0 {
                if empty_way.is_none() {
                    empty_way = Some(way);
                }
                continue;
            }
            let h = &mut hdrs[way];
            if wt == tag8
                && h.ring_hi == ring_hi
                && h.win_mask == win_mask
                && h.win_black == win_black
                && h.win_white == win_white
                && h.att_min_req == att_min_req
                && h.def_max_req == def_max_req
            {
                let c = &mut colds[way];
                if c.mask == sq_mask && planes.eq_on_mask(&c.planes, sq_mask) {
                    if (c.hinc != 0 && !fp.hinc) || (c.hinc == u8::from(fp.hinc) && fp.sh < h.sh) {
                        h.sh = fp.sh;
                        c.hinc = u8::from(fp.hinc);
                        c.nouse = fp.nouse;
                    }
                    if work_u16 > h.work {
                        h.work = work_u16;
                    }
                    return;
                }
            }
            if h.work < min_work {
                min_work = h.work;
                min_way = way;
            }
        }

        let target = match empty_way {
            Some(w) => w,
            None => {
                if work_u16 < min_work {
                    return;
                }
                min_way
            }
        };

        let shift = target * 8;
        self.bucket_tags[idx] = (tags & !(0xffu64 << shift)) | ((tag8 as u64) << shift);
        hdrs[target] = NiWayHeader {
            win_mask,
            win_black,
            win_white,
            att_min_req,
            def_max_req,
            ring_hi,
            sh: fp.sh,
            work: work_u16,
        };
        colds[target] = NiWayCold {
            planes: *planes,
            mask: sq_mask,
            nouse: fp.nouse,
            hinc: u8::from(fp.hinc),
        };
    }

    /// Stage-1 L1-tag check: returns non-zero SWAR lane bitmask iff at least one way in
    /// `bucket_tags[idx]` matches `ring_tag8(ring_hash)`. Allows callers to skip constructing
    /// `BoardPlanes` on all unmatched buckets!
    #[inline(always)]
    pub fn probe_tag(&mut self, ring_hash: u64) -> u64 {
        self.stats.lookups += 1;
        let idx = (ring_hash as usize) & self.bucket_mask;
        let tags = self.bucket_tags[idx];
        if tags == 0 {
            return 0;
        }
        swar_eq_bytes64(tags, ring_tag8(ring_hash))
    }

    /// Stage-2 query given a non-zero `matches` bitmask from `probe_tag`.
    #[inline(always)]
    pub fn lookup_with_matches(
        &mut self,
        ring_hash: u64,
        mut matches: u64,
        planes: &BoardPlanes,
        att_cur: MKey,
        def_cur: MKey,
        max_sh: u16,
        forbid_inc: bool,
    ) -> Option<NiHit> {
        debug_assert!(matches != 0);
        self.stats.tag_matches += 1;
        let idx = (ring_hash as usize) & self.bucket_mask;
        let ring_hi = (ring_hash >> 32) as u32;
        let win_shift = ring_win_shift(ring_hash);
        let q_black = (planes.black >> win_shift) as u64;
        let q_white = (planes.white >> win_shift) as u64;
        let q_pm = planes.white_slider_present_mask();

        let hdrs = &self.headers[idx].ways;
        let mut best: Option<NiHit> = None;

        while matches != 0 {
            let way = (matches.trailing_zeros() >> 3) as usize;
            matches &= matches - 1;
            let h = &hdrs[way];
            if h.ring_hi == ring_hi
                && ((q_black & h.win_mask) == h.win_black)
                && ((q_white & h.win_mask) == h.win_white)
                && h.sh < max_sh
            {
                let (h_att_min, h_def_max, h_req_absent) =
                    unpack_mkeys_with_req(h.att_min_req, h.def_max_req);
                if (q_pm & h_req_absent) != 0
                    || !att_cur.ge_all(h_att_min)
                    || !h_def_max.ge_all(def_cur)
                {
                    continue;
                }
                self.stats.cold_checks += 1;
                let c = &self.cold[idx].ways[way];
                let tb_mkey = proof_tbase_mkey(h_att_min, h_def_max, att_cur, def_cur);
                let eff_hinc = (c.hinc != 0) || (tb_mkey.total() > h_att_min.total());
                if forbid_inc && (eff_hinc || att_cur.0 != tb_mkey.0) {
                    continue;
                }
                if planes.eq_on_mask(&c.planes, c.mask) {
                    let hit = NiHit {
                        mask: pack_ni_mask(c.mask, h_req_absent, 0),
                        att_min: h_att_min,
                        def_max: h_def_max,
                        tdata: TData {
                            pn: 0,
                            dn: INFINATE,
                            sh: h.sh,
                        },
                        hinc: c.hinc != 0,
                        nouse: c.nouse,
                    };
                    if !eff_hinc {
                        self.stats.pn0_hits += 1;
                        return Some(hit);
                    }
                    if best.as_ref().is_none_or(|b| {
                        let b_tb = proof_tbase_mkey(b.att_min, b.def_max, att_cur, def_cur);
                        let b_eff_hinc = b.hinc || (b_tb.total() > b.att_min.total());
                        (!eff_hinc && b_eff_hinc)
                            || (eff_hinc == b_eff_hinc && hit.tdata.sh < b.tdata.sh)
                    }) {
                        best = Some(hit);
                    }
                }
            }
        }
        if best.is_some() {
            self.stats.pn0_hits += 1;
        }
        best
    }

    /// Full L1-tag + SoA header + exact 81-bit mask query for a 100%-sound mate proof (`pn == 0`).
    #[inline(always)]
    pub fn lookup(
        &mut self,
        ring_hash: u64,
        planes: &BoardPlanes,
        att_cur: MKey,
        def_cur: MKey,
        max_sh: u16,
    ) -> Option<NiHit> {
        let matches = self.probe_tag(ring_hash);
        if matches == 0 {
            return None;
        }
        self.lookup_with_matches(ring_hash, matches, planes, att_cur, def_cur, max_sh, false)
    }
}

const SHARED_TAG_WORDS: usize = 2;
const SHARED_WAYS: usize = SHARED_TAG_WORDS * 8;

/// 16-byte (never straddles a 64B cache line) 16-way SWAR tag array per bucket.
#[repr(C, align(16))]
struct SharedBucketTags {
    words: [AtomicU64; SHARED_TAG_WORDS],
}
const _: () = assert!(std::mem::size_of::<SharedBucketTags>() == 16);

/// SoA 63-square `(mask, black, white)` bitboard filter for all 16 ways (`384 B`, 64B-aligned),
/// allowing AVX2 to test 8 ways for both `black` and `white` simultaneously in zero branches!
#[repr(C, align(64))]
struct SharedNiBucketWin {
    mask: [AtomicU64; SHARED_WAYS],
    black: [AtomicU64; SHARED_WAYS],
    white: [AtomicU64; SHARED_WAYS],
}
const _: () = assert!(std::mem::size_of::<SharedNiBucketWin>() == 384);

/// 24-byte remaining header fields per way (`att_min`, `def_max`, `packed`), touched ONLY when
/// both `black` and `white` 63-square window bitboards match!
#[repr(C)]
struct SharedNiWayRest {
    att_min: AtomicU64,
    def_max: AtomicU64,
    /// `ring_hi: u32 | (sh: u16 << 32) | (work: u16 << 48)`
    packed: AtomicU64,
}
const _: () = assert!(std::mem::size_of::<SharedNiWayRest>() == 24);

#[repr(C, align(64))]
struct SharedNiBucketRest {
    ways: [SharedNiWayRest; SHARED_WAYS],
}

/// 120-byte atomic cold slot storing exact 81-bit `BoardPlanes` + `mask` + `(nouse, hinc)`.
#[repr(C)]
struct SharedNiWayCold {
    words: [AtomicU64; 15],
}

impl SharedNiWayCold {
    #[inline(always)]
    fn store_relaxed(&self, planes: &BoardPlanes, mask: u128, nouse: u8, hinc: bool) {
        self.words[0].store(planes.black as u64, Ordering::Relaxed);
        self.words[1].store((planes.black >> 64) as u64, Ordering::Relaxed);
        self.words[2].store(planes.white as u64, Ordering::Relaxed);
        self.words[3].store((planes.white >> 64) as u64, Ordering::Relaxed);
        self.words[4].store(planes.prom as u64, Ordering::Relaxed);
        self.words[5].store((planes.prom >> 64) as u64, Ordering::Relaxed);
        self.words[6].store(planes.k0 as u64, Ordering::Relaxed);
        self.words[7].store((planes.k0 >> 64) as u64, Ordering::Relaxed);
        self.words[8].store(planes.k1 as u64, Ordering::Relaxed);
        self.words[9].store((planes.k1 >> 64) as u64, Ordering::Relaxed);
        self.words[10].store(planes.k2 as u64, Ordering::Relaxed);
        self.words[11].store((planes.k2 >> 64) as u64, Ordering::Relaxed);
        self.words[12].store(mask as u64, Ordering::Relaxed);
        self.words[13].store((mask >> 64) as u64, Ordering::Relaxed);
        self.words[14].store((nouse as u64) | (u64::from(hinc) << 8), Ordering::Relaxed);
    }

    #[inline(always)]
    fn load_relaxed(&self) -> (BoardPlanes, u128, u8, bool) {
        let b_lo = self.words[0].load(Ordering::Relaxed) as u128;
        let b_hi = (self.words[1].load(Ordering::Relaxed) as u128) << 64;
        let w_lo = self.words[2].load(Ordering::Relaxed) as u128;
        let w_hi = (self.words[3].load(Ordering::Relaxed) as u128) << 64;
        let p_lo = self.words[4].load(Ordering::Relaxed) as u128;
        let p_hi = (self.words[5].load(Ordering::Relaxed) as u128) << 64;
        let k0_lo = self.words[6].load(Ordering::Relaxed) as u128;
        let k0_hi = (self.words[7].load(Ordering::Relaxed) as u128) << 64;
        let k1_lo = self.words[8].load(Ordering::Relaxed) as u128;
        let k1_hi = (self.words[9].load(Ordering::Relaxed) as u128) << 64;
        let k2_lo = self.words[10].load(Ordering::Relaxed) as u128;
        let k2_hi = (self.words[11].load(Ordering::Relaxed) as u128) << 64;
        let m_lo = self.words[12].load(Ordering::Relaxed) as u128;
        let m_hi = (self.words[13].load(Ordering::Relaxed) as u128) << 64;
        let meta = self.words[14].load(Ordering::Relaxed);
        (
            BoardPlanes {
                black: b_lo | b_hi,
                white: w_lo | w_hi,
                prom: p_lo | p_hi,
                k0: k0_lo | k0_hi,
                k1: k1_lo | k1_hi,
                k2: k2_lo | k2_hi,
            },
            m_lo | m_hi,
            (meta & 0xff) as u8,
            ((meta >> 8) & 1) != 0,
        )
    }
}

#[repr(C, align(64))]
struct SharedNiColdBucket {
    ways: [SharedNiWayCold; SHARED_WAYS],
}

/// 64-byte single-cache-line seqlock metadata slot (`zkey -> (NiFootprint, work)`),
/// allowing cooperative worker threads to compose parent footprints across threads!
#[repr(C, align(64))]
struct SharedMetaSlot {
    seq: AtomicU32,
    epoch: AtomicU32,
    zkey: AtomicU64,
    mask_lo: AtomicU64,
    /// `mask_hi(0..17) | (sh << 17) | (nouse << 33) | (hinc << 41) | (work << 48)`
    mask_hi_meta: AtomicU64,
    att_min: AtomicU64,
    def_max: AtomicU64,
    req_absent: AtomicU64,
    _pad: u64,
}
const _: () = assert!(std::mem::size_of::<SharedMetaSlot>() == 64);

#[repr(C, align(64))]
struct SharedMetaSet {
    ways: [SharedMetaSlot; META_WAYS],
}

#[inline(always)]
fn pack_rest_meta(ring_hi: u32, sh: u16, work: u16) -> u64 {
    (ring_hi as u64) | ((sh as u64) << 32) | ((work as u64) << 48)
}

#[inline(always)]
fn unpack_rest_meta(w: u64) -> (u32, u16, u16) {
    (w as u32, ((w >> 32) & 0xffff) as u16, (w >> 48) as u16)
}

/// Lock-free / wait-free cross-thread 16-way seqlock `SharedNiCache` for `ParallelEngine`.
///
/// - 16 ways per bucket (`SHARED_WAYS = 16`) + 4-way set-associative `SharedMetaSet`.
/// - Readers perform ZERO atomic writes (`Relaxed` loads validated by `Acquire` seqlock `seq == s0`),
///   keeping `bucket_tags` and active `win` lines in Shared (`S`) L1/L2 state across all cores.
/// - Writers use an optimistic read-only pre-check + wait-free `try_lock` on per-bucket/per-slot `seq`.
/// - Allocated 100% via `alloc_zeroed` for `0.0 ms` cold-start initialization.
pub struct SharedNiCache {
    bucket_tags: Box<[SharedBucketTags]>,
    bucket_seqs: Box<[AtomicU32]>,
    win: Box<[SharedNiBucketWin]>,
    rest: Box<[SharedNiBucketRest]>,
    cold: Box<[SharedNiColdBucket]>,
    bucket_mask: usize,
    meta: Box<[SharedMetaSet]>,
    meta_mask: usize,
    meta_epoch: AtomicU32,
    bucket_dirty: AtomicBool,
}

impl SharedNiCache {
    pub fn new(num_buckets: usize, meta_entries: usize) -> Self {
        let nb = num_buckets.next_power_of_two().max(64);
        let nm_sets = (meta_entries / META_WAYS).next_power_of_two().max(256);
        // SAFETY: All slice element types consist solely of AtomicU32/AtomicU64/u64 fields
        // in #[repr(C)] structs whose all-zero bit pattern is valid and represents "empty".
        unsafe {
            Self {
                bucket_tags: alloc_zeroed_slice::<SharedBucketTags>(nb),
                bucket_seqs: alloc_zeroed_slice::<AtomicU32>(nb),
                win: alloc_zeroed_slice::<SharedNiBucketWin>(nb),
                rest: alloc_zeroed_slice::<SharedNiBucketRest>(nb),
                cold: alloc_zeroed_slice::<SharedNiColdBucket>(nb),
                bucket_mask: nb - 1,
                meta: alloc_zeroed_slice::<SharedMetaSet>(nm_sets),
                meta_mask: nm_sets - 1,
                meta_epoch: AtomicU32::new(1),
                bucket_dirty: AtomicBool::new(false),
            }
        }
    }

    /// Instant O(1) clear on cold start (`bucket_dirty == false` -> 0 page touches!),
    /// and ~2–4 µs tag reset + O(1) epoch bump between consecutive solves.
    pub fn clear(&self) {
        if self.bucket_dirty.swap(false, Ordering::AcqRel) {
            for (b_idx, tb) in self.bucket_tags.iter().enumerate() {
                if tb.words[0].load(Ordering::Relaxed) != 0 {
                    for w in &tb.words {
                        w.store(0, Ordering::Relaxed);
                    }
                    self.bucket_seqs[b_idx].fetch_add(2, Ordering::Release);
                }
            }
        }
        let next = self.meta_epoch.load(Ordering::Relaxed).wrapping_add(1);
        if next == 0 {
            for set in self.meta.iter() {
                for slot in &set.ways {
                    slot.epoch.store(0, Ordering::Relaxed);
                }
            }
            self.meta_epoch.store(1, Ordering::Release);
        } else {
            self.meta_epoch.store(next, Ordering::Release);
        }
    }

    #[inline(always)]
    pub fn record_meta(&self, zkey: u64, fp: &NiFootprint, work: u16) {
        if !fp.valid {
            return;
        }
        let Some((sq_mask, req_absent)) = resolve_ni_mask(fp.mask) else {
            return;
        };
        let set = unsafe { self.meta.get_unchecked(meta_set_idx(zkey, self.meta_mask)) };
        let ep = self.meta_epoch.load(Ordering::Relaxed);

        let mut target_way: Option<(usize, u32)> = None;
        let mut empty_way: Option<(usize, u32)> = None;
        let mut oldest_way: Option<(usize, u32)> = None;

        for (w, slot) in set.ways.iter().enumerate() {
            let s = slot.seq.load(Ordering::Relaxed);
            if (s & 1) != 0 {
                continue;
            }
            if slot.epoch.load(Ordering::Relaxed) != ep {
                if empty_way.is_none() {
                    empty_way = Some((w, s));
                }
                continue;
            }
            if slot.zkey.load(Ordering::Relaxed) == zkey
                && slot.att_min.load(Ordering::Relaxed) == fp.att_min.0
                && slot.def_max.load(Ordering::Relaxed) == fp.def_max.0
            {
                let old_packed = slot.mask_hi_meta.load(Ordering::Relaxed);
                let old_sh = ((old_packed >> 17) & 0xffff) as u16;
                let old_hinc = ((old_packed >> 41) & 1) != 0;
                let old_work = (old_packed >> 48) as u16;
                let old_req =
                    (slot.req_absent.load(Ordering::Relaxed) & (NI_SLIDER_MASK as u64)) as u16;
                if old_sh == fp.sh
                    && old_hinc == fp.hinc
                    && old_req == req_absent
                    && slot.mask_lo.load(Ordering::Relaxed) == (sq_mask as u64)
                    && work <= old_work
                {
                    return;
                }
                target_way = Some((w, s));
                break;
            }
            if oldest_way.is_none_or(|(_, min_s)| s < min_s) {
                oldest_way = Some((w, s));
            }
        }

        let Some((w, s)) = target_way.or(empty_way).or(oldest_way) else {
            return;
        };
        let slot = &set.ways[w];
        if slot
            .seq
            .compare_exchange(s, s.wrapping_add(1), Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            return;
        }
        let mask_lo = sq_mask as u64;
        let mask_hi = ((sq_mask >> 64) as u64) & 0x1_ffff;
        let packed = mask_hi
            | ((fp.sh as u64) << 17)
            | ((fp.nouse as u64) << 33)
            | (u64::from(fp.hinc) << 41)
            | ((work as u64) << 48);
        slot.zkey.store(zkey, Ordering::Relaxed);
        slot.mask_lo.store(mask_lo, Ordering::Relaxed);
        slot.mask_hi_meta.store(packed, Ordering::Relaxed);
        slot.att_min.store(fp.att_min.0, Ordering::Relaxed);
        slot.def_max.store(fp.def_max.0, Ordering::Relaxed);
        slot.req_absent.store(req_absent as u64, Ordering::Relaxed);
        slot.epoch.store(ep, Ordering::Relaxed);
        slot.seq.store(s.wrapping_add(2), Ordering::Release);
    }

    #[inline(always)]
    pub fn lookup_meta(
        &self,
        zkey: u64,
        att_cur: MKey,
        def_cur: MKey,
    ) -> Option<(NiFootprint, u16)> {
        let set = unsafe { self.meta.get_unchecked(meta_set_idx(zkey, self.meta_mask)) };
        let ep = self.meta_epoch.load(Ordering::Relaxed);
        let mut best: Option<(NiFootprint, u16)> = None;
        for slot in &set.ways {
            let s0 = slot.seq.load(Ordering::Acquire);
            if (s0 & 1) != 0 {
                continue;
            }
            if slot.epoch.load(Ordering::Relaxed) != ep {
                continue;
            }
            if slot.zkey.load(Ordering::Relaxed) != zkey {
                continue;
            }
            let att_min = MKey(slot.att_min.load(Ordering::Relaxed));
            let def_max = MKey(slot.def_max.load(Ordering::Relaxed));
            if !att_cur.ge_all(att_min) || !def_max.ge_all(def_cur) {
                continue;
            }
            let mask_lo = slot.mask_lo.load(Ordering::Relaxed);
            let packed = slot.mask_hi_meta.load(Ordering::Relaxed);
            let req_absent =
                (slot.req_absent.load(Ordering::Relaxed) & (NI_SLIDER_MASK as u64)) as u16;
            fence(Ordering::Acquire);
            if slot.seq.load(Ordering::Relaxed) != s0 {
                continue;
            }
            let sq_mask = (mask_lo as u128) | (((packed & 0x1_ffff) as u128) << 64);
            let sh = ((packed >> 17) & 0xffff) as u16;
            let nouse = ((packed >> 33) & 0xff) as u8;
            let hinc = ((packed >> 41) & 1) != 0;
            let work = (packed >> 48) as u16;
            let fp = NiFootprint {
                valid: true,
                mask: pack_ni_mask(sq_mask, req_absent, 0),
                att_min,
                def_max,
                sh,
                hinc,
                nouse,
            };
            if best.as_ref().is_none_or(|(b, _)| {
                (!fp.hinc && b.hinc)
                    || (fp.hinc == b.hinc && fp.sh < b.sh)
                    || (fp.hinc == b.hinc
                        && fp.sh == b.sh
                        && fp.sq_mask().count_ones() < b.sq_mask().count_ones())
            }) {
                best = Some((fp, work));
            }
        }
        best
    }

    #[inline]
    pub fn insert(&self, ring_hash: u64, planes: &BoardPlanes, fp: &NiFootprint, work: u64) {
        if !fp.valid || work < MIN_NI_INSERT_WORK || fp.sh < MIN_NI_INSERT_SH {
            return;
        }
        let Some((sq_mask, req_absent)) = resolve_ni_mask(fp.mask) else {
            return;
        };
        if (req_absent & planes.white_slider_present_mask()) != 0 {
            return;
        }
        if sq_mask.count_ones() >= 68 {
            return;
        }
        let work_u16 = work.min(u16::MAX as u64) as u16;
        let idx = (ring_hash as usize) & self.bucket_mask;
        let tag8 = ring_tag8(ring_hash);
        let ring_hi = (ring_hash >> 32) as u32;
        let win_shift = ring_win_shift(ring_hash);
        let win_mask = (sq_mask >> win_shift) as u64;
        let win_black = ((planes.black & sq_mask) >> win_shift) as u64;
        let win_white = ((planes.white & sq_mask) >> win_shift) as u64;
        let (att_min_req, def_max_req) = pack_mkeys_with_req(fp.att_min, fp.def_max, req_absent);

        let tags_b = unsafe { self.bucket_tags.get_unchecked(idx) };
        let win_b = unsafe { self.win.get_unchecked(idx) };
        let rest_b = unsafe { self.rest.get_unchecked(idx) };
        let cold_b = unsafe { self.cold.get_unchecked(idx) };

        // Optimistic zero-write pre-check across occupied ways (`wt != 0`):
        // 1. Filters duplicate footprints on BOTH partial and full buckets without taking `seq`!
        // 2. Once all 16 ways are full, filters `work_u16 <= min_w` without dirtying `seq`!
        let last_tw = tags_b.words[SHARED_TAG_WORDS - 1].load(Ordering::Relaxed);
        let pre_full = (last_tw >> 56) != 0;
        let mut pre_min_w = u16::MAX;
        for word_idx in 0..SHARED_TAG_WORDS {
            let tw = if word_idx == SHARED_TAG_WORDS - 1 {
                last_tw
            } else {
                tags_b.words[word_idx].load(Ordering::Relaxed)
            };
            if tw == 0 {
                break;
            }
            if pre_full {
                for lane in 0..8 {
                    let way = word_idx * 8 + lane;
                    let (_, _, hw) =
                        unpack_rest_meta(rest_b.ways[way].packed.load(Ordering::Relaxed));
                    if hw < pre_min_w {
                        pre_min_w = hw;
                    }
                }
            }
            let mut m = swar_eq_bytes64(tw, tag8);
            let base_way = word_idx * 8;
            while m != 0 {
                let way = base_way + (m.trailing_zeros() >> 3) as usize;
                m &= m - 1;
                if win_b.mask[way].load(Ordering::Relaxed) != win_mask
                    || win_b.black[way].load(Ordering::Relaxed) != win_black
                    || win_b.white[way].load(Ordering::Relaxed) != win_white
                {
                    continue;
                }
                let r = &rest_b.ways[way];
                let (h_ring_hi, h_sh, h_work) = unpack_rest_meta(r.packed.load(Ordering::Relaxed));
                if h_ring_hi == ring_hi
                    && h_sh <= fp.sh
                    && work_u16 <= h_work
                    && r.att_min.load(Ordering::Relaxed) == att_min_req
                    && r.def_max.load(Ordering::Relaxed) == def_max_req
                {
                    let (c_planes, c_mask, _, c_hinc) = cold_b.ways[way].load_relaxed();
                    if (!c_hinc || fp.hinc)
                        && c_mask == sq_mask
                        && planes.eq_on_mask(&c_planes, sq_mask)
                    {
                        return;
                    }
                }
            }
        }
        if pre_full && work_u16 <= pre_min_w {
            return;
        }

        let seq_ref = unsafe { self.bucket_seqs.get_unchecked(idx) };
        let s = seq_ref.load(Ordering::Relaxed);
        if (s & 1) != 0 {
            return;
        }
        if seq_ref
            .compare_exchange(s, s.wrapping_add(1), Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            return;
        }

        if !self.bucket_dirty.load(Ordering::Relaxed) {
            self.bucket_dirty.store(true, Ordering::Relaxed);
        }

        let mut tag_words = [0u64; SHARED_TAG_WORDS];
        for (dst, src) in tag_words.iter_mut().zip(tags_b.words.iter()) {
            *dst = src.load(Ordering::Relaxed);
        }

        let mut empty_way: Option<usize> = None;
        let mut min_work = u16::MAX;
        let mut min_way = 0usize;
        'scan: for (word_idx, &tw) in tag_words.iter().enumerate() {
            for lane in 0..8 {
                let way = word_idx * 8 + lane;
                let wt = ((tw >> (lane * 8)) & 0xff) as u8;
                if wt == 0 {
                    empty_way = Some(way);
                    break 'scan;
                }
                let r = &rest_b.ways[way];
                let (h_ring_hi, h_sh, h_work) = unpack_rest_meta(r.packed.load(Ordering::Relaxed));
                if wt == tag8
                    && h_ring_hi == ring_hi
                    && win_b.mask[way].load(Ordering::Relaxed) == win_mask
                    && win_b.black[way].load(Ordering::Relaxed) == win_black
                    && win_b.white[way].load(Ordering::Relaxed) == win_white
                    && r.att_min.load(Ordering::Relaxed) == att_min_req
                    && r.def_max.load(Ordering::Relaxed) == def_max_req
                {
                    let (c_planes, c_mask, _c_nouse, c_hinc) = cold_b.ways[way].load_relaxed();
                    if c_mask == sq_mask && planes.eq_on_mask(&c_planes, sq_mask) {
                        let mut new_sh = h_sh;
                        let new_work = h_work.max(work_u16);
                        if (c_hinc && !fp.hinc) || (c_hinc == fp.hinc && fp.sh < h_sh) {
                            new_sh = fp.sh;
                            cold_b.ways[way].words[14].store(
                                (fp.nouse as u64) | (u64::from(fp.hinc) << 8),
                                Ordering::Relaxed,
                            );
                        }
                        if new_sh != h_sh || new_work != h_work {
                            r.packed
                                .store(pack_rest_meta(ring_hi, new_sh, new_work), Ordering::Relaxed);
                        }
                        seq_ref.store(s.wrapping_add(2), Ordering::Release);
                        return;
                    }
                }
                if h_work < min_work {
                    min_work = h_work;
                    min_way = way;
                }
            }
        }

        let target = match empty_way {
            Some(w) => w,
            None => {
                if work_u16 <= min_work {
                    seq_ref.store(s.wrapping_add(2), Ordering::Release);
                    return;
                }
                min_way
            }
        };

        win_b.mask[target].store(win_mask, Ordering::Relaxed);
        win_b.black[target].store(win_black, Ordering::Relaxed);
        win_b.white[target].store(win_white, Ordering::Relaxed);
        let r = &rest_b.ways[target];
        r.att_min.store(att_min_req, Ordering::Relaxed);
        r.def_max.store(def_max_req, Ordering::Relaxed);
        r.packed
            .store(pack_rest_meta(ring_hi, fp.sh, work_u16), Ordering::Relaxed);
        cold_b.ways[target].store_relaxed(planes, sq_mask, fp.nouse, fp.hinc);

        let tw_idx = target >> 3;
        let shift = (target & 7) * 8;
        let new_tw = (tag_words[tw_idx] & !(0xffu64 << shift)) | ((tag8 as u64) << shift);
        tags_b.words[tw_idx].store(new_tw, Ordering::Release);
        seq_ref.store(s.wrapping_add(2), Ordering::Release);
    }

    #[inline(always)]
    pub fn has_tag_match(&self, ring_hash: u64) -> bool {
        let idx = (ring_hash as usize) & self.bucket_mask;
        let tb = unsafe { self.bucket_tags.get_unchecked(idx) };
        let t0 = tb.words[0].load(Ordering::Relaxed);
        if t0 == 0 {
            return false;
        }
        let tag8 = ring_tag8(ring_hash);
        if swar_eq_bytes64(t0, tag8) != 0 {
            return true;
        }
        if (t0 >> 56) == 0 {
            return false;
        }
        let t1 = tb.words[1].load(Ordering::Relaxed);
        t1 != 0 && swar_eq_bytes64(t1, tag8) != 0
    }

    #[inline(always)]
    pub fn lookup_hit(
        &self,
        ring_hash: u64,
        planes: &BoardPlanes,
        att_cur: MKey,
        def_cur: MKey,
        max_sh: u16,
        forbid_inc: bool,
    ) -> Option<(NiHit, u16)> {
        let idx = (ring_hash as usize) & self.bucket_mask;
        let seq_ref = unsafe { self.bucket_seqs.get_unchecked(idx) };
        let s0 = seq_ref.load(Ordering::Acquire);
        if (s0 & 1) != 0 {
            return None;
        }
        let tb = unsafe { self.bucket_tags.get_unchecked(idx) };
        let t0 = tb.words[0].load(Ordering::Relaxed);
        if t0 == 0 {
            return None;
        }

        let tag8 = ring_tag8(ring_hash);
        let win_shift = ring_win_shift(ring_hash);
        let q_black = (planes.black >> win_shift) as u64;
        let q_white = (planes.white >> win_shift) as u64;
        let win_b = unsafe { self.win.get_unchecked(idx) };

        let mut cand16: u32;
        #[cfg(target_arch = "x86_64")]
        {
            if std::arch::is_x86_feature_detected!("avx2") {
                cand16 = unsafe { match_win_bucket16_avx2(win_b, tb, t0, tag8, q_black, q_white) };
            } else {
                cand16 = match_win_bucket16_scalar(win_b, tb, t0, tag8, q_black, q_white);
            }
        }
        #[cfg(not(target_arch = "x86_64"))]
        {
            cand16 = match_win_bucket16_scalar(win_b, tb, t0, tag8, q_black, q_white);
        }

        if cand16 == 0 {
            return None;
        }

        let q_pm = planes.white_slider_present_mask();
        let ring_hi = (ring_hash >> 32) as u32;
        let rest_b = unsafe { self.rest.get_unchecked(idx) };
        let cold_b = unsafe { self.cold.get_unchecked(idx) };
        let mut best: Option<(NiHit, u16)> = None;

        while cand16 != 0 {
            let way = cand16.trailing_zeros() as usize;
            cand16 &= cand16 - 1;
            let r = &rest_b.ways[way];
            let (h_ring_hi, h_sh, h_work) = unpack_rest_meta(r.packed.load(Ordering::Relaxed));
            if h_ring_hi != ring_hi || h_sh >= max_sh {
                continue;
            }
            let (h_att_min, h_def_max, h_req_absent) = unpack_mkeys_with_req(
                r.att_min.load(Ordering::Relaxed),
                r.def_max.load(Ordering::Relaxed),
            );
            if (q_pm & h_req_absent) != 0
                || !att_cur.ge_all(h_att_min)
                || !h_def_max.ge_all(def_cur)
            {
                continue;
            }
            let (c_planes, c_mask, c_nouse, c_hinc) = cold_b.ways[way].load_relaxed();
            fence(Ordering::Acquire);
            if seq_ref.load(Ordering::Relaxed) != s0 {
                return None;
            }
            let tb_mkey = proof_tbase_mkey(h_att_min, h_def_max, att_cur, def_cur);
            let eff_hinc = c_hinc || (tb_mkey.total() > h_att_min.total());
            if forbid_inc && (eff_hinc || att_cur.0 != tb_mkey.0) {
                continue;
            }
            if planes.eq_on_mask(&c_planes, c_mask) {
                let hit = NiHit {
                    mask: pack_ni_mask(c_mask, h_req_absent, 0),
                    att_min: h_att_min,
                    def_max: h_def_max,
                    tdata: TData {
                        pn: 0,
                        dn: INFINATE,
                        sh: h_sh,
                    },
                    hinc: c_hinc,
                    nouse: c_nouse,
                };
                if !eff_hinc {
                    return Some((hit, h_work));
                }
                if best.as_ref().is_none_or(|(b, _)| {
                    let b_tb = proof_tbase_mkey(b.att_min, b.def_max, att_cur, def_cur);
                    let b_eff_hinc = b.hinc || (b_tb.total() > b.att_min.total());
                    (!eff_hinc && b_eff_hinc)
                        || (eff_hinc == b_eff_hinc && hit.tdata.sh < b.tdata.sh)
                }) {
                    best = Some((hit, h_work));
                }
            }
        }
        best
    }
}

#[inline(always)]
fn match_win_bucket16_scalar(
    win_b: &SharedNiBucketWin,
    tb: &SharedBucketTags,
    t0: u64,
    tag8: u8,
    q_black: u64,
    q_white: u64,
) -> u32 {
    let mut out = 0u32;
    for word_idx in 0..SHARED_TAG_WORDS {
        let cur = if word_idx == 0 {
            t0
        } else {
            if (t0 >> 56) == 0 {
                break;
            }
            let tw = tb.words[word_idx].load(Ordering::Relaxed);
            if tw == 0 {
                break;
            }
            tw
        };
        let mut m = swar_eq_bytes64(cur, tag8);
        let base = word_idx * 8;
        while m != 0 {
            let way = base + (m.trailing_zeros() >> 3) as usize;
            m &= m - 1;
            let wm = win_b.mask[way].load(Ordering::Relaxed);
            if (q_black & wm) == win_b.black[way].load(Ordering::Relaxed)
                && (q_white & wm) == win_b.white[way].load(Ordering::Relaxed)
            {
                out |= 1u32 << way;
            }
        }
    }
    out
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
unsafe fn match_win_bucket16_avx2(
    win_b: &SharedNiBucketWin,
    tb: &SharedBucketTags,
    t0: u64,
    tag8: u8,
    q_black: u64,
    q_white: u64,
) -> u32 {
    use std::arch::x86_64::*;
    let vtag = _mm_set1_epi8(tag8 as i8);
    let m0_tags =
        (_mm_movemask_epi8(_mm_cmpeq_epi8(_mm_cvtsi64_si128(t0 as i64), vtag)) as u32) & 0xff;
    let qb = _mm256_set1_epi64x(q_black as i64);
    let qw = _mm256_set1_epi64x(q_white as i64);

    #[inline(always)]
    unsafe fn check4(
        win_b: &SharedNiBucketWin,
        base: usize,
        qb: __m256i,
        qw: __m256i,
    ) -> u32 {
        let m = _mm256_set_epi64x(
            win_b.mask[base + 3].load(Ordering::Relaxed) as i64,
            win_b.mask[base + 2].load(Ordering::Relaxed) as i64,
            win_b.mask[base + 1].load(Ordering::Relaxed) as i64,
            win_b.mask[base].load(Ordering::Relaxed) as i64,
        );
        let b = _mm256_set_epi64x(
            win_b.black[base + 3].load(Ordering::Relaxed) as i64,
            win_b.black[base + 2].load(Ordering::Relaxed) as i64,
            win_b.black[base + 1].load(Ordering::Relaxed) as i64,
            win_b.black[base].load(Ordering::Relaxed) as i64,
        );
        let w = _mm256_set_epi64x(
            win_b.white[base + 3].load(Ordering::Relaxed) as i64,
            win_b.white[base + 2].load(Ordering::Relaxed) as i64,
            win_b.white[base + 1].load(Ordering::Relaxed) as i64,
            win_b.white[base].load(Ordering::Relaxed) as i64,
        );
        let ok_b = _mm256_cmpeq_epi64(_mm256_and_si256(qb, m), b);
        let ok_w = _mm256_cmpeq_epi64(_mm256_and_si256(qw, m), w);
        _mm256_movemask_pd(_mm256_castsi256_pd(_mm256_and_si256(ok_b, ok_w))) as u32
    }

    let mut out = 0u32;
    if (m0_tags & 0x0f) != 0 {
        out |= (m0_tags & 0x0f) & check4(win_b, 0, qb, qw);
    }
    if (m0_tags & 0xf0) != 0 {
        out |= (m0_tags & 0xf0) & (check4(win_b, 4, qb, qw) << 4);
    }
    if (t0 >> 56) != 0 {
        let t1 = tb.words[1].load(Ordering::Relaxed);
        if t1 != 0 {
            let m1_tags = ((_mm_movemask_epi8(_mm_cmpeq_epi8(_mm_cvtsi64_si128(t1 as i64), vtag))
                as u32)
                & 0xff)
                << 8;
            if (m1_tags & 0x0f00) != 0 {
                out |= (m1_tags & 0x0f00) & (check4(win_b, 8, qb, qw) << 8);
            }
            if (m1_tags & 0xf000) != 0 {
                out |= (m1_tags & 0xf000) & (check4(win_b, 12, qb, qw) << 12);
            }
        }
    }
    out
}

#[repr(C)]
#[derive(Copy, Clone, Default)]
struct LocalMetaSlot {
    zkey: u64,
    sq_mask: u128,
    att_min: MKey,
    def_max: MKey,
    req_absent: u16,
    sh: u16,
    work: u16,
    epoch: u16,
    nouse: u8,
    hinc: u8,
}

#[repr(C)]
#[derive(Copy, Clone, Default)]
struct LocalMetaSet {
    ways: [LocalMetaSlot; META_WAYS],
    rr: u8,
    mru: u8,
}

/// Core-private L1/L2 4-way set-associative `zkey -> (NiFootprint, work)` table (`4096` slots),
/// absorbing ~98%+ of same-thread leaf/child footprint handoffs with zero atomics!
pub(crate) struct LocalMetaTable {
    sets: Box<[LocalMetaSet]>,
    mask: usize,
    epoch: u16,
}

impl LocalMetaTable {
    pub(crate) fn new(num_slots: usize, init_epoch: u64) -> Self {
        let n_sets = (num_slots / META_WAYS).next_power_of_two().max(256);
        let ep = ((init_epoch as u16) & 0x7fff).max(1);
        // SAFETY: LocalMetaSet is all-zero valid (`epoch == 0 != ep`).
        Self {
            sets: unsafe { alloc_zeroed_slice::<LocalMetaSet>(n_sets) },
            mask: n_sets - 1,
            epoch: ep,
        }
    }

    #[inline]
    pub(crate) fn sync_epoch(&mut self, global_epoch: u64) {
        let ep = ((global_epoch as u16) & 0x7fff).max(1);
        if self.epoch != ep {
            self.epoch = ep;
            if ep == 1 {
                for set in self.sets.iter_mut() {
                    for s in &mut set.ways {
                        s.epoch = 0;
                    }
                }
            }
        }
    }

    #[inline(always)]
    pub(crate) fn record(&mut self, zkey: u64, fp: &NiFootprint, work: u16) {
        if !fp.valid {
            return;
        }
        let Some((sq_mask, req_absent)) = resolve_ni_mask(fp.mask) else {
            return;
        };
        let ep = self.epoch;
        let set = unsafe { self.sets.get_unchecked_mut(meta_set_idx(zkey, self.mask)) };
        let new_hinc = u8::from(fp.hinc);
        let new_slot = LocalMetaSlot {
            zkey,
            sq_mask,
            att_min: fp.att_min,
            def_max: fp.def_max,
            req_absent,
            sh: fp.sh,
            work,
            epoch: ep,
            nouse: fp.nouse,
            hinc: new_hinc,
        };

        let mut target_way: Option<usize> = None;
        let mut empty_way: Option<usize> = None;
        let mut prev_work = 0u16;
        for (way, m) in set.ways.iter().enumerate() {
            if m.epoch != ep {
                if empty_way.is_none() {
                    empty_way = Some(way);
                }
                continue;
            }
            if m.zkey == zkey && m.att_min.0 == fp.att_min.0 && m.def_max.0 == fp.def_max.0 {
                prev_work = m.work;
                target_way = Some(way);
                break;
            }
        }
        let target = match target_way.or(empty_way) {
            Some(w) => w,
            None => {
                let w = (set.rr & 3) as usize;
                set.rr = set.rr.wrapping_add(1);
                w
            }
        };
        set.ways[target] = LocalMetaSlot {
            work: work.max(prev_work),
            ..new_slot
        };
        set.mru = target as u8;
    }

    #[inline(always)]
    pub(crate) fn lookup(
        &self,
        zkey: u64,
        att_cur: MKey,
        def_cur: MKey,
    ) -> Option<(NiFootprint, u16)> {
        let ep = self.epoch;
        let set = unsafe { self.sets.get_unchecked(meta_set_idx(zkey, self.mask)) };
        let mut best: Option<(NiFootprint, u16)> = None;
        for step in 0..META_WAYS {
            let way = (set.mru.wrapping_sub(step as u8) & 3) as usize;
            let m = &set.ways[way];
            if m.epoch != ep || m.zkey != zkey {
                continue;
            }
            if !att_cur.ge_all(m.att_min) || !m.def_max.ge_all(def_cur) {
                continue;
            }
            let fp = NiFootprint {
                valid: true,
                mask: pack_ni_mask(m.sq_mask, m.req_absent, 0),
                att_min: m.att_min,
                def_max: m.def_max,
                sh: m.sh,
                hinc: m.hinc != 0,
                nouse: m.nouse,
            };
            if best.as_ref().is_none_or(|(b, _)| {
                (!fp.hinc && b.hinc)
                    || (fp.hinc == b.hinc && fp.sh < b.sh)
                    || (fp.hinc == b.hinc
                        && fp.sh == b.sh
                        && fp.sq_mask().count_ones() < b.sq_mask().count_ones())
            }) {
                best = Some((fp, m.work));
            }
        }
        best
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::futsuu::{FmrsPos, PositionBackend, SearchConfig, Solver};

    #[test]
    fn test_ni_cache_swar_tag_and_soundness() {
        let mut cache = NiCache::new(256, 1024);
        let p = BoardPlanes {
            black: 0x101,
            white: 0x202,
            ..Default::default()
        };
        let fp = NiFootprint {
            valid: true,
            mask: 0x3ff,
            att_min: MKey::new(1, 0, 0, 0, 0, 0, 0),
            def_max: MAX_HAND,
            sh: 3,
            hinc: false,
            nouse: 0,
        };
        let h = 0x1234_5678_9abc_def0u64;
        cache.insert(h, &p, &fp, 10);
        // Matching query with distant change outside mask hits
        let mut p2 = p;
        p2.black |= 1u128 << 70;
        let hit = cache.lookup(h, &p2, MKey::new(2, 0, 0, 0, 0, 0, 0), MKey::EMPTY, 10);
        assert!(hit.is_some());
        // Different tag misses without hitting bucket
        assert!(cache
            .lookup(
                h ^ (0x55u64 << 32),
                &p2,
                MKey::new(2, 0, 0, 0, 0, 0, 0),
                MKey::EMPTY,
                10
            )
            .is_none());
        // Change inside mask misses
        let mut p3 = p;
        p3.black |= 1u128 << 5;
        assert!(cache
            .lookup(h, &p3, MKey::new(2, 0, 0, 0, 0, 0, 0), MKey::EMPTY, 10)
            .is_none());
        // Regression test: tag8 == 1 must NEVER bleed into empty (wt == 0) ways!
        cache.clear();
        let h_tag1 = 0x0000_0001_0000_0010u64; // ring_tag8 == 1
        cache.insert(h_tag1, &p, &fp, 10);
        assert!(cache
            .lookup(h_tag1, &p3, MKey::EMPTY, MKey::EMPTY, 10)
            .is_none());
        // Clear resets tag filter in O(1)
        cache.clear();
        assert!(cache
            .lookup(h_tag1, &p, MKey::new(2, 0, 0, 0, 0, 0, 0), MKey::EMPTY, 10)
            .is_none());
    }

    #[test]
    fn test_ni_soundness_mate_and_nomate() {
        let cases: &[(&str, bool)] = &[
            ("3sks3/9/4S4/9/9/B8/9/9/9 b S2rb4g4n4l18p 1", true),
            ("4k4/9/PPPPPPPPP/9/9/9/9/9/9 b B4L2rb4g4s4n9p 1", true),
            ("n+B1sS4/1R1g5/1Ls6/2k6/2n6/3L5/R8/9/9 b B2P3gs2n2l16p 1", true),
            ("4k4/9/4G4/9/9/9/9/9/9 b P2r2b3g4s4n4l17p 1", false),
            ("3sks3/9/4S4/9/9/B8/9/9/9 b 2rb4g4s4n4l18p 1", false),
            ("4k4/9/4P4/9/9/9/9/9/9 b 2r2b4g4s4n4l17p 1", false),
        ];
        for &(sfen, expected) in cases {
            let root = FmrsPos::from_sfen(sfen).unwrap();
            for &use_ni_cache in &[false, true] {
                let mut s = Solver::new(SearchConfig {
                    hash_mb: 32,
                    use_ni_cache,
                    node_limit: Some(200_000),
                    ..Default::default()
                });
                let r = s.solve(&root);
                assert_eq!(r.mate, Some(expected), "sfen={} ni={}", sfen, use_ni_cache);
            }
        }
    }

    #[test]
    fn test_shared_ni_cache_concurrent() {
        let shared = SharedNiCache::new(64, 1024);
        let base_hash = 0x7788_99aa_0000_0005u64;
        // Fill all 16 ways of the same bucket + same King-ring hash concurrently across 8 threads
        std::thread::scope(|s| {
            for t in 0..8usize {
                let cache = &shared;
                s.spawn(move || {
                    for k in 0..32usize {
                        let sq = ((t * 4 + k) % 45) + 18;
                        let p = BoardPlanes {
                            black: (1u128 << 4) | (1u128 << sq),
                            white: 1u128 << 13,
                            k0: 1u128 << sq,
                            ..Default::default()
                        };
                        let fp = NiFootprint {
                            valid: true,
                            mask: (1u128 << 4) | (1u128 << 13) | (1u128 << sq),
                            att_min: MKey::new(1, 0, 0, 0, 0, 0, 0),
                            def_max: MAX_HAND,
                            sh: 5 + (k as u16 % 7),
                            hinc: false,
                            nouse: 0,
                        };
                        cache.record_meta(base_hash ^ (sq as u64), &fp, 12 + k as u16);
                        cache.insert(base_hash, &p, &fp, (12 + k) as u64);
                        if cache.has_tag_match(base_hash) {
                            if let Some((hit, hw)) = cache.lookup_hit(
                                base_hash,
                                &p,
                                MKey::new(2, 0, 0, 0, 0, 0, 0),
                                MKey::EMPTY,
                                30,
                                false,
                            ) {
                                assert_eq!(hit.tdata.pn, 0);
                                assert!(hw >= 12);
                                assert!(p.eq_on_mask(&p, hit.mask));
                            }
                        }
                        // Mismatched piece inside mask must NEVER hit!
                        let mut p_bad = p;
                        p_bad.white |= 1u128 << sq;
                        assert!(cache
                            .lookup_hit(
                                base_hash,
                                &p_bad,
                                MKey::new(2, 0, 0, 0, 0, 0, 0),
                                MKey::EMPTY,
                                30,
                                false,
                            )
                            .is_none());
                    }
                });
            }
        });
        shared.clear();
        assert!(!shared.has_tag_match(base_hash));
    }

    #[test]
    fn test_white_slider_present_mask_1000_random_positions() {
        use crate::futsuu::types::{GKA, GKY, GRY, GUM, GHI};

        // Deterministic xorshift64 PRNG across 1,200 random boards covering every piece kind/color
        let mut rng = 0xdead_beef_cafe_babe_u64;
        let mut next_u64 = || {
            rng ^= rng << 13;
            rng ^= rng >> 7;
            rng ^= rng << 17;
            rng
        };

        let piece_tokens = [
            "P", "L", "N", "S", "G", "B", "R", "+P", "+L", "+N", "+S", "+B", "+R",
            "p", "l", "n", "s", "g", "b", "r", "+p", "+l", "+n", "+s", "+b", "+r",
        ];

        for trial in 0..1200usize {
            let mut grid = [[""; 9]; 9];
            let wk_r = (next_u64() % 9) as usize;
            let wk_f = (next_u64() % 9) as usize;
            grid[wk_r][wk_f] = "k";

            let n_pieces = (trial % 24) + 1;
            for _ in 0..n_pieces {
                let r = (next_u64() % 9) as usize;
                let f = (next_u64() % 9) as usize;
                if grid[r][f].is_empty() {
                    let tok = piece_tokens[(next_u64() as usize) % piece_tokens.len()];
                    // Avoid Pawn/Lance on dead rank just in case sfen parser checks
                    if (tok == "P" || tok == "L") && r == 0 {
                        continue;
                    }
                    if tok == "N" && r <= 1 {
                        continue;
                    }
                    if (tok == "p" || tok == "l") && r == 8 {
                        continue;
                    }
                    if tok == "n" && r >= 7 {
                        continue;
                    }
                    grid[r][f] = tok;
                }
            }

            let mut sfen = String::new();
            for r in 0..9 {
                if r > 0 {
                    sfen.push('/');
                }
                let mut empty = 0usize;
                for f in 0..9 {
                    if grid[r][f].is_empty() {
                        empty += 1;
                    } else {
                        if empty > 0 {
                            sfen.push_str(&empty.to_string());
                            empty = 0;
                        }
                        sfen.push_str(grid[r][f]);
                    }
                }
                if empty > 0 {
                    sfen.push_str(&empty.to_string());
                }
            }
            sfen.push_str(" b - 1");

            let pos = FmrsPos::from_sfen(&sfen).expect(&sfen);
            let planes = pos.ni_planes();
            let got = planes.white_slider_present_mask();

            // Direct square-by-square reference computation via PositionBackend::board_at
            let mut want = 0u16;
            let mut bish_cnt = 0usize;
            let mut rook_cnt = 0usize;
            let mut lance_col_cnt = [0usize; 9];
            for rank in 0..9usize {
                for file in 0..9usize {
                    let sq = rank * 9 + file; // shtsume sq; fmrs col == file
                    let koma = pos.board_at(sq);
                    if koma == GKA || koma == GUM {
                        want |= REQ_NO_W_BISH;
                        bish_cnt += 1;
                    } else if koma == GHI || koma == GRY {
                        want |= REQ_NO_W_ROOK;
                        rook_cnt += 1;
                    } else if koma == GKY {
                        want |= req_no_w_lance_col(file);
                        lance_col_cnt[file] += 1;
                    }
                }
            }
            if bish_cnt >= 2 {
                want |= REQ_AT_MOST_1_W_BISH;
            }
            if rook_cnt >= 2 {
                want |= REQ_AT_MOST_1_W_ROOK;
            }
            if lance_col_cnt.iter().any(|&c| c >= 2) {
                want |= REQ_AT_MOST_1_W_LANCE_PER_COL;
            }
            assert_eq!(got, want, "trial={} sfen={}", trial, sfen);
        }
    }

    #[test]
    fn test_meta_4way_multi_mkey_and_capture_boundary_soundness() {
        // 1. Verify 4 incomparable MKeys on the SAME zkey coexist in NiCache, LocalMetaTable, and SharedNiCache
        let mut ni = NiCache::new(64, 1024);
        let mut lmeta = LocalMetaTable::new(1024, 1);
        let shared = SharedNiCache::new(64, 1024);
        let zk = 0xCAFE_BABE_1234_5678u64;
        let hands = [
            MKey::new(3, 0, 0, 0, 0, 0, 0),
            MKey::new(2, 1, 0, 0, 0, 0, 0),
            MKey::new(1, 2, 0, 0, 0, 0, 0),
            MKey::new(0, 3, 0, 0, 0, 0, 0),
        ];
        for (i, &mk) in hands.iter().enumerate() {
            let fp = NiFootprint {
                valid: true,
                mask: pack_ni_mask(1u128 << (i + 1), REQ_NO_W_BISH, 0),
                att_min: mk,
                def_max: MAX_HAND,
                sh: 3 + (i as u16) * 2,
                hinc: false,
                nouse: 0,
            };
            ni.record_meta(zk, &fp);
            lmeta.record(zk, &fp, 10 + i as u16);
            shared.record_meta(zk, &fp, 10 + i as u16);
        }
        for (i, &mk) in hands.iter().enumerate() {
            let f1 = ni.lookup_meta(zk, mk, MKey::EMPTY).expect("NiCache 4-way");
            assert_eq!(f1.sh, 3 + (i as u16) * 2);
            assert_eq!(f1.req_absent(), REQ_NO_W_BISH);
            let (f2, _) = lmeta.lookup(zk, mk, MKey::EMPTY).expect("LocalMeta 4-way");
            assert_eq!(f2.sh, 3 + (i as u16) * 2);
            let (f3, _) = shared
                .lookup_meta(zk, mk, MKey::EMPTY)
                .expect("SharedMeta 4-way");
            assert_eq!(f3.sh, 3 + (i as u16) * 2);
        }

        // 2. Capture-transition boundary: if node_pm overlaps req_absent, record_meta & insert MUST drop fp!
        let bad_fp = NiFootprint {
            valid: true,
            mask: pack_ni_mask(0x1f, REQ_NO_W_BISH | REQ_NO_W_ROOK, REQ_NO_W_BISH),
            att_min: MKey::EMPTY,
            def_max: MAX_HAND,
            sh: 5,
            hinc: false,
            nouse: 0,
        };
        let zk_cap = 0x9999_8888_7777_6666u64;
        ni.record_meta(zk_cap, &bad_fp);
        lmeta.record(zk_cap, &bad_fp, 20);
        shared.record_meta(zk_cap, &bad_fp, 20);
        assert!(ni.lookup_meta(zk_cap, MAX_HAND, MKey::EMPTY).is_none());
        assert!(lmeta.lookup(zk_cap, MAX_HAND, MKey::EMPTY).is_none());
        assert!(shared.lookup_meta(zk_cap, MAX_HAND, MKey::EMPTY).is_none());

        // 3. Adversarial extra-White-slider query test on NiCache & SharedNiCache:
        //    Suppose P has 0 White sliders (`req_absent = REQ_NO_W_BISH | REQ_NO_W_ROOK | req_no_w_lance_col(4)`).
        //    Any query Q that adds a White Bishop, Horse, Rook, Dragon, or Lance in col 4 outside sq_mask MUST MISS!
        let p_clean = BoardPlanes {
            black: 1u128 << 40,
            white: 1u128 << 36, // White King at (4, 0) -> k0=1, k2=1, prom=1
            prom: 1u128 << 36,
            k0: 1u128 << 36,
            k2: 1u128 << 36,
            k1: 0,
        };
        assert_eq!(p_clean.white_slider_present_mask(), 0);
        let fp_clean = NiFootprint {
            valid: true,
            mask: pack_ni_mask(
                (1u128 << 40) | (1u128 << 36),
                REQ_NO_W_BISH | REQ_NO_W_ROOK | req_no_w_lance_col(4),
                0,
            ),
            att_min: MKey::EMPTY,
            def_max: MAX_HAND,
            sh: 5,
            hinc: false,
            nouse: 0,
        };
        let rhash = 0x4242_4242_0000_0007u64;
        ni.insert(rhash, &p_clean, &fp_clean, 25);
        shared.insert(rhash, &p_clean, &fp_clean, 25);

        // Clean query with an extra White Gold at sq 70 (outside mask, non-slider) HITS!
        let mut q_gold = p_clean;
        q_gold.white |= 1u128 << 70;
        q_gold.k0 |= 1u128 << 70;
        q_gold.k2 |= 1u128 << 70;
        assert!(ni.lookup(rhash, &q_gold, MKey::EMPTY, MKey::EMPTY, 20).is_some());
        assert!(shared
            .lookup_hit(rhash, &q_gold, MKey::EMPTY, MKey::EMPTY, 20, false)
            .is_some());

        // Adversarial queries with distant White Bishop, ProBishop, Rook, ProRook, or col-4 Lance MUST MISS!
        // (a) White Bishop at sq 80 (k1=1, k2=1, k0=0, prom=0)
        let mut q_bish = p_clean;
        q_bish.white |= 1u128 << 80;
        q_bish.k1 |= 1u128 << 80;
        q_bish.k2 |= 1u128 << 80;
        assert!(ni.lookup(rhash, &q_bish, MKey::EMPTY, MKey::EMPTY, 20).is_none());
        assert!(shared
            .lookup_hit(rhash, &q_bish, MKey::EMPTY, MKey::EMPTY, 20, false)
            .is_none());

        // (b) White ProBishop (Horse) at sq 80
        let mut q_horse = q_bish;
        q_horse.prom |= 1u128 << 80;
        assert!(ni.lookup(rhash, &q_horse, MKey::EMPTY, MKey::EMPTY, 20).is_none());
        assert!(shared
            .lookup_hit(rhash, &q_horse, MKey::EMPTY, MKey::EMPTY, 20, false)
            .is_none());

        // (c) White Rook at sq 80 (k0=1, k1=1, k2=1)
        let mut q_rook = q_bish;
        q_rook.k0 |= 1u128 << 80;
        assert!(ni.lookup(rhash, &q_rook, MKey::EMPTY, MKey::EMPTY, 20).is_none());
        assert!(shared
            .lookup_hit(rhash, &q_rook, MKey::EMPTY, MKey::EMPTY, 20, false)
            .is_none());

        // (d) White Lance in col 4 at sq 44 (col 4, row 8 -> 4*9 + 8 = 44; k1=1, k0=0, k2=0, prom=0)
        let mut q_lance4 = p_clean;
        q_lance4.white |= 1u128 << 44;
        q_lance4.k1 |= 1u128 << 44;
        assert!(ni.lookup(rhash, &q_lance4, MKey::EMPTY, MKey::EMPTY, 20).is_none());
        assert!(shared
            .lookup_hit(rhash, &q_lance4, MKey::EMPTY, MKey::EMPTY, 20, false)
            .is_none());

        // (e) White Lance in col 8 at sq 75 (not col 4!) IS allowed and HITS!
        let mut q_lance8 = p_clean;
        q_lance8.white |= 1u128 << 75;
        q_lance8.k1 |= 1u128 << 75;
        assert!(ni.lookup(rhash, &q_lance8, MKey::EMPTY, MKey::EMPTY, 20).is_some());
        assert!(shared
            .lookup_hit(rhash, &q_lance8, MKey::EMPTY, MKey::EMPTY, 20, false)
            .is_some());

        // 4. End-to-end solver adversarial test: position where P mates ONLY after capturing White's
        //    interposing/defending Bishop/Rook/Lance, tested with extra White sliders added on board/hand!
        let adv_sfens = [
            // Mate in 3 where Black captures a White Bishop on 2b, vs adding a 2nd distant White Bishop on 9i!
            ("7nk/7b1/7G1/9/9/9/9/9/9 b G 1", true),
            ("7nk/7b1/7G1/9/9/9/9/9/b8 b G 1", false), // 9i Bishop defends 2b (sq 10 = 2b? wait: 1a=0, 2b=col1,row1, 9i=col8,row8 -> diagonal!)
            // Distant White Rook on 9b defending 2b!
            ("7nk/b6p1/7G1/9/9/9/9/9/9 b G 1", true),
            ("7nk/r6p1/7G1/9/9/9/9/9/9 b G 1", false),
            // Distant White Lance on 2a defending 2c!
            ("8k/9/7P1/9/9/9/9/9/9 b 2G 1", true),
            ("7lk/9/7P1/9/9/9/9/9/9 b 2G 1", false),
        ];
        // Run both sequentially in the SAME Solver (even without clear? Solver::solve clears, so also test
        // positions where capture happens mid-tree inside a single solve!).
        for &(sfen, _hint) in &adv_sfens {
            let root = FmrsPos::from_sfen(sfen).unwrap();
            let mut s_off = Solver::new(SearchConfig {
                hash_mb: 32,
                threads: 1,
                use_ni_cache: false,
                node_limit: Some(200_000),
                ..Default::default()
            });
            let mut s_on = Solver::new(SearchConfig {
                hash_mb: 32,
                threads: 1,
                use_ni_cache: true,
                node_limit: Some(200_000),
                ..Default::default()
            });
            let mut s_par = Solver::new(SearchConfig {
                hash_mb: 64,
                threads: 4,
                use_ni_cache: true,
                node_limit: Some(200_000),
                ..Default::default()
            });
            let r_off = s_off.solve(&root);
            let r_on = s_on.solve(&root);
            let r_par = s_par.solve(&root);
            assert_eq!(r_on.mate, r_off.mate, "1T mismatch on {}", sfen);
            assert_eq!(r_par.mate, r_off.mate, "4T mismatch on {}", sfen);
            if r_off.mate == Some(true) {
                assert_eq!(r_on.mate_steps, r_off.mate_steps, "steps mismatch on {}", sfen);
            }
        }
    }
}
