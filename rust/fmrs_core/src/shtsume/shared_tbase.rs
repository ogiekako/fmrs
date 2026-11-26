//! Lock-free optimistic seqlock + 32-byte inline-bucket concurrent transposition table (`SharedTBase`).
//!
//! Design highlights:
//! - 8,192 64-byte-aligned shards (`N_SHARDS`), preserving 100% exact `MKey` hand dominance per `zkey`.
//! - Readers perform ZERO atomic writes (`Ordering::Relaxed` loads validated by an `Acquire` seqlock
//!   sequence check), keeping hot spine shard headers in Shared (`S`) L1/L2 state across all CCDs.
//! - Each hash bucket directly embeds a 32-byte-aligned `InlineZEntry` (`zkey` + inline head `MCard`),
//!   eliminating 2 dependent DRAM pointer chases (`table -> zpool -> mpool`) on ~75%+ of lookups and
//!   allowing a single `_mm_prefetch` to bring both `zkey` and `head_tdata` into L1.
//! - All shard pools (`buckets`, `zover`, `mpool`, `tpool`) have fixed capacity allocated once at
//!   construction time, guaranteeing stable addresses for concurrent lock-free readers with zero UB.

use std::cell::UnsafeCell;
use std::sync::atomic::{fence, AtomicBool, AtomicU16, AtomicU32, AtomicU64, Ordering};

use smallvec::SmallVec;

use crate::shtsume::types::{
    KeyState, MKey, MKeyCmp, MvItem, TData, Turn, ALL_DEPTH, GC_DELETE_OFFSET, GC_DELETE_RATE,
    GC_TSUMI_RATE, INFINATE,
};

const NONE16: u16 = u16::MAX;

const CUR_SH_BIT: u16 = 0x8000;
const SH_VAL_MASK: u16 = 0x7FFF;
const NOUSE_MASK: u8 = 0x3F;
const MC_HINC: u8 = 0x40;
const MC_PROTECT: u8 = 0x80;

pub(crate) const LOCAL_TT_BITS: usize = 12;
pub(crate) const LOCAL_TT_SIZE: usize = 1 << LOCAL_TT_BITS;
const LOCAL_TT_MASK: usize = LOCAL_TT_SIZE - 1;

const SOLVED_WORDS: usize = 4096;
const SOLVED_BIT_MASK: usize = (SOLVED_WORDS * 64) - 1;

#[inline(always)]
fn norm_zkey(zkey: u64) -> u64 {
    zkey | u64::from(zkey == 0)
}

#[inline(always)]
fn pack_w1(mkey: MKey, nouse: u8, hinc: bool, protect: bool) -> u64 {
    debug_assert_eq!(mkey.0 & 0xFF, 0);
    let meta = (nouse.min(NOUSE_MASK) & NOUSE_MASK)
        | if hinc { MC_HINC } else { 0 }
        | if protect { MC_PROTECT } else { 0 };
    mkey.0 | (meta as u64)
}

#[inline(always)]
fn unpack_w1(w1: u64) -> (MKey, u8, bool, bool) {
    let meta = (w1 & 0xFF) as u8;
    (
        MKey(w1 & !0xFF),
        meta & NOUSE_MASK,
        (meta & MC_HINC) != 0,
        (meta & MC_PROTECT) != 0,
    )
}

#[inline(always)]
fn pack_w2(td: TData, dp: i16, cur: bool) -> u64 {
    let sh_f = (td.sh & SH_VAL_MASK) | if cur { CUR_SH_BIT } else { 0 };
    (td.pn as u64)
        | ((td.dn as u64) << 16)
        | ((sh_f as u64) << 32)
        | (((dp as u16) as u64) << 48)
}

#[inline(always)]
fn unpack_w2(w2: u64) -> (TData, i16, bool) {
    let pn = (w2 & 0xFFFF) as u16;
    let dn = ((w2 >> 16) & 0xFFFF) as u16;
    let sh_f = ((w2 >> 32) & 0xFFFF) as u16;
    let dp = ((w2 >> 48) & 0xFFFF) as i16;
    (
        TData {
            pn,
            dn,
            sh: sh_f & SH_VAL_MASK,
        },
        dp,
        (sh_f & CUR_SH_BIT) != 0,
    )
}

#[inline(always)]
fn pack_zw3(next_zf: u16, overflow_mc: u16, tlist_tail: u16, cpn: u16) -> u64 {
    (next_zf as u64)
        | ((overflow_mc as u64) << 16)
        | ((tlist_tail as u64) << 32)
        | ((cpn as u64) << 48)
}

#[inline(always)]
fn unpack_zw3(w3: u64) -> (u16, u16, u16, u16) {
    (
        (w3 & 0xFFFF) as u16,
        ((w3 >> 16) & 0xFFFF) as u16,
        ((w3 >> 32) & 0xFFFF) as u16,
        ((w3 >> 48) & 0xFFFF) as u16,
    )
}

#[inline(always)]
fn pack_mw2(next_mc: u16, tlist_tail: u16, cpn: u16) -> u64 {
    (next_mc as u64) | ((tlist_tail as u64) << 16) | ((cpn as u64) << 32)
}

#[inline(always)]
fn unpack_mw2(w2: u64) -> (u16, u16, u16) {
    (
        (w2 & 0xFFFF) as u16,
        ((w2 >> 16) & 0xFFFF) as u16,
        ((w2 >> 32) & 0xFFFF) as u16,
    )
}

/// 32-byte aligned inline `ZFolder + head MCard` slot (exactly half a cache line, never straddles).
///
/// All-zero bytes (`w0 == 0`) represent a valid empty slot: readers never inspect `w1..w3`
/// unless `w0 == target_zkey` (`!= 0`), and writers initialize `w1..w3` (with `NONE16` links)
/// before publishing `w0 != 0`.
#[repr(C, align(32))]
struct InlineZEntry {
    w0: AtomicU64, // zkey (0 == empty)
    w1: AtomicU64, // mkey | meta(nouse, hinc, protect)
    w2: AtomicU64, // head_tdata(pn, dn, sh|cur) | head_dp
    w3: AtomicU64, // next_zf | overflow_mc | tlist_tail | cpn
}

/// 24-byte overflow `MCard` slot for positions with >= 2 distinct `MKey`s on the same `zkey`.
///
/// Slots in the zeroed arena are initialized across all fields (`w0..w2`) by `alloc_mcard_slot`
/// before their index is ever published into any `overflow_mc` / `next_mc` chain.
#[repr(C)]
struct MCardSlot {
    w0: AtomicU64, // mkey | meta
    w1: AtomicU64, // head_tdata | head_dp
    w2: AtomicU64, // next_mc | tlist_tail | cpn
}

/// 16-byte depth-list overflow node for unknown `MCard`s visited at multiple depths.
///
/// Slots in the zeroed arena are initialized across both fields (`w0, w1`) by `alloc_tlist`
/// before their index is ever published into any `tlist_tail` / `next_tl` chain.
#[repr(C)]
struct TListSlot {
    w0: AtomicU64, // tdata | dp
    w1: AtomicU32, // next_tl (NONE16 = end)
}

const _: () = assert!(std::mem::size_of::<InlineZEntry>() == 32);
const _: () = assert!(std::mem::align_of::<InlineZEntry>() == 32);
const _: () = assert!(std::mem::size_of::<MCardSlot>() == 24);
const _: () = assert!(std::mem::align_of::<MCardSlot>() == 8);
const _: () = assert!(std::mem::size_of::<TListSlot>() == 16);
const _: () = assert!(std::mem::align_of::<TListSlot>() == 8);

const HUGEPAGE_ALIGN: usize = 2 * 1024 * 1024;

/// Single contiguous 2 MiB-aligned OS-zeroed backing arena shared across all `N_SHARDS` shards
/// in SoA region order (`MADV_HUGEPAGE` on Linux; untouched 2 MiB pages stay unmapped):
/// - Block 0 (2 MiB-aligned): `[InlineZEntry; N_SHARDS * buckets_len]`
/// - Block 1: `[InlineZEntry; N_SHARDS * zover_cap]`
/// - Block 2: `[MCardSlot; N_SHARDS * mpool_cap]`
/// - Block 3: `[TListSlot; N_SHARDS * tpool_cap]`
struct ArenaBuffer {
    raw_ptr: std::ptr::NonNull<u8>,
    layout: std::alloc::Layout,
}

unsafe impl Send for ArenaBuffer {}
unsafe impl Sync for ArenaBuffer {}

impl ArenaBuffer {
    fn new_zeroed_2mb_aligned(total_bytes: usize, hugepage_prefix_bytes: usize) -> (Self, *mut u8) {
        // Use `align = 8` (`<= MIN_ALIGN` on all 32/64-bit targets) plus `HUGEPAGE_ALIGN` padding
        // so `std::alloc::alloc_zeroed` dispatches directly to `calloc` / lazy zero-mapped `mmap`
        // in microseconds without falling back to `posix_memalign` + single-threaded `memset`.
        let alloc_bytes = total_bytes
            .checked_add(HUGEPAGE_ALIGN)
            .expect("SharedTBase arena size overflow");
        let layout = std::alloc::Layout::from_size_align(alloc_bytes, 8)
            .expect("invalid SharedTBase arena layout");
        let raw = unsafe { std::alloc::alloc_zeroed(layout) };
        let Some(raw_ptr) = std::ptr::NonNull::new(raw) else {
            std::alloc::handle_alloc_error(layout);
        };
        let addr = raw as usize;
        let aligned_addr = (addr + (HUGEPAGE_ALIGN - 1)) & !(HUGEPAGE_ALIGN - 1);
        let base_ptr = unsafe { raw.add(aligned_addr - addr) };
        debug_assert_eq!((base_ptr as usize) % HUGEPAGE_ALIGN, 0);

        #[cfg(target_os = "linux")]
        if hugepage_prefix_bytes > 0 {
            debug_assert_eq!(hugepage_prefix_bytes % HUGEPAGE_ALIGN, 0);
            unsafe extern "C" {
                fn madvise(addr: *mut core::ffi::c_void, len: usize, advice: i32) -> i32;
            }
            const MADV_HUGEPAGE: i32 = 14;
            unsafe {
                let _ = madvise(
                    base_ptr as *mut core::ffi::c_void,
                    hugepage_prefix_bytes,
                    MADV_HUGEPAGE,
                );
            }
        }
        #[cfg(not(target_os = "linux"))]
        let _ = hugepage_prefix_bytes;

        (Self { raw_ptr, layout }, base_ptr)
    }
}

impl Drop for ArenaBuffer {
    fn drop(&mut self) {
        unsafe {
            std::alloc::dealloc(self.raw_ptr.as_ptr(), self.layout);
        }
    }
}

#[derive(Copy, Clone)]
struct CardSnap {
    mkey: MKey,
    nouse: u8,
    hinc: bool,
    protect: bool,
    head_tdata: TData,
    head_dp: i16,
    cur: bool,
    tlist_tail: u16,
    cpn: u16,
}

#[derive(Copy, Clone, PartialEq, Eq)]
enum ZLoc {
    Bucket(u16),
    Over(u16),
}

struct ShardWriterMeta {
    num: usize,
    used_buckets: Vec<u16>,
    prot_buckets: Vec<u16>,
    cur_counts: Vec<(u64, MKey, u16)>,
    gc_max_level: u16,
    zlen: u16,
    mlen: u16,
    tlen: u16,
    zfree: u16,
    mfree: u16,
    tfree: u16,
    zfree_cnt: u16,
    mfree_cnt: u16,
    tfree_cnt: u16,
}

impl ShardWriterMeta {
    #[inline(always)]
    fn new() -> Self {
        Self {
            num: 0,
            used_buckets: Vec::new(),
            prot_buckets: Vec::new(),
            cur_counts: Vec::new(),
            gc_max_level: 0,
            zlen: 0,
            mlen: 0,
            tlen: 0,
            zfree: NONE16,
            mfree: NONE16,
            tfree: NONE16,
            zfree_cnt: 0,
            mfree_cnt: 0,
            tfree_cnt: 0,
        }
    }

    #[inline(always)]
    fn record_prot_bucket(&mut self, b: u16) {
        if self.prot_buckets.len() < 32 {
            if !self.prot_buckets.contains(&b) {
                self.prot_buckets.push(b);
            }
        } else if self.prot_buckets.last() != Some(&b) {
            self.prot_buckets.push(b);
        }
    }
}

/// Per-shard header: Cache Line 0 (`0..64`) packs all lock-free reader fields + non-owning arena
/// pointers/capacities; Cache Lines 1..2 (`64..192`) isolate writer-only `ShardWriterMeta`.
#[repr(C, align(64))]
struct ShardSlot {
    seq: AtomicU64,
    write_lock: AtomicBool,
    dirty: AtomicBool,
    has_prot: AtomicBool,
    _pad0: u8,
    sz_elm: u32,
    mask: u64,
    buckets_ptr: *const InlineZEntry,
    zover_ptr: *const InlineZEntry,
    mpool_ptr: *const MCardSlot,
    tpool_ptr: *const TListSlot,
    zover_cap: u16,
    mpool_cap: u16,
    tpool_cap: u16,
    _pad1: u16,
    meta: UnsafeCell<ShardWriterMeta>,
}

#[cfg(target_pointer_width = "64")]
const _: () = assert!(std::mem::offset_of!(ShardSlot, meta) == 64);

unsafe impl Send for ShardSlot {}
unsafe impl Sync for ShardSlot {}

struct WriteGuard<'a> {
    slot: &'a ShardSlot,
    seq_start: u64,
}

impl<'a> WriteGuard<'a> {
    #[inline(always)]
    fn meta(&mut self) -> &mut ShardWriterMeta {
        unsafe { &mut *self.slot.meta.get() }
    }
}

impl<'a> Drop for WriteGuard<'a> {
    #[inline(always)]
    fn drop(&mut self) {
        self.slot
            .seq
            .store(self.seq_start.wrapping_add(2), Ordering::Release);
        self.slot.write_lock.store(false, Ordering::Release);
    }
}

#[derive(Copy, Clone)]
enum LookupResult {
    Miss {
        pn: u16,
        dn: u16,
    },
    Hit {
        tdata: TData,
        mkey: MKey,
        search: bool,
        hinc: bool,
        inc: bool,
        nouse: u8,
        nouse2: u8,
        cu: bool,
        pr: bool,
        set_flag_bits: bool,
    },
}

impl LookupResult {
    #[inline(always)]
    fn apply(self, mv: &mut MvItem) {
        match self {
            LookupResult::Miss { pn, dn } => {
                mv.tdata = TData { pn, dn, sh: 0 };
            }
            LookupResult::Hit {
                tdata,
                mkey,
                search,
                hinc,
                inc,
                nouse,
                nouse2,
                cu,
                pr,
                set_flag_bits,
            } => {
                mv.tdata = tdata;
                if mkey.0 != 0 || tdata.pn == 0 || tdata.dn == 0 {
                    mv.mkey = mkey;
                }
                if search {
                    mv.set_search(true);
                }
                if tdata.pn == 0 {
                    mv.set_hinc(hinc);
                    mv.set_inc(inc);
                    mv.nouse = nouse;
                    mv.nouse2 = nouse2;
                    if set_flag_bits {
                        mv.set_cu(cu);
                        mv.set_pr(pr);
                    }
                } else if tdata.dn == 0 {
                    mv.set_hinc(false);
                    mv.set_inc(false);
                }
            }
        }
    }
}

impl ShardSlot {
    #[allow(clippy::too_many_arguments)]
    #[inline(always)]
    fn from_arena(
        buckets_ptr: *const InlineZEntry,
        buckets_len: usize,
        zover_ptr: *const InlineZEntry,
        zover_cap: u16,
        mpool_ptr: *const MCardSlot,
        mpool_cap: u16,
        tpool_ptr: *const TListSlot,
        tpool_cap: u16,
        sz_elm: u32,
    ) -> Self {
        Self {
            seq: AtomicU64::new(0),
            write_lock: AtomicBool::new(false),
            dirty: AtomicBool::new(false),
            has_prot: AtomicBool::new(false),
            _pad0: 0,
            sz_elm,
            mask: (buckets_len as u64) - 1,
            buckets_ptr,
            zover_ptr,
            mpool_ptr,
            tpool_ptr,
            zover_cap,
            mpool_cap,
            tpool_cap,
            _pad1: 0,
            meta: UnsafeCell::new(ShardWriterMeta::new()),
        }
    }

    #[inline(always)]
    fn buckets(&self) -> &[InlineZEntry] {
        unsafe { std::slice::from_raw_parts(self.buckets_ptr, (self.mask as usize) + 1) }
    }

    #[inline(always)]
    fn zover(&self) -> &[InlineZEntry] {
        unsafe { std::slice::from_raw_parts(self.zover_ptr, self.zover_cap as usize) }
    }

    #[inline(always)]
    fn mpool(&self) -> &[MCardSlot] {
        unsafe { std::slice::from_raw_parts(self.mpool_ptr, self.mpool_cap as usize) }
    }

    #[inline(always)]
    fn tpool(&self) -> &[TListSlot] {
        unsafe { std::slice::from_raw_parts(self.tpool_ptr, self.tpool_cap as usize) }
    }

    #[inline(always)]
    fn lock_write(&self) -> WriteGuard<'_> {
        if self
            .write_lock
            .compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            self.lock_write_slow();
        }
        let s = self.seq.load(Ordering::Relaxed);
        self.seq.store(s.wrapping_add(1), Ordering::Relaxed);
        fence(Ordering::Release);
        WriteGuard {
            slot: self,
            seq_start: s,
        }
    }

    #[cold]
    fn lock_write_slow(&self) {
        let mut spins = 0u32;
        loop {
            while self.write_lock.load(Ordering::Relaxed) {
                spins += 1;
                if spins < 128 {
                    std::hint::spin_loop();
                } else {
                    std::thread::yield_now();
                    spins = 0;
                }
            }
            if self
                .write_lock
                .compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed)
                .is_ok()
            {
                return;
            }
        }
    }

    #[inline(always)]
    fn try_lock_write(&self) -> Option<WriteGuard<'_>> {
        if self
            .write_lock
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_ok()
        {
            let s = self.seq.load(Ordering::Relaxed);
            self.seq.store(s.wrapping_add(1), Ordering::Relaxed);
            fence(Ordering::Release);
            Some(WriteGuard {
                slot: self,
                seq_start: s,
            })
        } else {
            None
        }
    }

    #[inline(always)]
    fn read_seqlock<R>(&self, f: impl Fn(&Self) -> Option<R>) -> R {
        loop {
            let s1 = self.seq.load(Ordering::Acquire);
            if (s1 & 1) != 0 {
                std::hint::spin_loop();
                continue;
            }
            let res = f(self);
            fence(Ordering::Acquire);
            let s2 = self.seq.load(Ordering::Relaxed);
            if s1 == s2 {
                if let Some(v) = res {
                    return v;
                }
            }
            std::hint::spin_loop();
        }
    }

    #[inline(always)]
    fn zentry(&self, loc: ZLoc) -> &InlineZEntry {
        match loc {
            ZLoc::Bucket(i) => unsafe { &*self.buckets_ptr.add(i as usize) },
            ZLoc::Over(i) => unsafe { &*self.zover_ptr.add(i as usize) },
        }
    }

    #[inline(always)]
    fn load_inline_card(&self, ze: &InlineZEntry) -> (CardSnap, u16, u16) {
        let w1 = ze.w1.load(Ordering::Relaxed);
        let w2 = ze.w2.load(Ordering::Relaxed);
        let w3 = ze.w3.load(Ordering::Relaxed);
        let (mkey, nouse, hinc, protect) = unpack_w1(w1);
        let (head_tdata, head_dp, cur) = unpack_w2(w2);
        let (next_zf, overflow_mc, tlist_tail, cpn) = unpack_zw3(w3);
        (
            CardSnap {
                mkey,
                nouse,
                hinc,
                protect,
                head_tdata,
                head_dp,
                cur,
                tlist_tail,
                cpn,
            },
            next_zf,
            overflow_mc,
        )
    }

    #[inline(always)]
    fn load_pool_card(&self, idx: u16) -> Option<(CardSnap, u16)> {
        let slot = self.mpool().get(idx as usize)?;
        let w0 = slot.w0.load(Ordering::Relaxed);
        let w1 = slot.w1.load(Ordering::Relaxed);
        let w2 = slot.w2.load(Ordering::Relaxed);
        let (mkey, nouse, hinc, protect) = unpack_w1(w0);
        let (head_tdata, head_dp, cur) = unpack_w2(w1);
        let (next_mc, tlist_tail, cpn) = unpack_mw2(w2);
        Some((
            CardSnap {
                mkey,
                nouse,
                hinc,
                protect,
                head_tdata,
                head_dp,
                cur,
                tlist_tail,
                cpn,
            },
            next_mc,
        ))
    }

    #[inline(always)]
    fn store_inline_card(&self, ze: &InlineZEntry, zkey: u64, c: CardSnap, next_zf: u16, overflow_mc: u16) {
        ze.w1
            .store(pack_w1(c.mkey, c.nouse, c.hinc, c.protect), Ordering::Relaxed);
        ze.w2
            .store(pack_w2(c.head_tdata, c.head_dp, c.cur), Ordering::Relaxed);
        ze.w3.store(
            pack_zw3(next_zf, overflow_mc, c.tlist_tail, c.cpn),
            Ordering::Relaxed,
        );
        ze.w0.store(zkey, Ordering::Relaxed);
    }

    #[inline(always)]
    fn store_pool_card(&self, idx: u16, c: CardSnap, next_mc: u16) {
        let slot = unsafe { &*self.mpool_ptr.add(idx as usize) };
        slot.w0
            .store(pack_w1(c.mkey, c.nouse, c.hinc, c.protect), Ordering::Relaxed);
        slot.w1
            .store(pack_w2(c.head_tdata, c.head_dp, c.cur), Ordering::Relaxed);
        slot.w2
            .store(pack_mw2(next_mc, c.tlist_tail, c.cpn), Ordering::Relaxed);
    }

    #[inline(always)]
    fn find_zloc_opt(&self, zkey: u64) -> Option<Option<ZLoc>> {
        let b = (zkey & self.mask) as u16;
        let ze = unsafe { &*self.buckets_ptr.add(b as usize) };
        let bz = ze.w0.load(Ordering::Relaxed);
        if bz == 0 {
            return Some(None);
        }
        if bz == zkey {
            return Some(Some(ZLoc::Bucket(b)));
        }
        let mut next = (ze.w3.load(Ordering::Relaxed) & 0xFFFF) as u16;
        let max_z = self.zover_cap as usize;
        let zover = self.zover();
        let mut steps = 0usize;
        while next != NONE16 {
            steps += 1;
            if steps > max_z {
                return None;
            }
            let oz = zover.get(next as usize)?;
            if oz.w0.load(Ordering::Relaxed) == zkey {
                return Some(Some(ZLoc::Over(next)));
            }
            next = (oz.w3.load(Ordering::Relaxed) & 0xFFFF) as u16;
        }
        Some(None)
    }

    #[inline(always)]
    fn lookup_readonly_opt(&self, ks: &KeyState, tn: Turn, flag: bool) -> Option<LookupResult> {
        let zkey = norm_zkey(ks.zkey);
        let b = (zkey & self.mask) as u16;
        let b_ze = unsafe { &*self.buckets_ptr.add(b as usize) };
        let bz = b_ze.w0.load(Ordering::Relaxed);
        if bz == 0 {
            return Some(LookupResult::Miss { pn: 1, dn: 1 });
        }
        let mkey = ks.mkey_for(tn);
        let ze = if bz == zkey {
            b_ze
        } else {
            let mut next = (b_ze.w3.load(Ordering::Relaxed) & 0xFFFF) as u16;
            let mut found = None;
            let max_z = self.zover_cap as usize;
            let zover = self.zover();
            let mut steps = 0usize;
            while next != NONE16 {
                steps += 1;
                if steps > max_z {
                    return None;
                }
                let oz = zover.get(next as usize)?;
                if oz.w0.load(Ordering::Relaxed) == zkey {
                    found = Some(oz);
                    break;
                }
                next = (oz.w3.load(Ordering::Relaxed) & 0xFFFF) as u16;
            }
            match found {
                Some(z) => z,
                None => return Some(LookupResult::Miss { pn: 1, dn: 1 }),
            }
        };

        let (head_card, _, overflow_mc) = self.load_inline_card(ze);

        // Ultra-fast single-card path (~95% of all TT hits): zero loop, zero extra pool loads!
        if overflow_mc == NONE16 {
            return self.eval_single_card(head_card, mkey, ks.count as i16, flag);
        }

        let mut super_tsumi: Option<CardSnap> = None;
        let mut infer_fudumi: Option<CardSnap> = None;
        let mut equal_tsumi: Option<CardSnap> = None;
        let mut equal_fudumi: Option<CardSnap> = None;
        let mut equal_unknown: Option<CardSnap> = None;
        let mut tmp_pn: u16 = 1;
        let mut tmp_dn: u16 = 1;

        let mut cur_card = Some(head_card);
        let mut next_mc = overflow_mc;
        let mut steps = 0usize;
        let tpool = self.tpool();
        let max_t = tpool.len();
        let max_m = self.mpool_cap as usize;

        while let Some(mcard) = cur_card {
            let head_td = mcard.head_tdata;
            if mkey.0 == mcard.mkey.0 {
                if head_td.pn == 0 {
                    if equal_tsumi.is_none() {
                        equal_tsumi = Some(mcard);
                    }
                } else if head_td.dn == 0 {
                    if equal_fudumi.is_none() {
                        equal_fudumi = Some(mcard);
                    }
                    break;
                } else if equal_unknown.is_none() {
                    equal_unknown = Some(mcard);
                }
            } else if mkey.ge_all(mcard.mkey) {
                if head_td.pn == 0 {
                    super_tsumi = Some(mcard);
                    break;
                } else if head_td.dn != 0 {
                    if tmp_pn > head_td.pn {
                        tmp_pn = head_td.pn;
                    }
                    let mut tl = mcard.tlist_tail;
                    let mut tsteps = 0usize;
                    while tl != NONE16 {
                        tsteps += 1;
                        if tsteps > max_t {
                            return None;
                        }
                        let ts = tpool.get(tl as usize)?;
                        let (ntd, _, _) = unpack_w2(ts.w0.load(Ordering::Relaxed));
                        if tmp_pn > ntd.pn {
                            tmp_pn = ntd.pn;
                        }
                        tl = ts.w1.load(Ordering::Relaxed) as u16;
                    }
                }
            } else if mcard.mkey.ge_all(mkey) {
                if head_td.pn != 0 && head_td.dn == 0 {
                    infer_fudumi = Some(mcard);
                    break;
                } else if head_td.pn != 0 {
                    if head_td.pn > tmp_pn {
                        tmp_pn = head_td.pn;
                    }
                    let mut tl = mcard.tlist_tail;
                    let mut tsteps = 0usize;
                    while tl != NONE16 {
                        tsteps += 1;
                        if tsteps > max_t {
                            return None;
                        }
                        let ts = tpool.get(tl as usize)?;
                        let (ntd, _, _) = unpack_w2(ts.w0.load(Ordering::Relaxed));
                        if ntd.pn > tmp_pn {
                            tmp_pn = ntd.pn;
                        }
                        tl = ts.w1.load(Ordering::Relaxed) as u16;
                    }
                    if flag && mcard.cur && mcard.cpn > tmp_pn {
                        tmp_pn = mcard.cpn;
                    }
                }
            }

            if next_mc == NONE16 {
                break;
            }
            steps += 1;
            if steps > max_m {
                return None;
            }
            let (mc, nxt) = self.load_pool_card(next_mc)?;
            cur_card = Some(mc);
            next_mc = nxt;
        }

        if let Some(mcard) = super_tsumi {
            return Some(LookupResult::Hit {
                tdata: mcard.head_tdata,
                mkey: mcard.mkey,
                search: true,
                hinc: mcard.hinc,
                inc: true,
                nouse: mcard.nouse,
                nouse2: mkey
                    .total()
                    .saturating_sub(mcard.mkey.total())
                    .saturating_add(mcard.nouse),
                cu: false,
                pr: mcard.protect,
                set_flag_bits: flag,
            });
        }
        if let Some(mcard) = infer_fudumi {
            return Some(LookupResult::Hit {
                tdata: mcard.head_tdata,
                mkey: mcard.mkey,
                search: true,
                hinc: false,
                inc: false,
                nouse: 0,
                nouse2: 0,
                cu: false,
                pr: false,
                set_flag_bits: false,
            });
        }
        if let Some(mcard) = equal_tsumi {
            return Some(LookupResult::Hit {
                tdata: mcard.head_tdata,
                mkey: mcard.mkey,
                search: true,
                hinc: mcard.hinc,
                inc: mcard.hinc,
                nouse: mcard.nouse,
                nouse2: mcard.nouse,
                cu: mcard.cur,
                pr: mcard.protect,
                set_flag_bits: flag,
            });
        }
        if let Some(mcard) = equal_fudumi {
            return Some(LookupResult::Hit {
                tdata: mcard.head_tdata,
                mkey: mcard.mkey,
                search: true,
                hinc: false,
                inc: false,
                nouse: 0,
                nouse2: 0,
                cu: false,
                pr: false,
                set_flag_bits: false,
            });
        }
        if let Some(mcard) = equal_unknown {
            if flag && mcard.cur {
                return Some(LookupResult::Hit {
                    tdata: TData {
                        pn: 1u16.max(mcard.cpn),
                        dn: 1,
                        sh: 0,
                    },
                    mkey: MKey::EMPTY,
                    search: true,
                    hinc: false,
                    inc: false,
                    nouse: 0,
                    nouse2: 0,
                    cu: false,
                    pr: false,
                    set_flag_bits: false,
                });
            }
            let target_dp = ks.count as i16;
            let mut hit_td = TData::INIT;
            let mut hit = false;
            if mcard.head_dp == target_dp {
                hit_td = mcard.head_tdata;
                hit = true;
            }
            tmp_pn = tmp_pn.max(mcard.head_tdata.pn);
            tmp_dn = tmp_dn.max(mcard.head_tdata.dn);
            let mut tl = mcard.tlist_tail;
            let mut tsteps = 0usize;
            while tl != NONE16 {
                tsteps += 1;
                if tsteps > max_t {
                    return None;
                }
                let ts = tpool.get(tl as usize)?;
                let (ntd, ndp, _) = unpack_w2(ts.w0.load(Ordering::Relaxed));
                if ndp == target_dp {
                    hit_td = ntd;
                    hit = true;
                }
                tmp_pn = tmp_pn.max(ntd.pn);
                tmp_dn = tmp_dn.max(ntd.dn);
                tl = ts.w1.load(Ordering::Relaxed) as u16;
            }
            if hit {
                return Some(LookupResult::Hit {
                    tdata: hit_td,
                    mkey: MKey::EMPTY,
                    search: true,
                    hinc: false,
                    inc: false,
                    nouse: 0,
                    nouse2: 0,
                    cu: false,
                    pr: false,
                    set_flag_bits: false,
                });
            }
            return Some(LookupResult::Miss {
                pn: tmp_pn,
                dn: tmp_dn,
            });
        }
        Some(LookupResult::Miss {
            pn: tmp_pn,
            dn: tmp_dn,
        })
    }

    #[inline(always)]
    fn eval_single_card(
        &self,
        mcard: CardSnap,
        mkey: MKey,
        target_dp: i16,
        flag: bool,
    ) -> Option<LookupResult> {
        let head_td = mcard.head_tdata;
        if mkey.0 == mcard.mkey.0 {
            if head_td.pn == 0 {
                return Some(LookupResult::Hit {
                    tdata: head_td,
                    mkey: mcard.mkey,
                    search: true,
                    hinc: mcard.hinc,
                    inc: mcard.hinc,
                    nouse: mcard.nouse,
                    nouse2: mcard.nouse,
                    cu: mcard.cur,
                    pr: mcard.protect,
                    set_flag_bits: flag,
                });
            }
            if head_td.dn == 0 {
                return Some(LookupResult::Hit {
                    tdata: head_td,
                    mkey: mcard.mkey,
                    search: true,
                    hinc: false,
                    inc: false,
                    nouse: 0,
                    nouse2: 0,
                    cu: false,
                    pr: false,
                    set_flag_bits: false,
                });
            }
            if flag && mcard.cur {
                return Some(LookupResult::Hit {
                    tdata: TData {
                        pn: 1u16.max(mcard.cpn),
                        dn: 1,
                        sh: 0,
                    },
                    mkey: MKey::EMPTY,
                    search: true,
                    hinc: false,
                    inc: false,
                    nouse: 0,
                    nouse2: 0,
                    cu: false,
                    pr: false,
                    set_flag_bits: false,
                });
            }
            let mut hit_td = TData::INIT;
            let mut hit = false;
            if mcard.head_dp == target_dp {
                hit_td = head_td;
                hit = true;
            }
            let mut tmp_pn = 1u16.max(head_td.pn);
            let mut tmp_dn = 1u16.max(head_td.dn);
            let mut tl = mcard.tlist_tail;
            let tpool = self.tpool();
            let max_t = tpool.len();
            let mut tsteps = 0usize;
            while tl != NONE16 {
                tsteps += 1;
                if tsteps > max_t {
                    return None;
                }
                let ts = tpool.get(tl as usize)?;
                let (ntd, ndp, _) = unpack_w2(ts.w0.load(Ordering::Relaxed));
                if ndp == target_dp {
                    hit_td = ntd;
                    hit = true;
                }
                tmp_pn = tmp_pn.max(ntd.pn);
                tmp_dn = tmp_dn.max(ntd.dn);
                tl = ts.w1.load(Ordering::Relaxed) as u16;
            }
            if hit {
                return Some(LookupResult::Hit {
                    tdata: hit_td,
                    mkey: MKey::EMPTY,
                    search: true,
                    hinc: false,
                    inc: false,
                    nouse: 0,
                    nouse2: 0,
                    cu: false,
                    pr: false,
                    set_flag_bits: false,
                });
            }
            return Some(LookupResult::Miss {
                pn: tmp_pn,
                dn: tmp_dn,
            });
        }
        if mkey.ge_all(mcard.mkey) {
            if head_td.pn == 0 {
                return Some(LookupResult::Hit {
                    tdata: head_td,
                    mkey: mcard.mkey,
                    search: true,
                    hinc: mcard.hinc,
                    inc: true,
                    nouse: mcard.nouse,
                    nouse2: mkey
                        .total()
                        .saturating_sub(mcard.mkey.total())
                        .saturating_add(mcard.nouse),
                    cu: false,
                    pr: mcard.protect,
                    set_flag_bits: flag,
                });
            }
            return Some(LookupResult::Miss { pn: 1, dn: 1 });
        }
        if mcard.mkey.ge_all(mkey) {
            if head_td.pn != 0 && head_td.dn == 0 {
                return Some(LookupResult::Hit {
                    tdata: head_td,
                    mkey: mcard.mkey,
                    search: true,
                    hinc: false,
                    inc: false,
                    nouse: 0,
                    nouse2: 0,
                    cu: false,
                    pr: false,
                    set_flag_bits: false,
                });
            }
            if head_td.pn != 0 {
                let mut tmp_pn = 1u16.max(head_td.pn);
                let mut tl = mcard.tlist_tail;
                let tpool = self.tpool();
                let mut tsteps = 0usize;
                while tl != NONE16 {
                    tsteps += 1;
                    if tsteps > 16 {
                        return None;
                    }
                    let ts = tpool.get(tl as usize)?;
                    let (ntd, _, _) = unpack_w2(ts.w0.load(Ordering::Relaxed));
                    tmp_pn = tmp_pn.max(ntd.pn);
                    tl = ts.w1.load(Ordering::Relaxed) as u16;
                }
                if flag && mcard.cur && mcard.cpn > tmp_pn {
                    tmp_pn = mcard.cpn;
                }
                return Some(LookupResult::Miss { pn: tmp_pn, dn: 1 });
            }
        }
        Some(LookupResult::Miss { pn: 1, dn: 1 })
    }

    #[inline(always)]
    fn is_terminal_opt(&self, ks: &KeyState, tn: Turn) -> Option<bool> {
        let zkey = norm_zkey(ks.zkey);
        let zloc = match self.find_zloc_opt(zkey)? {
            Some(l) => l,
            None => return Some(false),
        };
        let mkey = ks.mkey_for(tn);
        let (head_card, _, mut next_mc) = self.load_inline_card(self.zentry(zloc));
        let mut cur_card = Some(head_card);
        let max_m = self.mpool_cap as usize;
        let mut steps = 0usize;
        while let Some(mcard) = cur_card {
            let td = mcard.head_tdata;
            if td.pn == 0 && mkey.ge_all(mcard.mkey) {
                return Some(true);
            }
            if td.pn != 0 && td.dn == 0 && mcard.mkey.ge_all(mkey) {
                return Some(true);
            }
            if next_mc == NONE16 {
                break;
            }
            steps += 1;
            if steps > max_m {
                return None;
            }
            let (mc, nxt) = self.load_pool_card(next_mc)?;
            cur_card = Some(mc);
            next_mc = nxt;
        }
        Some(false)
    }

    /// Lock-free check whether an inline or overflow `mpool` entry already stores the exact same
    /// `(mkey, target_dp, tdata)` with no terminal dominance change pending, making `fumei_update`
    /// a byte-for-byte no-op without acquiring `WriteGuard`.
    #[inline(always)]
    fn already_has_exact_unknown(&self, ks: &KeyState, td: TData, tn: Turn) -> bool {
        let s1 = self.seq.load(Ordering::Acquire);
        if (s1 & 1) != 0 {
            return false;
        }
        let zkey = norm_zkey(ks.zkey);
        let b = (zkey & self.mask) as usize;
        let b_ze = unsafe { &*self.buckets_ptr.add(b) };
        let bz = b_ze.w0.load(Ordering::Relaxed);
        let ze_opt = if bz == 0 {
            None
        } else if bz == zkey {
            Some(b_ze)
        } else {
            let zover = self.zover();
            let mut next = (b_ze.w3.load(Ordering::Relaxed) & 0xFFFF) as u16;
            let mut found = None;
            for _ in 0..2 {
                if next == NONE16 {
                    break;
                }
                let Some(oz) = zover.get(next as usize) else {
                    break;
                };
                if oz.w0.load(Ordering::Relaxed) == zkey {
                    found = Some(oz);
                    break;
                }
                next = (oz.w3.load(Ordering::Relaxed) & 0xFFFF) as u16;
            }
            found
        };
        let same = if let Some(ze) = ze_opt {
            let qmk = ks.mkey_for(tn);
            let target_dp = ks.count as i16;
            let (head, _, mut next_mc) = self.load_inline_card(ze);
            let htd = head.head_tdata;
            if head.mkey.0 == qmk.0 {
                !head.cur
                    && htd.pn != 0
                    && htd.dn != 0
                    && head.head_dp == target_dp
                    && htd == td
            } else if (htd.pn == 0 && qmk.ge_all(head.mkey))
                || (htd.pn != 0 && htd.dn == 0 && head.mkey.ge_all(qmk))
            {
                false
            } else {
                let mut matched = false;
                for _ in 0..4 {
                    if next_mc == NONE16 {
                        break;
                    }
                    let Some((mc, nxt)) = self.load_pool_card(next_mc) else {
                        break;
                    };
                    let mtd = mc.head_tdata;
                    if mc.mkey.0 == qmk.0 {
                        matched = !mc.cur
                            && mtd.pn != 0
                            && mtd.dn != 0
                            && mc.head_dp == target_dp
                            && mtd == td;
                        break;
                    }
                    if (mtd.pn == 0 && qmk.ge_all(mc.mkey))
                        || (mtd.pn != 0 && mtd.dn == 0 && mc.mkey.ge_all(qmk))
                    {
                        break;
                    }
                    next_mc = nxt;
                }
                matched
            }
        } else {
            false
        };
        fence(Ordering::Acquire);
        same && self.seq.load(Ordering::Relaxed) == s1
    }

    #[inline(always)]
    fn hs_lookup_opt(&self, ks: &KeyState, tn: Turn) -> Option<(i8, Option<(MKey, TData, bool)>)> {
        let zkey = norm_zkey(ks.zkey);
        let zloc = match self.find_zloc_opt(zkey)? {
            Some(l) => l,
            None => return Some((-1, None)),
        };
        let qmk = ks.mkey_for(tn);
        let (head_card, _, mut next_mc) = self.load_inline_card(self.zentry(zloc));
        let mut cur_card = Some(head_card);
        let max_m = self.mpool_cap as usize;
        let mut steps = 0usize;
        while let Some(mcard) = cur_card {
            let td = mcard.head_tdata;
            if qmk.ge_all(mcard.mkey) && td.pn == 0 {
                let can_cache = qmk.0 == mcard.mkey.0 && !mcard.hinc;
                return Some((1, Some((mcard.mkey, td, can_cache))));
            }
            if mcard.mkey.ge_all(qmk) && td.dn == 0 {
                return Some((0, Some((mcard.mkey, td, true))));
            }
            if qmk.0 == mcard.mkey.0 && td.pn != 0 {
                return Some((0, None));
            }
            if next_mc == NONE16 {
                break;
            }
            steps += 1;
            if steps > max_m {
                return None;
            }
            let (mc, nxt) = self.load_pool_card(next_mc)?;
            cur_card = Some(mc);
            next_mc = nxt;
        }
        Some((-1, None))
    }

    // ---- Writer-only helpers (always called while `WriteGuard` is held) ----

    #[inline(always)]
    fn z_avail(&self, meta: &ShardWriterMeta) -> usize {
        (self.zover_cap - meta.zlen) as usize + (meta.zfree_cnt as usize)
    }

    #[inline(always)]
    fn m_avail(&self, meta: &ShardWriterMeta) -> usize {
        (self.mpool_cap - meta.mlen) as usize + (meta.mfree_cnt as usize)
    }

    #[inline(always)]
    fn t_avail(&self, meta: &ShardWriterMeta) -> usize {
        (self.tpool_cap - meta.tlen) as usize + (meta.tfree_cnt as usize)
    }

    #[inline(always)]
    fn needs_gc(&self, meta: &ShardWriterMeta) -> bool {
        meta.num >= self.sz_elm as usize
            || self.z_avail(meta) <= 2
            || self.m_avail(meta) <= 2
            || self.t_avail(meta) <= 2
    }

    #[inline(always)]
    fn alloc_zover(&self, meta: &mut ShardWriterMeta) -> Option<u16> {
        if meta.zfree != NONE16 {
            let idx = meta.zfree;
            meta.zfree = (self.zover()[idx as usize].w3.load(Ordering::Relaxed) & 0xFFFF) as u16;
            meta.zfree_cnt = meta.zfree_cnt.saturating_sub(1);
            Some(idx)
        } else if meta.zlen < self.zover_cap {
            let idx = meta.zlen;
            meta.zlen += 1;
            Some(idx)
        } else {
            None
        }
    }

    #[inline(always)]
    fn free_zover(&self, meta: &mut ShardWriterMeta, idx: u16) {
        let ze = &self.zover()[idx as usize];
        ze.w0.store(0, Ordering::Relaxed);
        ze.w3
            .store(pack_zw3(meta.zfree, NONE16, NONE16, 0), Ordering::Relaxed);
        meta.zfree = idx;
        meta.zfree_cnt = meta.zfree_cnt.saturating_add(1);
    }

    #[inline(always)]
    fn alloc_mcard_slot(&self, meta: &mut ShardWriterMeta, card: CardSnap, next_mc: u16) -> Option<u16> {
        let idx = if meta.mfree != NONE16 {
            let i = meta.mfree;
            meta.mfree = (self.mpool()[i as usize].w2.load(Ordering::Relaxed) & 0xFFFF) as u16;
            meta.mfree_cnt = meta.mfree_cnt.saturating_sub(1);
            i
        } else if meta.mlen < self.mpool_cap {
            let i = meta.mlen;
            meta.mlen += 1;
            i
        } else {
            return None;
        };
        self.store_pool_card(idx, card, next_mc);
        Some(idx)
    }

    #[inline(always)]
    fn free_mcard_slot(&self, meta: &mut ShardWriterMeta, idx: u16) {
        let slot = &self.mpool()[idx as usize];
        slot.w2
            .store(pack_mw2(meta.mfree, NONE16, 0), Ordering::Relaxed);
        meta.mfree = idx;
        meta.mfree_cnt = meta.mfree_cnt.saturating_add(1);
    }

    #[inline(always)]
    fn alloc_mcard(&self, meta: &mut ShardWriterMeta, card: CardSnap, next_mc: u16) -> Option<u16> {
        let idx = self.alloc_mcard_slot(meta, card, next_mc)?;
        meta.num += 1;
        Some(idx)
    }

    #[inline(always)]
    fn alloc_tlist(&self, meta: &mut ShardWriterMeta, next: u16, td: TData, dp: i16) -> Option<u16> {
        let idx = if meta.tfree != NONE16 {
            let i = meta.tfree;
            meta.tfree = self.tpool()[i as usize].w1.load(Ordering::Relaxed) as u16;
            meta.tfree_cnt = meta.tfree_cnt.saturating_sub(1);
            i
        } else if meta.tlen < self.tpool_cap {
            let i = meta.tlen;
            meta.tlen += 1;
            i
        } else {
            return None;
        };
        meta.num += 1;
        let ts = &self.tpool()[idx as usize];
        ts.w0.store(pack_w2(td, dp, false), Ordering::Relaxed);
        ts.w1.store(next as u32, Ordering::Relaxed);
        Some(idx)
    }

    #[inline(always)]
    fn free_tlist_chain(&self, meta: &mut ShardWriterMeta, mut idx: u16) {
        let tpool = self.tpool();
        while idx != NONE16 {
            let ts = &tpool[idx as usize];
            let next = ts.w1.load(Ordering::Relaxed) as u16;
            ts.w1.store(meta.tfree as u32, Ordering::Relaxed);
            meta.tfree = idx;
            meta.tfree_cnt = meta.tfree_cnt.saturating_add(1);
            meta.num = meta.num.saturating_sub(1);
            idx = next;
        }
    }

    #[inline(always)]
    fn find_zloc_writer(&self, zkey: u64) -> Option<ZLoc> {
        let b = (zkey & self.mask) as u16;
        let b_ze = &self.buckets()[b as usize];
        let bz = b_ze.w0.load(Ordering::Relaxed);
        if bz == 0 {
            return None;
        }
        if bz == zkey {
            return Some(ZLoc::Bucket(b));
        }
        let zover = self.zover();
        let mut next = (b_ze.w3.load(Ordering::Relaxed) & 0xFFFF) as u16;
        while next != NONE16 {
            let oz = &zover[next as usize];
            if oz.w0.load(Ordering::Relaxed) == zkey {
                return Some(ZLoc::Over(next));
            }
            next = (oz.w3.load(Ordering::Relaxed) & 0xFFFF) as u16;
        }
        None
    }

    #[inline]
    fn insert_new_zentry(&self, meta: &mut ShardWriterMeta, zkey: u64, card: CardSnap) {
        let b = (zkey & self.mask) as u16;
        let b_ze = &self.buckets()[b as usize];
        let bz = b_ze.w0.load(Ordering::Relaxed);
        if bz == 0 {
            meta.used_buckets.push(b);
            meta.num += 1;
            self.store_inline_card(b_ze, zkey, card, NONE16, NONE16);
        } else if let Some(oi) = self.alloc_zover(meta) {
            let old_next = (b_ze.w3.load(Ordering::Relaxed) & 0xFFFF) as u16;
            self.store_inline_card(&self.zover()[oi as usize], zkey, card, old_next, NONE16);
            let (_, ov, tl, cpn) = unpack_zw3(b_ze.w3.load(Ordering::Relaxed));
            b_ze.w3
                .store(pack_zw3(oi, ov, tl, cpn), Ordering::Relaxed);
            meta.num += 1;
        }
    }

    /// Collect all `MCard`s of a `ZLoc` (with their pool slot index if in `mpool`), rebuild after edits.
    fn collect_cards(&self, zloc: ZLoc) -> (u16, SmallVec<[(Option<u16>, CardSnap); 8]>) {
        let ze = self.zentry(zloc);
        let (head, next_zf, mut mc) = self.load_inline_card(ze);
        let mut cards = SmallVec::<[(Option<u16>, CardSnap); 8]>::new();
        cards.push((None, head));
        let mpool = self.mpool();
        while mc != NONE16 {
            let slot = &mpool[mc as usize];
            let (mkey, nouse, hinc, protect) = unpack_w1(slot.w0.load(Ordering::Relaxed));
            let (head_tdata, head_dp, cur) = unpack_w2(slot.w1.load(Ordering::Relaxed));
            let (next_mc, tlist_tail, cpn) = unpack_mw2(slot.w2.load(Ordering::Relaxed));
            cards.push((
                Some(mc),
                CardSnap {
                    mkey,
                    nouse,
                    hinc,
                    protect,
                    head_tdata,
                    head_dp,
                    cur,
                    tlist_tail,
                    cpn,
                },
            ));
            mc = next_mc;
        }
        (next_zf, cards)
    }

    /// Write back a non-empty list of `(existing_pool_idx, CardSnap)` into `zloc`,
    /// freeing unused pool indices, allocating pool indices as needed, and updating `meta.num`
    /// by exact net delta (`written_cards - orig_cards_len`).
    #[allow(clippy::too_many_arguments)]
    fn writeback_cards(
        &self,
        meta: &mut ShardWriterMeta,
        zloc: ZLoc,
        zkey: u64,
        next_zf: u16,
        orig_cards_len: usize,
        cards: &[(Option<u16>, CardSnap)],
        freed_pool_slots: &[u16],
    ) {
        debug_assert!(!cards.is_empty());
        for &idx in freed_pool_slots {
            self.free_mcard_slot(meta, idx);
        }
        let (first_orig_pool, first_card) = cards[0];
        if let Some(pidx) = first_orig_pool {
            self.free_mcard_slot(meta, pidx);
        }
        let mut overflow_mc = NONE16;
        let mut written = 1usize;
        for &(orig_pool, card) in cards[1..].iter().rev() {
            if let Some(pidx) = orig_pool {
                self.store_pool_card(pidx, card, overflow_mc);
                overflow_mc = pidx;
                written += 1;
            } else if let Some(new_idx) = self.alloc_mcard_slot(meta, card, overflow_mc) {
                overflow_mc = new_idx;
                written += 1;
            }
        }
        self.store_inline_card(self.zentry(zloc), zkey, first_card, next_zf, overflow_mc);
        meta.num = (meta.num + written).saturating_sub(orig_cards_len);
    }

    fn tsumi_update(
        &self,
        meta: &mut ShardWriterMeta,
        ks: &KeyState,
        mv: &MvItem,
        tn: Turn,
        flag: bool,
        force_sh: bool,
    ) {
        let zkey = norm_zkey(ks.zkey);
        if flag {
            meta.record_prot_bucket((zkey & self.mask) as u16);
        }
        let new_card = CardSnap {
            mkey: mv.mkey,
            nouse: mv.nouse,
            hinc: mv.hinc(),
            protect: flag,
            head_tdata: mv.tdata,
            head_dp: ALL_DEPTH,
            cur: false,
            tlist_tail: NONE16,
            cpn: 0,
        };
        let Some(zloc) = self.find_zloc_writer(zkey) else {
            self.insert_new_zentry(meta, zkey, new_card);
            return;
        };

        let cur_mkey = ks.mkey_for(tn);
        let ze = self.zentry(zloc);
        let (head, next_zf, overflow_mc) = self.load_inline_card(ze);

        // Fast path (~90%): single inline card with matching `cur_mkey` -> overwrite directly in place!
        if overflow_mc == NONE16 && head.mkey.0 == cur_mkey.0 {
            if head.tlist_tail != NONE16 {
                self.free_tlist_chain(meta, head.tlist_tail);
            }
            self.store_inline_card(ze, zkey, new_card, next_zf, NONE16);
            return;
        }

        let (next_zf, orig_cards) = self.collect_cards(zloc);
        let orig_count = orig_cards.len();
        let mut kept = SmallVec::<[(Option<u16>, CardSnap); 8]>::new();
        let mut freed = SmallVec::<[u16; 8]>::new();

        // Step 1: remove first card matching `cur_mkey`.
        let mut removed_cur = false;
        let mut after_cur = SmallVec::<[(Option<u16>, CardSnap); 8]>::new();
        for (pidx, c) in orig_cards {
            if !removed_cur && c.mkey.0 == cur_mkey.0 {
                removed_cur = true;
                if c.tlist_tail != NONE16 {
                    self.free_tlist_chain(meta, c.tlist_tail);
                }
                if let Some(p) = pidx {
                    freed.push(p);
                }
                continue;
            }
            after_cur.push((pidx, c));
        }

        // Step 2: dominance prune against `mv.mkey`.
        let proof_mkey = mv.mkey;
        let mut updated = false;
        for (pidx, mut c) in after_cur {
            match proof_mkey.compare(c.mkey) {
                MKeyCmp::Super | MKeyCmp::Other => {
                    kept.push((pidx, c));
                }
                MKeyCmp::Infer => {
                    if c.tlist_tail != NONE16 {
                        self.free_tlist_chain(meta, c.tlist_tail);
                    }
                    if let Some(p) = pidx {
                        freed.push(p);
                    }
                }
                MKeyCmp::Equal => {
                    let td = c.head_tdata;
                    if td.pn == 0 {
                        if (force_sh && !c.protect)
                            || td.sh > mv.tdata.sh
                            || c.hinc != mv.hinc()
                        {
                            c.head_tdata.sh = mv.tdata.sh;
                            c.hinc = mv.hinc();
                            c.nouse = mv.nouse;
                        }
                        if flag {
                            c.protect = true;
                        }
                        updated = true;
                    } else if td.dn != 0 {
                        if c.tlist_tail != NONE16 {
                            self.free_tlist_chain(meta, c.tlist_tail);
                            c.tlist_tail = NONE16;
                        }
                        c.head_dp = ALL_DEPTH;
                        c.head_tdata = mv.tdata;
                        c.hinc = mv.hinc();
                        c.nouse = mv.nouse;
                        if flag {
                            c.protect = true;
                        }
                        updated = true;
                    }
                    kept.push((pidx, c));
                }
            }
        }

        if !updated {
            kept.insert(0, (None, new_card));
        }
        self.writeback_cards(meta, zloc, zkey, next_zf, orig_count, &kept, &freed);
    }

    fn fudumi_update(&self, meta: &mut ShardWriterMeta, ks: &KeyState, mv: &MvItem, tn: Turn) {
        let zkey = norm_zkey(ks.zkey);
        let new_card = CardSnap {
            mkey: mv.mkey,
            nouse: 0,
            hinc: false,
            protect: false,
            head_tdata: mv.tdata,
            head_dp: ALL_DEPTH,
            cur: false,
            tlist_tail: NONE16,
            cpn: 0,
        };
        let Some(zloc) = self.find_zloc_writer(zkey) else {
            self.insert_new_zentry(meta, zkey, new_card);
            return;
        };

        let cur_mkey = ks.mkey_for(tn);
        let ze = self.zentry(zloc);
        let (head, next_zf, overflow_mc) = self.load_inline_card(ze);

        // Fast path: single inline card with matching `cur_mkey`.
        if overflow_mc == NONE16 && head.mkey.0 == cur_mkey.0 {
            if head.tlist_tail != NONE16 {
                self.free_tlist_chain(meta, head.tlist_tail);
            }
            self.store_inline_card(ze, zkey, new_card, next_zf, NONE16);
            return;
        }

        let (next_zf, orig_cards) = self.collect_cards(zloc);
        let orig_count = orig_cards.len();
        let mut freed = SmallVec::<[u16; 8]>::new();

        let mut removed_cur = false;
        let mut after_cur = SmallVec::<[(Option<u16>, CardSnap); 8]>::new();
        for (pidx, c) in orig_cards {
            if !removed_cur && c.mkey.0 == cur_mkey.0 {
                removed_cur = true;
                if c.tlist_tail != NONE16 {
                    self.free_tlist_chain(meta, c.tlist_tail);
                }
                if let Some(p) = pidx {
                    freed.push(p);
                }
                continue;
            }
            after_cur.push((pidx, c));
        }

        let dp_mkey = mv.mkey;
        let mut kept = SmallVec::<[(Option<u16>, CardSnap); 8]>::new();
        let mut updated = false;
        for (pidx, mut c) in after_cur {
            match dp_mkey.compare(c.mkey) {
                MKeyCmp::Super => {
                    if c.tlist_tail != NONE16 {
                        self.free_tlist_chain(meta, c.tlist_tail);
                    }
                    if let Some(p) = pidx {
                        freed.push(p);
                    }
                }
                MKeyCmp::Infer | MKeyCmp::Other => {
                    kept.push((pidx, c));
                }
                MKeyCmp::Equal => {
                    let td = c.head_tdata;
                    if td.dn == 0 {
                        updated = true;
                    } else if td.pn != 0 {
                        if c.tlist_tail != NONE16 {
                            self.free_tlist_chain(meta, c.tlist_tail);
                            c.tlist_tail = NONE16;
                        }
                        c.head_dp = ALL_DEPTH;
                        c.head_tdata = mv.tdata;
                        updated = true;
                    }
                    kept.push((pidx, c));
                }
            }
        }

        if !updated {
            kept.insert(0, (None, new_card));
        }
        self.writeback_cards(meta, zloc, zkey, next_zf, orig_count, &kept, &freed);
    }

    fn fumei_update(
        &self,
        meta: &mut ShardWriterMeta,
        ks: &KeyState,
        mv: &mut MvItem,
        tn: Turn,
        in_path: impl Fn(u64, MKey) -> bool,
    ) {
        let zkey = norm_zkey(ks.zkey);
        let mkey = ks.mkey_for(tn);
        let Some(zloc) = self.find_zloc_writer(zkey) else {
            self.insert_new_zentry(
                meta,
                zkey,
                CardSnap {
                    mkey,
                    nouse: 0,
                    hinc: false,
                    protect: false,
                    head_tdata: mv.tdata,
                    head_dp: ks.count as i16,
                    cur: false,
                    tlist_tail: NONE16,
                    cpn: 0,
                },
            );
            return;
        };

        let ze = self.zentry(zloc);
        let (head, next_zf, overflow_mc) = self.load_inline_card(ze);

        // Ultra-fast path: single inline card with equal `mkey` and unknown status.
        if overflow_mc == NONE16 && head.mkey.0 == mkey.0 {
            let htd = head.head_tdata;
            if htd.pn == 0 || htd.dn == 0 {
                mv.tdata = htd;
                mv.mkey = head.mkey;
                return;
            }
            self.update_unknown_card_in_place(meta, Some(zloc), None, head, next_zf, overflow_mc, zkey, ks.count as i16, mv.tdata);
            return;
        }

        let mut super_tsumi: Option<CardSnap> = None;
        let mut infer_fudumi: Option<CardSnap> = None;
        let mut equal_tsumi: Option<CardSnap> = None;
        let mut equal_fudumi: Option<CardSnap> = None;
        let mut equal_unknown: Option<(Option<u16>, CardSnap)> = None;

        // Walk inline + overflow_mc chain.
        let mut cur_pidx: Option<u16> = None;
        let mut cur_card = Some(head);
        let mut next_mc = overflow_mc;

        let tpool = self.tpool();
        let mpool = self.mpool();

        while let Some(mcard) = cur_card {
            let head_td = mcard.head_tdata;
            if mkey.0 == mcard.mkey.0 {
                if head_td.pn == 0 {
                    equal_tsumi = Some(mcard);
                } else if head_td.dn == 0 {
                    equal_fudumi = Some(mcard);
                } else {
                    equal_unknown = Some((cur_pidx, mcard));
                }
            } else if mkey.ge_all(mcard.mkey) {
                if head_td.pn == 0 {
                    if super_tsumi.is_none_or(|st| st.mkey.ge_all(mcard.mkey)) {
                        super_tsumi = Some(mcard);
                    }
                } else if head_td.dn != 0 && !mcard.cur && !in_path(ks.zkey, mcard.mkey) {
                    if mv.tdata.pn > head_td.pn {
                        let mut updated_td = head_td;
                        updated_td.pn = mv.tdata.pn;
                        self.store_card_w2(zloc, cur_pidx, updated_td, mcard.head_dp, mcard.cur);
                    }
                    let mut tl = mcard.tlist_tail;
                    while tl != NONE16 {
                        let ts = &tpool[tl as usize];
                        let (mut ntd, ndp, _) = unpack_w2(ts.w0.load(Ordering::Relaxed));
                        if mv.tdata.pn > ntd.pn {
                            ntd.pn = mv.tdata.pn;
                            ts.w0.store(pack_w2(ntd, ndp, false), Ordering::Relaxed);
                        }
                        tl = ts.w1.load(Ordering::Relaxed) as u16;
                    }
                }
            } else if mcard.mkey.ge_all(mkey) {
                if head_td.pn != 0 && head_td.dn == 0 {
                    if infer_fudumi.is_none_or(|inf| mcard.mkey.ge_all(inf.mkey)) {
                        infer_fudumi = Some(mcard);
                    }
                } else if head_td.pn != 0 && !mcard.cur && !in_path(ks.zkey, mcard.mkey) {
                    if mv.tdata.pn < head_td.pn {
                        let mut updated_td = head_td;
                        updated_td.pn = mv.tdata.pn;
                        self.store_card_w2(zloc, cur_pidx, updated_td, mcard.head_dp, mcard.cur);
                    }
                    let mut tl = mcard.tlist_tail;
                    while tl != NONE16 {
                        let ts = &tpool[tl as usize];
                        let (mut ntd, ndp, _) = unpack_w2(ts.w0.load(Ordering::Relaxed));
                        if mv.tdata.pn < ntd.pn {
                            ntd.pn = mv.tdata.pn;
                            ts.w0.store(pack_w2(ntd, ndp, false), Ordering::Relaxed);
                        }
                        tl = ts.w1.load(Ordering::Relaxed) as u16;
                    }
                }
            }

            if next_mc == NONE16 {
                break;
            }
            cur_pidx = Some(next_mc);
            let slot = &mpool[next_mc as usize];
            let (mk, nu, hi, pr) = unpack_w1(slot.w0.load(Ordering::Relaxed));
            let (td, dp, cur) = unpack_w2(slot.w1.load(Ordering::Relaxed));
            let (nxt, tlt, cpn) = unpack_mw2(slot.w2.load(Ordering::Relaxed));
            cur_card = Some(CardSnap {
                mkey: mk,
                nouse: nu,
                hinc: hi,
                protect: pr,
                head_tdata: td,
                head_dp: dp,
                cur,
                tlist_tail: tlt,
                cpn,
            });
            next_mc = nxt;
        }

        for found in [super_tsumi, infer_fudumi, equal_tsumi, equal_fudumi] {
            if let Some(mc) = found {
                mv.tdata = mc.head_tdata;
                mv.mkey = mc.mkey;
                return;
            }
        }

        if let Some((pidx, card)) = equal_unknown {
            if pidx.is_none() {
                self.update_unknown_card_in_place(
                    meta,
                    Some(zloc),
                    None,
                    card,
                    next_zf,
                    overflow_mc,
                    zkey,
                    ks.count as i16,
                    mv.tdata,
                );
            } else {
                self.update_unknown_card_in_place(
                    meta,
                    None,
                    pidx,
                    card,
                    0,
                    0,
                    zkey,
                    ks.count as i16,
                    mv.tdata,
                );
            }
        } else {
            // Prepend new unknown card into `mpool` (linked under `zloc.overflow_mc`).
            let new_card = CardSnap {
                mkey,
                nouse: 0,
                hinc: false,
                protect: false,
                head_tdata: mv.tdata,
                head_dp: ks.count as i16,
                cur: false,
                tlist_tail: NONE16,
                cpn: 0,
            };
            if let Some(new_idx) = self.alloc_mcard(meta, new_card, overflow_mc) {
                let (nzf, _, tl, cpn) = unpack_zw3(ze.w3.load(Ordering::Relaxed));
                ze.w3
                    .store(pack_zw3(nzf, new_idx, tl, cpn), Ordering::Relaxed);
            }
        }
    }

    #[inline(always)]
    fn store_card_w2(&self, zloc: ZLoc, pidx: Option<u16>, td: TData, dp: i16, cur: bool) {
        let val = pack_w2(td, dp, cur);
        match pidx {
            None => self.zentry(zloc).w2.store(val, Ordering::Relaxed),
            Some(i) => self.mpool()[i as usize].w1.store(val, Ordering::Relaxed),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn update_unknown_card_in_place(
        &self,
        meta: &mut ShardWriterMeta,
        inline_loc: Option<ZLoc>,
        pool_idx: Option<u16>,
        mut card: CardSnap,
        next_zf: u16,
        overflow_mc: u16,
        _zkey: u64,
        target_dp: i16,
        new_td: TData,
    ) {
        let mut hit_head = false;
        if card.head_dp == target_dp {
            hit_head = true;
        } else if new_td.pn > card.head_tdata.pn {
            card.head_tdata.pn = new_td.pn;
        }

        let tpool = self.tpool();
        let mut same_tl = NONE16;
        let mut tl_len = 0u32;
        let mut min_tl = NONE16;
        let mut min_sum = u32::MAX;
        let mut tl = card.tlist_tail;
        while tl != NONE16 {
            let ts = &tpool[tl as usize];
            let (mut ntd, ndp, _) = unpack_w2(ts.w0.load(Ordering::Relaxed));
            let nxt = ts.w1.load(Ordering::Relaxed) as u16;
            if ndp == target_dp {
                same_tl = tl;
            } else {
                if new_td.pn > ntd.pn {
                    ntd.pn = new_td.pn;
                    ts.w0.store(pack_w2(ntd, ndp, false), Ordering::Relaxed);
                }
                let s = (ntd.pn as u32) + (ntd.dn as u32);
                if s <= min_sum {
                    min_sum = s;
                    min_tl = tl;
                }
            }
            tl_len += 1;
            tl = nxt;
        }

        let new_sum = (new_td.pn as u32) + (new_td.dn as u32);
        if hit_head {
            let old_sum = (card.head_tdata.pn as u32) + (card.head_tdata.dn as u32);
            if old_sum < 32 || new_sum >= (old_sum >> 2) {
                card.head_tdata = new_td;
            }
        } else if same_tl != NONE16 {
            let ts = &tpool[same_tl as usize];
            let (old_td, ndp, _) = unpack_w2(ts.w0.load(Ordering::Relaxed));
            let old_sum = (old_td.pn as u32) + (old_td.dn as u32);
            if old_sum < 32 || new_sum >= (old_sum >> 2) {
                ts.w0.store(pack_w2(new_td, ndp, false), Ordering::Relaxed);
            }
        } else if tl_len >= 3 && min_tl != NONE16 {
            let ts = &tpool[min_tl as usize];
            ts.w0
                .store(pack_w2(card.head_tdata, card.head_dp, false), Ordering::Relaxed);
            card.head_tdata = new_td;
            card.head_dp = target_dp;
        } else if let Some(pushed) =
            self.alloc_tlist(meta, card.tlist_tail, card.head_tdata, card.head_dp)
        {
            card.head_tdata = new_td;
            card.head_dp = target_dp;
            card.tlist_tail = pushed;
        }

        if let Some(zloc) = inline_loc {
            let ze = self.zentry(zloc);
            ze.w2
                .store(pack_w2(card.head_tdata, card.head_dp, card.cur), Ordering::Relaxed);
            ze.w3.store(
                pack_zw3(next_zf, overflow_mc, card.tlist_tail, card.cpn),
                Ordering::Relaxed,
            );
        } else if let Some(pidx) = pool_idx {
            let slot = &self.mpool()[pidx as usize];
            let (next_mc, _, cpn) = unpack_mw2(slot.w2.load(Ordering::Relaxed));
            slot.w1
                .store(pack_w2(card.head_tdata, card.head_dp, card.cur), Ordering::Relaxed);
            slot.w2
                .store(pack_mw2(next_mc, card.tlist_tail, cpn), Ordering::Relaxed);
        }
    }

    #[inline]
    fn should_delete_entry(
        tdata: TData,
        dp: i16,
        gc_level: u16,
        tsumi_delete_flag: bool,
        odd_tsumi_cut: u16,
        n_tsumi: &mut usize,
        del_tsumi: &mut usize,
    ) -> bool {
        if tdata.pn == 0 {
            *n_tsumi += 1;
            let del = if tsumi_delete_flag {
                tdata.sh.is_multiple_of(2) || tdata.sh < 2 || tdata.sh < odd_tsumi_cut
            } else {
                false
            };
            if del {
                *del_tsumi += 1;
            }
            del
        } else if tdata.dn == 0 {
            if gc_level < GC_DELETE_OFFSET {
                tdata.sh.is_multiple_of(2)
            } else {
                tdata.sh.is_multiple_of(2) || tdata.sh < gc_level - 2
            }
        } else {
            let dp_even = (dp & 1) == 0;
            (tdata.pn < gc_level && dp_even) || (tdata.dn < gc_level && dp_even) || !dp_even
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn gc_card(
        &self,
        meta: &mut ShardWriterMeta,
        mut card: CardSnap,
        gc_level: u16,
        tsumi_delete_flag: bool,
        odd_tsumi_cut: u16,
        n_tsumi: &mut usize,
        del_tsumi: &mut usize,
        delete_num: &mut usize,
    ) -> Option<CardSnap> {
        if card.protect || card.cur {
            return Some(card);
        }
        let tpool = self.tpool();
        let mut tl = card.tlist_tail;
        let mut new_tl = NONE16;
        while tl != NONE16 {
            let cur_tl = tl;
            let ts = &tpool[cur_tl as usize];
            tl = ts.w1.load(Ordering::Relaxed) as u16;
            let (ntd, ndp, _) = unpack_w2(ts.w0.load(Ordering::Relaxed));
            if Self::should_delete_entry(
                ntd,
                ndp,
                gc_level,
                tsumi_delete_flag,
                odd_tsumi_cut,
                n_tsumi,
                del_tsumi,
            ) {
                ts.w1.store(meta.tfree as u32, Ordering::Relaxed);
                meta.tfree = cur_tl;
                meta.tfree_cnt = meta.tfree_cnt.saturating_add(1);
                meta.num = meta.num.saturating_sub(1);
                *delete_num += 1;
            } else {
                ts.w1.store(new_tl as u32, Ordering::Relaxed);
                new_tl = cur_tl;
            }
        }
        card.tlist_tail = new_tl;

        let del_head = Self::should_delete_entry(
            card.head_tdata,
            card.head_dp,
            gc_level,
            tsumi_delete_flag,
            odd_tsumi_cut,
            n_tsumi,
            del_tsumi,
        );
        if del_head {
            *delete_num += 1;
            if new_tl != NONE16 {
                let ts = &tpool[new_tl as usize];
                let (ptd, pdp, _) = unpack_w2(ts.w0.load(Ordering::Relaxed));
                let pnext = ts.w1.load(Ordering::Relaxed) as u16;
                card.head_tdata = ptd;
                card.head_dp = pdp;
                card.tlist_tail = pnext;
                ts.w1.store(meta.tfree as u32, Ordering::Relaxed);
                meta.tfree = new_tl;
                meta.tfree_cnt = meta.tfree_cnt.saturating_add(1);
                meta.num = meta.num.saturating_sub(1);
                Some(card)
            } else {
                None
            }
        } else {
            Some(card)
        }
    }

    fn gc(&self, meta: &mut ShardWriterMeta, root_pn: u16) -> u16 {
        let need_z = self.z_avail(meta) <= 2;
        let need_m = self.m_avail(meta) <= 2;
        let need_t = self.t_avail(meta) <= 2;
        let req_z = if need_z { ((self.zover_cap as usize) / 8).max(8) } else { 0 };
        let req_m = if need_m { ((self.mpool_cap as usize) / 8).max(8) } else { 0 };
        let req_t = if need_t { ((self.tpool_cap as usize) / 8).max(8) } else { 0 };

        let mut gc_level: u16 = 2u16.max(meta.gc_max_level);
        let mut tsumi_delete_flag = false;
        let mut odd_tsumi_cut: u16 = 0;
        let mut delete_num: usize = 0;
        let sz_elm = self.sz_elm as usize;
        let eff_base = meta.num.max(sz_elm / 4);
        let gc_target = (eff_base * GC_DELETE_RATE / 100).max(1);
        let n_tsumi_max = sz_elm * GC_TSUMI_RATE / 100;
        let buckets = self.buckets();
        let zover = self.zover();

        loop {
            if odd_tsumi_cut == 0 && gc_level > meta.gc_max_level {
                meta.gc_max_level = gc_level;
            }
            let mut n_tsumi: usize = 0;
            let mut del_tsumi: usize = 0;

            let mut write_idx: usize = 0;
            for u in 0..meta.used_buckets.len() {
                let b = meta.used_buckets[u];
                // Gather all ZEntries in bucket `b`: Bucket(b) + zover chain.
                let mut surviving_over: SmallVec<[u16; 8]> = SmallVec::new();

                // Process zover chain first so we know which zover entries survive.
                let b_ze = &buckets[b as usize];
                let mut cur_over = (b_ze.w3.load(Ordering::Relaxed) & 0xFFFF) as u16;
                while cur_over != NONE16 {
                    let oi = cur_over;
                    let oz = &zover[oi as usize];
                    let oz_zkey = oz.w0.load(Ordering::Relaxed);
                    let (next_zf, cards) = self.collect_cards(ZLoc::Over(oi));
                    let orig_len = cards.len();
                    cur_over = next_zf;

                    let mut kept = SmallVec::<[(Option<u16>, CardSnap); 8]>::new();
                    let mut freed = SmallVec::<[u16; 8]>::new();
                    for (pidx, c) in cards {
                        if let Some(sc) = self.gc_card(
                            meta,
                            c,
                            gc_level,
                            tsumi_delete_flag,
                            odd_tsumi_cut,
                            &mut n_tsumi,
                            &mut del_tsumi,
                            &mut delete_num,
                        ) {
                            kept.push((pidx, sc));
                        } else if let Some(p) = pidx {
                            freed.push(p);
                        }
                    }
                    if kept.is_empty() {
                        for p in freed {
                            self.free_mcard_slot(meta, p);
                        }
                        meta.num = meta.num.saturating_sub(orig_len);
                        self.free_zover(meta, oi);
                    } else {
                        self.writeback_cards(
                            meta,
                            ZLoc::Over(oi),
                            oz_zkey,
                            NONE16,
                            orig_len,
                            &kept,
                            &freed,
                        );
                        surviving_over.push(oi);
                    }
                }

                // Process inline bucket `b`.
                let b_zkey = b_ze.w0.load(Ordering::Relaxed);
                let mut bucket_alive = false;
                if b_zkey != 0 {
                    let (_, cards) = self.collect_cards(ZLoc::Bucket(b));
                    let orig_len = cards.len();
                    let mut kept = SmallVec::<[(Option<u16>, CardSnap); 8]>::new();
                    let mut freed = SmallVec::<[u16; 8]>::new();
                    for (pidx, c) in cards {
                        if let Some(sc) = self.gc_card(
                            meta,
                            c,
                            gc_level,
                            tsumi_delete_flag,
                            odd_tsumi_cut,
                            &mut n_tsumi,
                            &mut del_tsumi,
                            &mut delete_num,
                        ) {
                            kept.push((pidx, sc));
                        } else if let Some(p) = pidx {
                            freed.push(p);
                        }
                    }
                    if kept.is_empty() {
                        for p in freed {
                            self.free_mcard_slot(meta, p);
                        }
                        meta.num = meta.num.saturating_sub(orig_len);
                        // Promote first surviving zover into bucket `b` if any!
                        if let Some(prom_oi) = surviving_over.pop() {
                            let poz = &zover[prom_oi as usize];
                            let pz = poz.w0.load(Ordering::Relaxed);
                            let pw1 = poz.w1.load(Ordering::Relaxed);
                            let pw2 = poz.w2.load(Ordering::Relaxed);
                            let (_, pov, ptl, pcpn) = unpack_zw3(poz.w3.load(Ordering::Relaxed));
                            let mut next_chain = NONE16;
                            for &oi in surviving_over.iter().rev() {
                                let oz = &zover[oi as usize];
                                let (_, ov, tl, cpn) = unpack_zw3(oz.w3.load(Ordering::Relaxed));
                                oz.w3
                                    .store(pack_zw3(next_chain, ov, tl, cpn), Ordering::Relaxed);
                                next_chain = oi;
                            }
                            b_ze.w1.store(pw1, Ordering::Relaxed);
                            b_ze.w2.store(pw2, Ordering::Relaxed);
                            b_ze.w3
                                .store(pack_zw3(next_chain, pov, ptl, pcpn), Ordering::Relaxed);
                            b_ze.w0.store(pz, Ordering::Relaxed);
                            self.free_zover(meta, prom_oi);
                            bucket_alive = true;
                        } else {
                            b_ze.w0.store(0, Ordering::Relaxed);
                            b_ze.w3
                                .store(pack_zw3(NONE16, NONE16, NONE16, 0), Ordering::Relaxed);
                        }
                    } else {
                        let mut next_chain = NONE16;
                        for &oi in surviving_over.iter().rev() {
                            let oz = &zover[oi as usize];
                            let (_, ov, tl, cpn) = unpack_zw3(oz.w3.load(Ordering::Relaxed));
                            oz.w3
                                .store(pack_zw3(next_chain, ov, tl, cpn), Ordering::Relaxed);
                            next_chain = oi;
                        }
                        self.writeback_cards(
                            meta,
                            ZLoc::Bucket(b),
                            b_zkey,
                            next_chain,
                            orig_len,
                            &kept,
                            &freed,
                        );
                        bucket_alive = true;
                    }
                }

                if bucket_alive {
                    meta.used_buckets[write_idx] = b;
                    write_idx += 1;
                }
            }
            meta.used_buckets.truncate(write_idx);

            let pools_ok = self.z_avail(meta) >= req_z
                && self.m_avail(meta) >= req_m
                && self.t_avail(meta) >= req_t;
            if (delete_num > gc_target && pools_ok) || meta.num == 0 || gc_level >= INFINATE - 1 {
                break;
            }
            let want_tsumi_del = ((n_tsumi.saturating_sub(del_tsumi) > n_tsumi_max)
                && gc_level >= root_pn / 2)
                || (!pools_ok && gc_level >= root_pn.max(4));
            if !tsumi_delete_flag && want_tsumi_del {
                tsumi_delete_flag = true;
            } else if tsumi_delete_flag && !pools_ok && gc_level >= root_pn.saturating_mul(2).max(16) {
                odd_tsumi_cut = odd_tsumi_cut.max(4).saturating_add(4);
                gc_level = if gc_level < 32 {
                    gc_level + 2
                } else {
                    gc_level.saturating_add(gc_level >> 2).min(INFINATE - 1)
                };
            } else {
                gc_level = if gc_level < 32 {
                    gc_level.saturating_add(1)
                } else {
                    gc_level.saturating_add(gc_level >> 2).min(INFINATE - 1)
                };
            }
        }
        meta.gc_max_level
    }

    fn clear(&self, meta: &mut ShardWriterMeta) {
        let buckets = self.buckets();
        for b in meta.used_buckets.drain(..) {
            let b_ze = &buckets[b as usize];
            b_ze.w0.store(0, Ordering::Relaxed);
            b_ze.w3
                .store(pack_zw3(NONE16, NONE16, NONE16, 0), Ordering::Relaxed);
        }
        meta.prot_buckets.clear();
        meta.cur_counts.clear();
        meta.zlen = 0;
        meta.mlen = 0;
        meta.tlen = 0;
        meta.zfree = NONE16;
        meta.mfree = NONE16;
        meta.tfree = NONE16;
        meta.zfree_cnt = 0;
        meta.mfree_cnt = 0;
        meta.tfree_cnt = 0;
        meta.num = 0;
        meta.gc_max_level = 0;
    }

    fn clear_protect(&self, meta: &mut ShardWriterMeta) {
        let mpool = self.mpool();
        for b in meta.prot_buckets.drain(..) {
            let mut zloc_opt = Some(ZLoc::Bucket(b));
            while let Some(zloc) = zloc_opt {
                let ze = self.zentry(zloc);
                if ze.w0.load(Ordering::Relaxed) == 0 {
                    break;
                }
                let w1 = ze.w1.load(Ordering::Relaxed);
                if (w1 & (MC_PROTECT as u64)) != 0 {
                    ze.w1
                        .store(w1 & !(MC_PROTECT as u64), Ordering::Relaxed);
                }
                let (next_zf, mut mc, _, _) = unpack_zw3(ze.w3.load(Ordering::Relaxed));
                while mc != NONE16 {
                    let slot = &mpool[mc as usize];
                    let mw0 = slot.w0.load(Ordering::Relaxed);
                    if (mw0 & (MC_PROTECT as u64)) != 0 {
                        slot.w0
                            .store(mw0 & !(MC_PROTECT as u64), Ordering::Relaxed);
                    }
                    mc = (slot.w2.load(Ordering::Relaxed) & 0xFFFF) as u16;
                }
                zloc_opt = if next_zf != NONE16 {
                    Some(ZLoc::Over(next_zf))
                } else {
                    None
                };
            }
        }
    }

    fn set_card_cur(&self, zkey: u64, mkey: MKey, cur: bool, cpn: Option<u16>) {
        let Some(zloc) = self.find_zloc_writer(zkey) else {
            return;
        };
        let ze = self.zentry(zloc);
        let (head, next_zf, mut mc) = self.load_inline_card(ze);
        if head.mkey.0 == mkey.0 {
            ze.w2
                .store(pack_w2(head.head_tdata, head.head_dp, cur), Ordering::Relaxed);
            if let Some(new_cpn) = cpn {
                ze.w3.store(
                    pack_zw3(next_zf, mc, head.tlist_tail, new_cpn),
                    Ordering::Relaxed,
                );
            }
            return;
        }
        let mpool = self.mpool();
        while mc != NONE16 {
            let slot = &mpool[mc as usize];
            let (mk, _, _, _) = unpack_w1(slot.w0.load(Ordering::Relaxed));
            let (td, dp, _) = unpack_w2(slot.w1.load(Ordering::Relaxed));
            let (next_mc, tlt, _old_cpn) = unpack_mw2(slot.w2.load(Ordering::Relaxed));
            if mk.0 == mkey.0 {
                slot.w1.store(pack_w2(td, dp, cur), Ordering::Relaxed);
                if let Some(new_cpn) = cpn {
                    slot.w2
                        .store(pack_mw2(next_mc, tlt, new_cpn), Ordering::Relaxed);
                }
                return;
            }
            mc = next_mc;
        }
    }

    fn inc_cur_count(&self, meta: &mut ShardWriterMeta, zkey: u64, mkey: MKey) {
        for e in meta.cur_counts.iter_mut() {
            if e.0 == zkey && e.1 .0 == mkey.0 {
                e.2 = e.2.saturating_add(1);
                return;
            }
        }
        meta.cur_counts.push((zkey, mkey, 1));
    }

    fn dec_cur_count(&self, meta: &mut ShardWriterMeta, zkey: u64, mkey: MKey) -> bool {
        for i in 0..meta.cur_counts.len() {
            if meta.cur_counts[i].0 == zkey && meta.cur_counts[i].1 .0 == mkey.0 {
                meta.cur_counts[i].2 = meta.cur_counts[i].2.saturating_sub(1);
                if meta.cur_counts[i].2 == 0 {
                    meta.cur_counts.swap_remove(i);
                    return true;
                }
                return false;
            }
        }
        true
    }

    fn enter_current(&self, meta: &mut ShardWriterMeta, ks: &KeyState, cpn: u16, tn: Turn) {
        let zkey = norm_zkey(ks.zkey);
        let mkey = ks.mkey_for(tn);
        self.inc_cur_count(meta, zkey, mkey);
        let Some(zloc) = self.find_zloc_writer(zkey) else {
            self.insert_new_zentry(
                meta,
                zkey,
                CardSnap {
                    mkey,
                    nouse: 0,
                    hinc: false,
                    protect: false,
                    head_tdata: TData::INIT,
                    head_dp: ks.count as i16,
                    cur: true,
                    tlist_tail: NONE16,
                    cpn,
                },
            );
            return;
        };
        let ze = self.zentry(zloc);
        let (head, next_zf, mut mc) = self.load_inline_card(ze);
        if head.mkey.0 == mkey.0 {
            ze.w2
                .store(pack_w2(head.head_tdata, head.head_dp, true), Ordering::Relaxed);
            ze.w3.store(
                pack_zw3(next_zf, mc, head.tlist_tail, cpn),
                Ordering::Relaxed,
            );
            return;
        }
        let orig_ov = mc;
        let mpool = self.mpool();
        while mc != NONE16 {
            let slot = &mpool[mc as usize];
            let (mk, _, _, _) = unpack_w1(slot.w0.load(Ordering::Relaxed));
            let (td, dp, _) = unpack_w2(slot.w1.load(Ordering::Relaxed));
            let (next_mc, tlt, _) = unpack_mw2(slot.w2.load(Ordering::Relaxed));
            if mk.0 == mkey.0 {
                slot.w1.store(pack_w2(td, dp, true), Ordering::Relaxed);
                slot.w2
                    .store(pack_mw2(next_mc, tlt, cpn), Ordering::Relaxed);
                return;
            }
            mc = next_mc;
        }
        if let Some(new_idx) = self.alloc_mcard(
            meta,
            CardSnap {
                mkey,
                nouse: 0,
                hinc: false,
                protect: false,
                head_tdata: TData::INIT,
                head_dp: ks.count as i16,
                cur: true,
                tlist_tail: NONE16,
                cpn,
            },
            orig_ov,
        ) {
            ze.w3.store(
                pack_zw3(next_zf, new_idx, head.tlist_tail, head.cpn),
                Ordering::Relaxed,
            );
        }
    }

    fn leave_current(&self, meta: &mut ShardWriterMeta, raw_zkey: u64, mkey: MKey) {
        let zkey = norm_zkey(raw_zkey);
        if self.dec_cur_count(meta, zkey, mkey) {
            self.set_card_cur(zkey, mkey, false, None);
        }
    }

    fn make_tree_set_item(
        &self,
        meta: &mut ShardWriterMeta,
        ks: &KeyState,
        tn: Turn,
        protect: bool,
    ) -> Option<(u64, MKey)> {
        let zkey = norm_zkey(ks.zkey);
        let zloc = self.find_zloc_writer(zkey)?;
        let mkey = ks.mkey_for(tn);
        let ze = self.zentry(zloc);
        let (head, next_zf, mut mc) = self.load_inline_card(ze);
        let mpool = self.mpool();

        let mut super_tsumi: Option<(Option<u16>, CardSnap)> = None;
        let mut equal_tsumi: Option<(Option<u16>, CardSnap)> = None;

        let mut cur_pidx: Option<u16> = None;
        let mut cur_card = Some(head);
        while let Some(c) = cur_card {
            if c.head_tdata.pn == 0 {
                if mkey.0 == c.mkey.0 {
                    equal_tsumi = Some((cur_pidx, c));
                } else if mkey.ge_all(c.mkey) {
                    super_tsumi = Some((cur_pidx, c));
                    break;
                }
            }
            if mc == NONE16 {
                break;
            }
            cur_pidx = Some(mc);
            let slot = &mpool[mc as usize];
            let (mk, nu, hi, pr) = unpack_w1(slot.w0.load(Ordering::Relaxed));
            let (td, dp, cur) = unpack_w2(slot.w1.load(Ordering::Relaxed));
            let (nxt, tlt, cpn) = unpack_mw2(slot.w2.load(Ordering::Relaxed));
            cur_card = Some(CardSnap {
                mkey: mk,
                nouse: nu,
                hinc: hi,
                protect: pr,
                head_tdata: td,
                head_dp: dp,
                cur,
                tlist_tail: tlt,
                cpn,
            });
            mc = nxt;
        }

        let (t_pidx, t_card) = super_tsumi.or(equal_tsumi)?;
        if protect {
            if !t_card.protect {
                meta.record_prot_bucket((zkey & self.mask) as u16);
                match t_pidx {
                    None => {
                        let w1 = ze.w1.load(Ordering::Relaxed);
                        ze.w1.store(w1 | (MC_PROTECT as u64), Ordering::Relaxed);
                    }
                    Some(pi) => {
                        let slot = &mpool[pi as usize];
                        let w0 = slot.w0.load(Ordering::Relaxed);
                        slot.w0.store(w0 | (MC_PROTECT as u64), Ordering::Relaxed);
                    }
                }
            }
            None
        } else {
            self.inc_cur_count(meta, zkey, t_card.mkey);
            match t_pidx {
                None => {
                    ze.w2.store(
                        pack_w2(t_card.head_tdata, t_card.head_dp, true),
                        Ordering::Relaxed,
                    );
                    let _ = next_zf;
                }
                Some(pi) => {
                    mpool[pi as usize].w1.store(
                        pack_w2(t_card.head_tdata, t_card.head_dp, true),
                        Ordering::Relaxed,
                    );
                }
            }
            Some((zkey, t_card.mkey))
        }
    }
}

#[derive(Copy, Clone)]
struct ReadOnlyBucketPtr(*const InlineZEntry);
unsafe impl Send for ReadOnlyBucketPtr {}
unsafe impl Sync for ReadOnlyBucketPtr {}

/// Direct-mapped per-worker L2 terminal cache (`4096 * 24 B = 96 KB`).
#[derive(Copy, Clone)]
#[repr(C)]
pub(crate) struct LocalTerminalEntry {
    pub zkey: u64,
    pub query_mkey: MKey,
    pub res_mkey: MKey,
    pub tdata: TData,
    pub gc_gen: u16,
}

impl LocalTerminalEntry {
    pub(crate) const EMPTY: Self = Self {
        zkey: 0,
        query_mkey: MKey::EMPTY,
        res_mkey: MKey::EMPTY,
        tdata: TData::INIT,
        gc_gen: 0,
    };
}

#[inline(always)]
pub(crate) fn local_tt_idx(zkey: u64, mkey: MKey) -> usize {
    let h = zkey ^ (zkey >> 23) ^ (mkey.0 as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15);
    (h as usize) & LOCAL_TT_MASK
}

/// Lock-free optimistic-read sharded transposition table preserving exact `MKey` dominance.
pub struct SharedTBase {
    shards: Box<[ShardSlot]>,
    buckets_base: ReadOnlyBucketPtr,
    buckets_shift: u32,
    bucket_page_shift: u32,
    #[allow(dead_code)]
    shard_mask: u64,
    n_shards: usize,
    shard_idx_mask: usize,
    bucket_page_bits: Box<[AtomicU64]>,
    shard_gc_gens: Box<[AtomicU16]>,
    solved_bits: Box<[AtomicU64]>,
    shard_gcs: AtomicU64,
    gc_max_level: AtomicU16,
    pub(crate) root_pn: AtomicU16,
    // Declared last so `shards` (which holds non-owning pointers into `_arena`) drops before `_arena`.
    _arena: ArenaBuffer,
}

impl SharedTBase {
    #[inline]
    pub(crate) const fn shard_count_for_hash_mb(hash_mb: usize) -> usize {
        if hash_mb <= 2048 {
            8192usize
        } else if hash_mb <= 8192 {
            16384usize
        } else {
            32768usize
        }
    }

    pub fn new(hash_mb: usize) -> Self {
        let n_shards = Self::shard_count_for_hash_mb(hash_mb);
        let shard_idx_mask = n_shards - 1;
        let bytes_per_shard = (hash_mb.max(1) * 1024 * 1024) / n_shards;
        let mut buckets_len = 64usize;
        while (buckets_len * 2) * 32 <= (bytes_per_shard * 50 / 100) && buckets_len < 32768 {
            buckets_len <<= 1;
        }
        debug_assert!(buckets_len.is_power_of_two() && buckets_len <= 32768);
        let buckets_shift = buckets_len.trailing_zeros();
        // 2 MiB / 32 B = 65,536 = 2^16 InlineZEntries per 2 MiB HugePage.
        let bucket_page_shift = 16 - buckets_shift;
        let zover_cap = ((buckets_len * 3) / 4).clamp(32, 65500);
        let mpool_cap = ((buckets_len * 3) / 4).clamp(32, 65500);
        let tpool_cap = ((buckets_len * 2) / 3).clamp(32, 65500);
        let sz_elm = ((buckets_len * 7) / 4).max(64) as u32;

        // Lay out ONE 2 MiB-aligned OS-zeroed arena grouped into 4 contiguous SoA region blocks
        // advised with `MADV_HUGEPAGE` on Linux:
        // - Block 0: all `n_shards * buckets_len` `InlineZEntry`s (write-first faulted via
        //   `shard_slot` so Linux allocates writable 2 MiB THPs with zero `huge_zero_pmd` splits).
        // - Block 1: all `n_shards * zover_cap` `InlineZEntry`s (first touch is always a write).
        // - Block 2: all `n_shards * mpool_cap` `MCardSlot`s (first touch is always a write).
        // - Block 3: all `n_shards * tpool_cap` `TListSlot`s (first touch is always a write).
        let buckets_bytes = n_shards
            .checked_mul(buckets_len)
            .and_then(|n| n.checked_mul(std::mem::size_of::<InlineZEntry>()))
            .expect("buckets region overflow");
        let zover_bytes = n_shards
            .checked_mul(zover_cap)
            .and_then(|n| n.checked_mul(std::mem::size_of::<InlineZEntry>()))
            .expect("zover region overflow");
        let mpool_bytes = n_shards
            .checked_mul(mpool_cap)
            .and_then(|n| n.checked_mul(std::mem::size_of::<MCardSlot>()))
            .expect("mpool region overflow");
        let tpool_bytes = n_shards
            .checked_mul(tpool_cap)
            .and_then(|n| n.checked_mul(std::mem::size_of::<TListSlot>()))
            .expect("tpool region overflow");

        let zover_offset = buckets_bytes;
        let mpool_offset = zover_offset
            .checked_add(zover_bytes)
            .expect("arena offset overflow");
        let tpool_offset = mpool_offset
            .checked_add(mpool_bytes)
            .expect("arena offset overflow");
        let total_bytes = tpool_offset
            .checked_add(tpool_bytes)
            .expect("arena total overflow");

        let arena_huge_bytes = total_bytes
            .checked_add(HUGEPAGE_ALIGN - 1)
            .expect("arena align overflow")
            & !(HUGEPAGE_ALIGN - 1);

        let (arena, base_ptr) =
            ArenaBuffer::new_zeroed_2mb_aligned(arena_huge_bytes, arena_huge_bytes);
        let buckets_base_ptr = base_ptr as *const InlineZEntry;
        let zover_base_ptr = unsafe { base_ptr.add(zover_offset) as *const InlineZEntry };
        let mpool_base_ptr = unsafe { base_ptr.add(mpool_offset) as *const MCardSlot };
        let tpool_base_ptr = unsafe { base_ptr.add(tpool_offset) as *const TListSlot };

        let zover_cap_u16 = zover_cap as u16;
        let mpool_cap_u16 = mpool_cap as u16;
        let tpool_cap_u16 = tpool_cap as u16;
        let shard_mask = (buckets_len as u64) - 1;

        let mut vec = Vec::with_capacity(n_shards);
        for i in 0..n_shards {
            let slot = unsafe {
                ShardSlot::from_arena(
                    buckets_base_ptr.add(i * buckets_len),
                    buckets_len,
                    zover_base_ptr.add(i * zover_cap),
                    zover_cap_u16,
                    mpool_base_ptr.add(i * mpool_cap),
                    mpool_cap_u16,
                    tpool_base_ptr.add(i * tpool_cap),
                    tpool_cap_u16,
                    sz_elm,
                )
            };
            vec.push(slot);
        }
        let num_bucket_pages = buckets_bytes.div_ceil(HUGEPAGE_ALIGN).max(1);
        let num_page_words = num_bucket_pages.div_ceil(64).max(1);
        let page_bits: Box<[AtomicU64]> = (0..num_page_words)
            .map(|_| AtomicU64::new(0))
            .collect::<Vec<_>>()
            .into_boxed_slice();
        let gens: Box<[AtomicU16]> = (0..n_shards)
            .map(|_| AtomicU16::new(0))
            .collect::<Vec<_>>()
            .into_boxed_slice();
        let sbits: Box<[AtomicU64]> = (0..SOLVED_WORDS)
            .map(|_| AtomicU64::new(0))
            .collect::<Vec<_>>()
            .into_boxed_slice();

        Self {
            shards: vec.into_boxed_slice(),
            buckets_base: ReadOnlyBucketPtr(buckets_base_ptr),
            buckets_shift,
            bucket_page_shift,
            shard_mask,
            n_shards,
            shard_idx_mask,
            bucket_page_bits: page_bits,
            shard_gc_gens: gens,
            solved_bits: sbits,
            shard_gcs: AtomicU64::new(0),
            gc_max_level: AtomicU16::new(0),
            root_pn: AtomicU16::new(1),
            _arena: arena,
        }
    }

    #[inline(always)]
    fn shard_idx(&self, zkey: u64) -> usize {
        (((zkey >> 32) ^ (zkey >> 49)) as usize) & self.shard_idx_mask
    }

    /// Returns shard `idx`, lazily faulting in its 2 MiB bucket HugePage with `FAULT_FLAG_WRITE`
    /// on first access so Linux THP allocates a writable 2 MiB PMD directly instead of mapping
    /// `huge_zero_pmd` on a read fault and splitting it (`thp_split_pmd`) on first write.
    #[inline(always)]
    fn shard_slot(&self, idx: usize) -> &ShardSlot {
        let p = idx >> self.bucket_page_shift;
        let w = unsafe { self.bucket_page_bits.get_unchecked(p >> 6) };
        let bit = 1u64 << (p & 63);
        if (w.load(Ordering::Acquire) & bit) == 0 {
            self.touch_bucket_hugepage_write(p, w, bit);
        }
        unsafe { self.shards.get_unchecked(idx) }
    }

    #[cold]
    #[inline(never)]
    fn touch_bucket_hugepage_write(&self, page_idx: usize, word: &AtomicU64, bit: u64) {
        let entry_offset = page_idx << 16; // 65,536 * 32 B = 2 MiB
        let ze = unsafe { &*self.buckets_base.0.add(entry_offset) };
        // `black_box(0)` prevents LLVM `InstCombineAtomicRMW` from folding idempotent `fetch_or(0)`
        // into an atomic load (which would fault in read-only `huge_zero_pmd` instead of a writable THP).
        ze.w0.fetch_or(std::hint::black_box(0), Ordering::Relaxed);
        word.fetch_or(bit, Ordering::Release);
    }

    #[inline(always)]
    fn solved_bit_pos(zkey: u64) -> (usize, u64) {
        let b = (((zkey >> 17) ^ (zkey >> 41)) as usize) & SOLVED_BIT_MASK;
        (b >> 6, 1u64 << (b & 63))
    }

    #[inline(always)]
    fn mark_solved_bit(&self, zkey: u64) {
        let (w_idx, mask) = Self::solved_bit_pos(zkey);
        let word = unsafe { self.solved_bits.get_unchecked(w_idx) };
        if (word.load(Ordering::Relaxed) & mask) == 0 {
            word.fetch_or(mask, Ordering::Relaxed);
        }
    }

    #[inline(always)]
    fn may_be_solved(&self, zkey: u64) -> bool {
        let (w_idx, mask) = Self::solved_bit_pos(zkey);
        let word = unsafe { self.solved_bits.get_unchecked(w_idx) };
        (word.load(Ordering::Relaxed) & mask) != 0
    }

    pub fn clear(&self) {
        for (i, slot) in self.shards.iter().enumerate() {
            if slot.dirty.load(Ordering::Relaxed) {
                slot.dirty.store(false, Ordering::Relaxed);
                slot.has_prot.store(false, Ordering::Relaxed);
                self.shard_gc_gens[i].store(0, Ordering::Relaxed);
                let mut g = slot.lock_write();
                slot.clear(g.meta());
            }
        }
        for w in self.solved_bits.iter() {
            if w.load(Ordering::Relaxed) != 0 {
                w.store(0, Ordering::Relaxed);
            }
        }
        self.shard_gcs.store(0, Ordering::Relaxed);
        self.gc_max_level.store(0, Ordering::Relaxed);
        self.root_pn.store(1, Ordering::Relaxed);
    }

    pub fn clear_protect(&self) {
        for slot in self.shards.iter() {
            if slot.has_prot.load(Ordering::Relaxed) {
                slot.has_prot.store(false, Ordering::Relaxed);
                let mut g = slot.lock_write();
                slot.clear_protect(g.meta());
            }
        }
    }

    #[inline(always)]
    pub fn gc_num(&self) -> u16 {
        let g = self.shard_gcs.load(Ordering::Relaxed);
        ((g / (self.n_shards as u64)).min(u16::MAX as u64)) as u16
    }

    #[inline(always)]
    pub fn any_gc(&self) -> bool {
        self.shard_gcs.load(Ordering::Relaxed) >= (self.n_shards as u64 / 4)
    }

    #[inline(always)]
    pub fn gc_max_level(&self) -> u16 {
        self.gc_max_level.load(Ordering::Relaxed)
    }

    #[inline(always)]
    pub fn prefetch(&self, zkey: u64) {
        #[cfg(target_arch = "x86_64")]
        unsafe {
            let nz = norm_zkey(zkey);
            let idx = self.shard_idx(zkey);
            let b = (nz & self.shard_mask) as usize;
            let shard_ptr = self.shards.as_ptr().add(idx) as *const i8;
            core::arch::x86_64::_mm_prefetch(shard_ptr, core::arch::x86_64::_MM_HINT_T0);
            let bucket_offset = (idx << self.buckets_shift) | b;
            core::arch::x86_64::_mm_prefetch(
                self.buckets_base.0.add(bucket_offset) as *const i8,
                core::arch::x86_64::_MM_HINT_T0,
            );
        }
        #[cfg(not(target_arch = "x86_64"))]
        {
            let _ = (zkey, self.buckets_base, self.buckets_shift);
        }
    }

    #[inline(always)]
    pub fn lookup(&self, ks: &KeyState, mv: &mut MvItem, tn: Turn) {
        let slot = self.shard_slot(self.shard_idx(ks.zkey));
        let res = slot.read_seqlock(|s| s.lookup_readonly_opt(ks, tn, false));
        res.apply(mv);
    }

    #[inline(always)]
    pub(crate) fn lookup_cached(
        &self,
        ks: &KeyState,
        mv: &mut MvItem,
        tn: Turn,
        local_tt: &mut [LocalTerminalEntry],
    ) {
        let qmk = ks.mkey_for(tn);
        let tt_i = local_tt_idx(ks.zkey, qmk);
        let cached = unsafe { *local_tt.get_unchecked(tt_i) };
        let s_idx = self.shard_idx(ks.zkey);
        let cur_gen = unsafe { self.shard_gc_gens.get_unchecked(s_idx) }.load(Ordering::Relaxed);
        if cached.zkey == ks.zkey
            && cached.query_mkey.0 == qmk.0
            && cached.gc_gen == cur_gen
            && (cached.tdata.pn == 0 || cached.tdata.dn == 0)
        {
            mv.tdata = cached.tdata;
            mv.mkey = cached.res_mkey;
            mv.set_search(true);
            mv.set_hinc(false);
            mv.set_inc(false);
            mv.nouse = 0;
            mv.nouse2 = 0;
            return;
        }
        let slot = self.shard_slot(s_idx);
        let res = slot.read_seqlock(|s| s.lookup_readonly_opt(ks, tn, false));
        res.apply(mv);
        if mv.tdata.dn == 0 || (mv.tdata.pn == 0 && !mv.inc()) {
            let gen_after =
                unsafe { self.shard_gc_gens.get_unchecked(s_idx) }.load(Ordering::Relaxed);
            if gen_after == cur_gen {
                unsafe {
                    *local_tt.get_unchecked_mut(tt_i) = LocalTerminalEntry {
                        zkey: ks.zkey,
                        query_mkey: qmk,
                        res_mkey: mv.mkey,
                        tdata: mv.tdata,
                        gc_gen: cur_gen,
                    };
                }
            }
        }
    }

    #[inline]
    pub(crate) fn sync_sibling_cached(
        &self,
        ks: &KeyState,
        mv: &mut MvItem,
        tn: Turn,
        flag: bool,
        local_tt: &mut [LocalTerminalEntry],
    ) {
        let s_idx = self.shard_idx(ks.zkey);
        if !flag {
            let qmk = ks.mkey_for(tn);
            let tt_i = local_tt_idx(ks.zkey, qmk);
            let cached = unsafe { *local_tt.get_unchecked(tt_i) };
            let cur_gen =
                unsafe { self.shard_gc_gens.get_unchecked(s_idx) }.load(Ordering::Relaxed);
            if cached.zkey == ks.zkey
                && cached.query_mkey.0 == qmk.0
                && cached.gc_gen == cur_gen
                && (cached.tdata.pn == 0 || cached.tdata.dn == 0)
            {
                mv.tdata = cached.tdata;
                mv.mkey = cached.res_mkey;
                mv.set_search(true);
                mv.set_hinc(false);
                mv.set_inc(false);
                mv.nouse = 0;
                mv.nouse2 = 0;
                return;
            }
            let slot = self.shard_slot(s_idx);
            let nz = norm_zkey(ks.zkey);
            let found_res = slot.read_seqlock(|s| {
                if s.find_zloc_opt(nz)?.is_none() {
                    return Some(None);
                }
                Some(Some(s.lookup_readonly_opt(ks, tn, false)?))
            });
            let Some(res) = found_res else {
                return;
            };
            let old_pn = mv.tdata.pn;
            let old_dn = mv.tdata.dn;
            let was_searched = mv.search();
            res.apply(mv);
            if !mv.search() && !was_searched {
                mv.tdata.pn = mv.tdata.pn.max(old_pn);
                mv.tdata.dn = mv.tdata.dn.max(old_dn);
            } else if mv.tdata.dn == 0 || (mv.tdata.pn == 0 && !mv.inc()) {
                let gen_after =
                    unsafe { self.shard_gc_gens.get_unchecked(s_idx) }.load(Ordering::Relaxed);
                if gen_after == cur_gen {
                    unsafe {
                        *local_tt.get_unchecked_mut(tt_i) = LocalTerminalEntry {
                            zkey: ks.zkey,
                            query_mkey: qmk,
                            res_mkey: mv.mkey,
                            tdata: mv.tdata,
                            gc_gen: cur_gen,
                        };
                    }
                }
            }
        } else {
            let slot = self.shard_slot(s_idx);
            let nz = norm_zkey(ks.zkey);
            let found_res = slot.read_seqlock(|s| {
                if s.find_zloc_opt(nz)?.is_none() {
                    return Some(None);
                }
                Some(Some(s.lookup_readonly_opt(ks, tn, true)?))
            });
            let Some(res) = found_res else {
                return;
            };
            let old_pn = mv.tdata.pn;
            let old_dn = mv.tdata.dn;
            let was_searched = mv.search();
            res.apply(mv);
            if !mv.search() && !was_searched {
                mv.tdata.pn = mv.tdata.pn.max(old_pn);
                mv.tdata.dn = mv.tdata.dn.max(old_dn);
            }
        }
    }

    #[inline(always)]
    pub fn make_tree_lookup(&self, ks: &KeyState, mv: &mut MvItem, tn: Turn) {
        let slot = self.shard_slot(self.shard_idx(ks.zkey));
        let res = slot.read_seqlock(|s| s.lookup_readonly_opt(ks, tn, true));
        res.apply(mv);
    }

    #[inline(always)]
    pub fn is_terminal(&self, ks: &KeyState, tn: Turn) -> bool {
        if !self.may_be_solved(ks.zkey) {
            return false;
        }
        let slot = self.shard_slot(self.shard_idx(ks.zkey));
        slot.read_seqlock(|s| s.is_terminal_opt(ks, tn))
    }

    #[inline(always)]
    pub fn hs_lookup_raw(&self, ks: &KeyState, tn: Turn) -> i8 {
        let slot = self.shard_slot(self.shard_idx(ks.zkey));
        slot.read_seqlock(|s| s.hs_lookup_opt(ks, tn)).0
    }

    #[inline]
    pub(crate) fn hs_lookup_cached(
        &self,
        ks: &KeyState,
        tn: Turn,
        local_tt: &mut [LocalTerminalEntry],
    ) -> i8 {
        let qmk = ks.mkey_for(tn);
        let tt_i = local_tt_idx(ks.zkey, qmk);
        let cached = unsafe { *local_tt.get_unchecked(tt_i) };
        let s_idx = self.shard_idx(ks.zkey);
        let cur_gen = unsafe { self.shard_gc_gens.get_unchecked(s_idx) }.load(Ordering::Relaxed);
        if cached.zkey == ks.zkey && cached.query_mkey.0 == qmk.0 && cached.gc_gen == cur_gen {
            if cached.tdata.pn == 0 {
                return 1;
            }
            if cached.tdata.dn == 0 {
                return 0;
            }
        }
        let slot = self.shard_slot(s_idx);
        let (res, cache_info) = slot.read_seqlock(|s| s.hs_lookup_opt(ks, tn));
        if let Some((res_mk, td, can_cache)) = cache_info {
            if can_cache {
                let gen_after =
                    unsafe { self.shard_gc_gens.get_unchecked(s_idx) }.load(Ordering::Relaxed);
                if gen_after == cur_gen {
                    unsafe {
                        *local_tt.get_unchecked_mut(tt_i) = LocalTerminalEntry {
                            zkey: ks.zkey,
                            query_mkey: qmk,
                            res_mkey: res_mk,
                            tdata: td,
                            gc_gen: cur_gen,
                        };
                    }
                }
            }
        }
        res
    }

    #[inline]
    pub fn update(&self, ks: &KeyState, mv: &mut MvItem, tn: Turn) {
        self.update_inner(ks, mv, tn, false, false, |_, _| false);
    }

    #[inline]
    pub(crate) fn update_with_path(
        &self,
        ks: &KeyState,
        mv: &mut MvItem,
        tn: Turn,
        in_path: impl Fn(u64, MKey) -> bool,
    ) {
        self.update_inner(ks, mv, tn, false, false, in_path);
    }

    #[inline]
    pub(crate) fn update_leaf_hint(
        &self,
        ks: &KeyState,
        mv: &mut MvItem,
        tn: Turn,
        in_path: impl Fn(u64, MKey) -> bool,
        try_only: bool,
    ) {
        let rpn = self.root_pn.load(Ordering::Relaxed);
        if mv.tdata.pn > INFINATE.saturating_sub(rpn) {
            return;
        }
        let idx = self.shard_idx(ks.zkey);
        let slot = self.shard_slot(idx);
        if slot.already_has_exact_unknown(ks, mv.tdata, tn) {
            return;
        }
        let mut guard = if try_only {
            match slot.try_lock_write() {
                Some(g) => g,
                None => return,
            }
        } else {
            slot.lock_write()
        };
        if !slot.dirty.load(Ordering::Relaxed) {
            slot.dirty.store(true, Ordering::Relaxed);
        }
        let meta = guard.meta();
        if slot.needs_gc(meta) {
            let lv = slot.gc(meta, rpn);
            self.shard_gc_gens[idx].fetch_add(1, Ordering::Relaxed);
            self.shard_gcs.fetch_add(1, Ordering::Relaxed);
            self.gc_max_level.fetch_max(lv, Ordering::Relaxed);
        }
        slot.fumei_update(meta, ks, mv, tn, in_path);
    }

    #[inline]
    pub fn make_tree_update(&self, ks: &KeyState, mv: &mut MvItem, tn: Turn) {
        self.update_inner(ks, mv, tn, true, false, |_, _| false);
    }

    #[inline]
    pub fn make_tree_simple_update(&self, ks: &KeyState, mv: &mut MvItem, tn: Turn) {
        self.update_inner(ks, mv, tn, true, true, |_, _| false);
    }

    fn update_inner(
        &self,
        ks: &KeyState,
        mv: &mut MvItem,
        tn: Turn,
        flag: bool,
        force_sh: bool,
        in_path: impl Fn(u64, MKey) -> bool,
    ) {
        let rpn = self.root_pn.load(Ordering::Relaxed);
        if mv.tdata.pn > INFINATE.saturating_sub(rpn) && mv.tdata.dn != 0 {
            return;
        }
        // Lock-free skip when a concurrent worker already proved/disproved this position:
        // avoids acquiring `write_lock` and bumping `seq` on hot already-solved spine shards.
        let idx = self.shard_idx(ks.zkey);
        let slot = self.shard_slot(idx);
        if mv.tdata.pn != 0 && mv.tdata.dn != 0 {
            if self.may_be_solved(ks.zkey) && slot.read_seqlock(|s| s.is_terminal_opt(ks, tn)) {
                let res = slot.read_seqlock(|s| s.lookup_readonly_opt(ks, tn, false));
                res.apply(mv);
                return;
            }
            if slot.already_has_exact_unknown(ks, mv.tdata, tn) {
                return;
            }
        }
        if !slot.dirty.load(Ordering::Relaxed) {
            slot.dirty.store(true, Ordering::Relaxed);
        }
        if flag && mv.tdata.pn == 0 && !slot.has_prot.load(Ordering::Relaxed) {
            slot.has_prot.store(true, Ordering::Relaxed);
        }
        let mut guard = slot.lock_write();
        let meta = guard.meta();
        if slot.needs_gc(meta) {
            let lv = slot.gc(meta, rpn);
            self.shard_gc_gens[idx].fetch_add(1, Ordering::Relaxed);
            self.shard_gcs.fetch_add(1, Ordering::Relaxed);
            self.gc_max_level.fetch_max(lv, Ordering::Relaxed);
        }
        if mv.tdata.pn == 0 {
            slot.tsumi_update(meta, ks, mv, tn, flag, force_sh);
            self.mark_solved_bit(ks.zkey);
        } else if mv.tdata.dn == 0 {
            slot.fudumi_update(meta, ks, mv, tn);
            self.mark_solved_bit(ks.zkey);
        } else {
            slot.fumei_update(meta, ks, mv, tn, in_path);
        }
    }

    pub fn enter_current<'a>(&'a self, ks: &KeyState, cpn: u16, tn: Turn) -> CurrentGuard<'a> {
        let idx = self.shard_idx(ks.zkey);
        let slot = self.shard_slot(idx);
        if !slot.dirty.load(Ordering::Relaxed) {
            slot.dirty.store(true, Ordering::Relaxed);
        }
        {
            let mut guard = slot.lock_write();
            slot.enter_current(guard.meta(), ks, cpn, tn);
        }
        CurrentGuard {
            tbase: self,
            zkey: ks.zkey,
            mkey: ks.mkey_for(tn),
        }
    }

    pub fn make_tree_set_item(
        &self,
        ks: &KeyState,
        tn: Turn,
        protect: bool,
    ) -> Option<(u64, MKey)> {
        let idx = self.shard_idx(ks.zkey);
        let slot = self.shard_slot(idx);
        if protect && !slot.has_prot.load(Ordering::Relaxed) {
            slot.has_prot.store(true, Ordering::Relaxed);
        }
        let mut guard = slot.lock_write();
        slot.make_tree_set_item(guard.meta(), ks, tn, protect)
    }

    pub fn clear_current_token(&self, token: Option<(u64, MKey)>) {
        if let Some((zkey, mkey)) = token {
            let idx = self.shard_idx(zkey);
            let slot = self.shard_slot(idx);
            let mut guard = slot.lock_write();
            slot.leave_current(guard.meta(), zkey, mkey);
        }
    }
}

pub struct CurrentGuard<'a> {
    tbase: &'a SharedTBase,
    zkey: u64,
    mkey: MKey,
}

impl<'a> Drop for CurrentGuard<'a> {
    #[inline]
    fn drop(&mut self) {
        let idx = self.tbase.shard_idx(self.zkey);
        let slot = unsafe { self.tbase.shards.get_unchecked(idx) };
        let mut guard = slot.lock_write();
        slot.leave_current(guard.meta(), self.zkey, self.mkey);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn test_seqlock_tbase_concurrent_stress() {
        let tb = Arc::new(SharedTBase::new(16));
        let mut handles = Vec::new();
        for tid in 0..8usize {
            let tb = Arc::clone(&tb);
            handles.push(std::thread::spawn(move || {
                let mut ltt = vec![LocalTerminalEntry::EMPTY; LOCAL_TT_SIZE].into_boxed_slice();
                for step in 0..4000u64 {
                    let zkey = (step.wrapping_mul(0x9e37_79b9_7f4a_7c15) ^ ((tid as u64) << 7))
                        & 0xFFFF_FFFF_FFFF_FF00
                        | (step & 0xFF);
                    let mk = MKey::new((step % 3) as u8, ((step >> 2) % 2) as u8, 0, 0, 0, 0, 0);
                    let ks = KeyState {
                        zkey,
                        mkey: [mk, MKey::EMPTY],
                        turn: Turn::Sente,
                        count: (step % 40) as u16,
                    };
                    let mut mv = MvItem::empty_root();
                    tb.lookup_cached(&ks, &mut mv, Turn::Sente, &mut ltt);
                    if step % 5 == 0 {
                        mv.tdata = TData {
                            pn: 0,
                            dn: INFINATE,
                            sh: ((step % 15) as u16) * 2 + 1,
                        };
                        mv.mkey = mk;
                        tb.update(&ks, &mut mv, Turn::Sente);
                    } else if step % 7 == 0 {
                        mv.tdata = TData::FUDUMI;
                        mv.mkey = mk;
                        tb.update(&ks, &mut mv, Turn::Sente);
                    } else {
                        mv.tdata = TData {
                            pn: ((step % 20) as u16) + 1,
                            dn: ((step % 15) as u16) + 1,
                            sh: 1,
                        };
                        tb.update(&ks, &mut mv, Turn::Sente);
                    }
                    let mut check = MvItem::empty_root();
                    tb.lookup(&ks, &mut check, Turn::Sente);
                }
            }));
        }
        for h in handles {
            h.join().unwrap();
        }
        tb.clear();
    }

    #[test]
    fn test_zeroed_arena_2mb_align_and_chains() {
        let tb = SharedTBase::new(16);
        assert_eq!((tb.buckets_base.0 as usize) % HUGEPAGE_ALIGN, 0);
        assert_eq!(std::mem::size_of::<ShardSlot>(), 192);

        // Untouched all-zero buckets (including zkey == 0 -> norm_zkey == 1) must cleanly miss.
        for raw_zkey in [0u64, 1, 0xDEAD_BEEF_CAFE_0000] {
            let ks = KeyState {
                zkey: raw_zkey,
                mkey: [MKey::EMPTY, MKey::EMPTY],
                turn: Turn::Sente,
                count: 0,
            };
            let mut mv = MvItem::empty_root();
            tb.lookup(&ks, &mut mv, Turn::Sente);
            assert_eq!(mv.tdata.pn, 1);
            assert_eq!(mv.tdata.dn, 1);
            assert!(!tb.is_terminal(&ks, Turn::Sente));
            assert_eq!(tb.hs_lookup_raw(&ks, Turn::Sente), -1);
        }

        // Force zover collisions (same shard_idx + same bucket index), mpool chains, and tpool chains.
        let base_zkey = 0x1234_5678_0000_0005u64;
        for c in 0..4u64 {
            // Shifting by (buckets_shift) while keeping bits 32..63 fixed collides in same shard + bucket!
            let zkey = base_zkey + (c << (tb.buckets_shift + 1));
            assert_eq!(tb.shard_idx(zkey), tb.shard_idx(base_zkey));
            assert_eq!(zkey & tb.shard_mask, base_zkey & tb.shard_mask);
            for p in 0..3u8 {
                let mk = MKey::new(p, 0, 0, 0, 0, 0, 0);
                for dp in [2u16, 4, 6, 8] {
                    let ks = KeyState {
                        zkey,
                        mkey: [mk, MKey::EMPTY],
                        turn: Turn::Sente,
                        count: dp,
                    };
                    let mut mv = MvItem::empty_root();
                    mv.tdata = TData {
                        pn: dp + 3,
                        dn: dp + 5,
                        sh: 1,
                    };
                    tb.update(&ks, &mut mv, Turn::Sente);
                }
            }
        }
        tb.clear();
        // Verify clean state after clear() and re-insertion.
        let ks = KeyState {
            zkey: base_zkey,
            mkey: [MKey::new(1, 0, 0, 0, 0, 0, 0), MKey::EMPTY],
            turn: Turn::Sente,
            count: 2,
        };
        let mut mv = MvItem::empty_root();
        tb.lookup(&ks, &mut mv, Turn::Sente);
        assert_eq!(mv.tdata, TData { pn: 1, dn: 1, sh: 0 });
    }

    #[test]
    fn test_mpool_starvation_gc_and_multi_mkey_exact_unknown() {
        let tb = SharedTBase::new(16);
        assert_eq!(tb.n_shards, 8192);
        let base_zkey = 0x5555_6666_0000_0000u64;
        let s_idx = tb.shard_idx(base_zkey);
        let slot = tb.shard_slot(s_idx);

        // 1. Insert 24 inline single-card low-pn entries so delete_num > gc_target would trip early
        //    even if zero mpool slots were freed.
        for b in 1..=24u64 {
            let ks = KeyState {
                zkey: base_zkey | b,
                mkey: [MKey::EMPTY, MKey::EMPTY],
                turn: Turn::Sente,
                count: 1,
            };
            let mut mv = MvItem::empty_root();
            mv.tdata = TData { pn: 2, dn: 2, sh: 1 };
            tb.update(&ks, &mut mv, Turn::Sente);
        }

        // 2. Exhaust mpool in shard `s_idx` using incomparable multi-MKey entries (even dp, high pn=12)
        //    while `meta.num < sz_elm`, plus test `already_has_exact_unknown` across overflow_mc!
        let m_cap = slot.mpool_cap as usize;
        let mut inserted_m = 0usize;
        for bucket in 30..36u64 {
            let zkey = base_zkey | bucket;
            for k in 0..=18u8 {
                if inserted_m > m_cap + 6 {
                    break;
                }
                // Incomparable hands: k pawns + (18 - k) lances? Max lances is 4, so vary (fu, ky, ke, gi)
                let fu = k;
                let ky = (18 - k) % 5;
                let ke = ((18 - k) / 5) % 5;
                let mk = MKey::new(fu, ky, ke, 0, 0, 0, 0);
                let ks = KeyState {
                    zkey,
                    mkey: [mk, MKey::EMPTY],
                    turn: Turn::Sente,
                    count: 4,
                };
                let mut mv = MvItem::empty_root();
                mv.tdata = TData { pn: 12, dn: 12, sh: 1 };
                tb.update(&ks, &mut mv, Turn::Sente);
                inserted_m += 1;
            }
        }

        // Verify shard recovered mpool capacity cleanly without ratcheting gc_max_level to INFINATE-1!
        {
            let mut g = slot.lock_write();
            let meta = g.meta();
            assert!(!slot.needs_gc(meta), "needs_gc must be false after mpool GC recovery");
            assert!(slot.m_avail(meta) >= 8, "m_avail={}", slot.m_avail(meta));
            assert!(meta.gc_max_level < 64, "gc_max_level={}", meta.gc_max_level);
        }

        // Verify `already_has_exact_unknown` hits across inline + first 4 mpool cards!
        let test_z = base_zkey | 40;
        let hands = [
            MKey::new(4, 0, 0, 0, 0, 0, 0),
            MKey::new(3, 1, 0, 0, 0, 0, 0),
            MKey::new(2, 2, 0, 0, 0, 0, 0),
            MKey::new(1, 3, 0, 0, 0, 0, 0),
            MKey::new(0, 4, 0, 0, 0, 0, 0),
        ];
        for (i, &mk) in hands.iter().enumerate() {
            let ks = KeyState {
                zkey: test_z,
                mkey: [mk, MKey::EMPTY],
                turn: Turn::Sente,
                count: 6,
            };
            let mut mv = MvItem::empty_root();
            mv.tdata = TData {
                pn: 5 + i as u16,
                dn: 7 + i as u16,
                sh: 1,
            };
            tb.update(&ks, &mut mv, Turn::Sente);
        }
        for (i, &mk) in hands.iter().enumerate() {
            let ks = KeyState {
                zkey: test_z,
                mkey: [mk, MKey::EMPTY],
                turn: Turn::Sente,
                count: 6,
            };
            let td = TData {
                pn: 5 + i as u16,
                dn: 7 + i as u16,
                sh: 1,
            };
            assert!(slot.already_has_exact_unknown(&ks, td, Turn::Sente));
            let td_diff = TData {
                pn: 6 + i as u16,
                dn: 7 + i as u16,
                sh: 1,
            };
            assert!(!slot.already_has_exact_unknown(&ks, td_diff, Turn::Sente));
        }

        // Dynamic shard scaling check without allocating multi-GiB virtual arenas in CI.
        assert_eq!(SharedTBase::shard_count_for_hash_mb(16), 8192);
        assert_eq!(SharedTBase::shard_count_for_hash_mb(2048), 8192);
        assert_eq!(SharedTBase::shard_count_for_hash_mb(4096), 16384);
        assert_eq!(SharedTBase::shard_count_for_hash_mb(16384), 32768);
    }
}
