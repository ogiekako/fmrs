use std::{
    cell::UnsafeCell,
    ops::Range,
    ptr::NonNull,
    sync::{
        atomic::{AtomicU8, AtomicUsize, Ordering},
        Mutex,
    },
};
#[cfg(all(not(target_arch = "wasm32"), target_os = "linux"))]
use std::{collections::HashMap, sync::OnceLock};

/// Coarse sub-phase of `advance_parallel_filtered`, published for an optional
/// out-of-band progress heartbeat (see the command-side ticker thread). A
/// single global slot: with many seeds in flight the last writer wins, which
/// is exactly what we want in the deep tail where one huge seed dominates and
/// a single slow step otherwise looks frozen. Writing it is one relaxed store
/// at each phase boundary (a handful per step), so it is always on; nothing
/// reads it unless the ticker thread is spawned.
static PROGRESS_PHASE: AtomicU8 = AtomicU8::new(0);

/// 0=idle, 1=P (candidate generation), 2=C (candidate collect/extend),
/// 3=V (uniqueness verification waves), 4=F (memo shrink/finalize).
#[inline]
fn set_progress_phase(p: u8) {
    PROGRESS_PHASE.store(p, Ordering::Relaxed);
}

/// Current phase as a single display char (`.` = idle/between steps).
pub fn progress_phase_char() -> char {
    match PROGRESS_PHASE.load(Ordering::Relaxed) {
        1 => 'P',
        2 => 'C',
        3 => 'V',
        4 => 'F',
        _ => '.',
    }
}

use anyhow::bail;
use log::{debug, info};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::BinaryHeap;

use crate::{
    nohash::{NoHashMap64, NoHashSet64},
    piece::{Color, Kind},
    position::{
        advance::advance::advance_aux, position::PositionAux, previous, BitBoard, Movement,
        Position, UndoMove,
    },
    solve::standard_solve::standard_solve,
};

type Memo = ShardedFlatMemo;

// ===== ShardedFlatMemo: lock-free sharded hash table for backward search =====
//
// Designed for backward search workload:
//  * Reads during Phase 2 are massively concurrent and read-only on the base
//    (writes go to per-thread NoHashMap64 deltas). With this table, reads
//    require zero atomic ops — just an array index and linear probe.
//  * Merges happen single-threaded *per shard* but in parallel *across shards*,
//    giving 8-way merge parallelism without any locks.
//  * `alloc_zeroed` produces lazy zero pages on Linux (mmap MAP_ANONYMOUS),
//    so pre-allocating capacity that won't be filled costs no resident memory.
//  * `madvise(MADV_HUGEPAGE)` requests transparent huge pages, cutting TLB
//    pressure for the multi-GB tables.
//  * Empty-slot sentinel is `u64::MAX` (not 0) — but the slot vector is
//    allocated zeroed, so a separate `initialized` flag isn't needed: we
//    treat key==0 OR key==SENTINEL_INVALID as empty during the rare case.
//    Actually we use 0 as the empty sentinel so zeroed pages = empty.
//    Inserts of key==0 are silently skipped (probability ≈ 2^-64).

const SHARD_BITS: u32 = 6;
const NUM_SHARDS: usize = 1 << SHARD_BITS;
const SHARD_SHIFT: u32 = 64 - SHARD_BITS;
const FLAT_EMPTY_KEY: u64 = 0;

/// Sentinel marker for INF_START/INF_END in the 8-bit packed StepRange.
/// Real step values must fit in 0..=253 (smoke search bounds, per design).
/// Backward search beyond ~250 plies would saturate.
const PACK_SENTINEL_INF_START: u8 = 254;
const PACK_SENTINEL_INF_END: u8 = 255;

/// Pack a StepRange into u32 (4 × u8 bytes). Caller must ensure non-INF step
/// values fit in u8 (0..=253). For smoke searches with mate_in ≪ 256 this
/// always holds; backward search at deeper plies should not use this layout.
#[inline(always)]
fn pack_step_range(sr: StepRange) -> u32 {
    let pack_start = |v: u16| -> u8 {
        if v >= INF_START {
            PACK_SENTINEL_INF_START
        } else {
            debug_assert!(v <= 253, "StepRange start {} exceeds packed range", v);
            v as u8
        }
    };
    let pack_end = |v: u16| -> u8 {
        if v >= INF_END {
            PACK_SENTINEL_INF_END
        } else {
            debug_assert!(v <= 253, "StepRange end {} exceeds packed range", v);
            v as u8
        }
    };
    let bytes = [
        pack_start(sr.next_start),
        pack_end(sr.next_end),
        pack_start(sr.shortest_start),
        pack_end(sr.shortest_end),
    ];
    u32::from_le_bytes(bytes)
}

#[inline(always)]
fn unpack_step_range(packed: u32) -> StepRange {
    let bytes = packed.to_le_bytes();
    let unpack_start = |b: u8| -> u16 {
        if b == PACK_SENTINEL_INF_START {
            INF_START
        } else {
            b as u16
        }
    };
    let unpack_end = |b: u8| -> u16 {
        if b == PACK_SENTINEL_INF_END {
            INF_END
        } else {
            b as u16
        }
    };
    StepRange {
        next_start: unpack_start(bytes[0]),
        next_end: unpack_end(bytes[1]),
        shortest_start: unpack_start(bytes[2]),
        shortest_end: unpack_end(bytes[3]),
    }
}

struct ShardedFlatMemo {
    shards: Box<[FlatShard]>,
}

struct FlatShard {
    inner: UnsafeCell<FlatShardInner>,
    len: AtomicUsize,
}

/// SoA layout: keys probed during lookups, values read only on hit.
/// Effective per-slot storage is 12 bytes (u64 + u32 packed StepRange) vs.
/// the legacy 16-byte AoS layout. The split also doubles probe-step
/// cache-line density (8 keys per 64B line vs. 4 in the AoS layout).
struct FlatShardInner {
    keys: MmapSlice<u64>,
    values: MmapSlice<u32>,
    mask: usize,
    capacity_threshold: usize,
}

/// Slice backed by an anonymous `mmap` allocation on Unix targets.
///
/// Guarantees 2 MiB virtual-address alignment so the kernel can satisfy
/// `MADV_HUGEPAGE` at fault time (one 2 MiB huge page per block, never split
/// across NUMA nodes), while `mbind(MPOL_INTERLEAVE)` distributes those huge
/// pages across NUMA nodes for balanced bandwidth on multi-socket machines.
///
/// Non-Unix targets such as `wasm32-unknown-unknown` fall back to the global
/// allocator because `libc::mmap` is not available there.
struct MmapSlice<T> {
    ptr: NonNull<T>,
    len: usize,
    #[cfg(target_family = "unix")]
    mmap_size: usize, // rounded-up byte length passed to mmap / munmap
}

unsafe impl<T: Send> Send for MmapSlice<T> {}
unsafe impl<T: Sync> Sync for MmapSlice<T> {}

// ===== mmap region pool (Linux) =====
//
// 各 seed の memo は `FlatShard::grow` の倍々再確保や seed 完了時の drop で
// 巨大 mmap 領域を頻繁に解放する。`munmap` は同一アドレス空間を共有する全
// CPU へ同期 TLB shootdown IPI をブロードキャストするため、128 スレッド環境
// では大量の shootdown 嵐となり実効並列度が 1/3 まで落ちる。
//
// 対策: drop 時に `munmap` せず `madvise(MADV_FREE)` してサイズ別プールへ
// 返却し、同サイズの確保要求で再利用する。
//  * `MADV_FREE` 済みページはメモリ圧力時にカーネルが回収できる (swap 不要
//    の lazy reclaim) ため、OOM 挙動は `munmap` と実質同じ = OOM 安全。
//  * `MADV_FREE` の TLB flush は遅延・バッチ化され、`munmap` のような同期
//    全 CPU IPI を発生させない。
//  * 再利用前に圧力がなければ物理ページは常駐したまま → 再ゼロのみで復帰し
//    mmap/munmap/shootdown ゼロ。
//  * VMA 属性 (MADV_HUGEPAGE / mbind) は MADV_FREE を跨いで保持されるため
//    再利用時の再設定は不要。
//
// プールはサイズクラスごとに上限を設け、超過分のみ実 `munmap`(まれ)。
#[cfg(all(not(target_arch = "wasm32"), target_os = "linux"))]
mod mmap_pool {
    use super::{HashMap, Mutex, OnceLock};

    /// 同一サイズクラスで保持する最大領域数。これを超えた解放は実 munmap。
    const PER_CLASS_CAP: usize = 64;

    static POOL: OnceLock<Mutex<HashMap<usize, Vec<usize>>>> = OnceLock::new();

    fn pool() -> &'static Mutex<HashMap<usize, Vec<usize>>> {
        POOL.get_or_init(|| Mutex::new(HashMap::new()))
    }

    /// `mmap_size` バイトの再利用可能領域があれば先頭アドレスを返す。
    pub(super) fn take(mmap_size: usize) -> Option<usize> {
        let mut guard = pool().lock().unwrap();
        guard.get_mut(&mmap_size).and_then(|v| v.pop())
    }

    /// 領域をプールへ返却する。プールが満杯なら呼び出し側へ false を返し、
    /// 呼び出し側が munmap する。
    pub(super) fn put(addr: usize, mmap_size: usize) -> bool {
        let mut guard = pool().lock().unwrap();
        let v = guard.entry(mmap_size).or_default();
        if v.len() >= PER_CLASS_CAP {
            return false;
        }
        v.push(addr);
        true
    }
}

#[cfg(target_family = "unix")]
impl<T> Drop for MmapSlice<T> {
    fn drop(&mut self) {
        let addr = self.ptr.as_ptr() as usize;
        #[cfg(all(not(target_arch = "wasm32"), target_os = "linux"))]
        unsafe {
            // MADV_DONTNEED: 匿名マップはゼロフィルオンデマンドで返す (カーネル保証)。
            // プールから再利用する際に write_bytes が不要になり、キー配列の
            // memset コスト (実測 ~6%) を削除できる。
            // MADV_FREE と同様に TLB shootdown を避けつつ OOM 圧力時に物理ページを
            // 即時解放できるため OOM 安全性は変わらない。
            libc::madvise(
                self.ptr.as_ptr().cast(),
                self.mmap_size,
                libc::MADV_DONTNEED,
            );
            if mmap_pool::put(addr, self.mmap_size) {
                return;
            }
            // プール満杯時のみ実 munmap (まれ)。
            libc::munmap(self.ptr.as_ptr().cast(), self.mmap_size);
        }
        #[cfg(not(all(not(target_arch = "wasm32"), target_os = "linux")))]
        unsafe {
            libc::munmap(self.ptr.as_ptr().cast(), self.mmap_size);
        }
    }
}

#[cfg(not(target_family = "unix"))]
impl<T> Drop for MmapSlice<T> {
    fn drop(&mut self) {
        unsafe {
            let layout = std::alloc::Layout::array::<T>(self.len).unwrap();
            std::alloc::dealloc(self.ptr.as_ptr() as *mut u8, layout);
        }
    }
}

impl<T> std::ops::Deref for MmapSlice<T> {
    type Target = [T];
    fn deref(&self) -> &[T] {
        unsafe { std::slice::from_raw_parts(self.ptr.as_ptr(), self.len) }
    }
}

impl<T> std::ops::DerefMut for MmapSlice<T> {
    fn deref_mut(&mut self) -> &mut [T] {
        unsafe { std::slice::from_raw_parts_mut(self.ptr.as_ptr(), self.len) }
    }
}

unsafe impl Sync for ShardedFlatMemo {}
unsafe impl Send for ShardedFlatMemo {}
unsafe impl Sync for FlatShard {}
unsafe impl Send for FlatShard {}

#[inline(always)]
fn shard_index(key: u64) -> usize {
    (key >> SHARD_SHIFT) as usize
}

fn alloc_slot_arrays(size: usize) -> (MmapSlice<u64>, MmapSlice<u32>) {
    // MADV_DONTNEED on pool return zeroes pages automatically; no write_bytes needed.
    (alloc_zeroed_slice::<u64>(size), alloc_zeroed_slice::<u32>(size))
}

#[cfg(not(target_arch = "wasm32"))]
#[cfg(target_family = "unix")]
fn alloc_zeroed_slice<T: Copy>(len: usize) -> MmapSlice<T> {
    debug_assert!(len > 0);
    debug_assert!(len.is_power_of_two());

    const PAGE_2MB: usize = 2 * 1024 * 1024;
    let size_bytes = len * std::mem::size_of::<T>();
    // Round up to 2 MiB so the entire allocation fits in whole huge pages.
    let mmap_size = (size_bytes + PAGE_2MB - 1) & !(PAGE_2MB - 1);

    // Reuse a pooled region of the same size if available: skips mmap, the
    // alignment-trim munmaps, MADV_HUGEPAGE and mbind (all persist on the VMA),
    // and — most importantly — avoids the munmap TLB-shootdown storm.
    // MADV_DONTNEED on pool return guarantees zero-fill-on-demand for anonymous
    // mappings (Linux kernel promise), so no explicit write_bytes is needed here.
    #[cfg(all(not(target_arch = "wasm32"), target_os = "linux"))]
    if let Some(addr) = mmap_pool::take(mmap_size) {
        unsafe {
            let ptr = addr as *mut T;
            return MmapSlice {
                ptr: NonNull::new_unchecked(ptr),
                len,
                mmap_size,
            };
        }
    }

    unsafe {
        // Over-allocate by one 2 MiB page so we can align the start address
        // to a 2 MiB boundary regardless of what the kernel returns.
        let raw = libc::mmap(
            std::ptr::null_mut(),
            mmap_size + PAGE_2MB,
            libc::PROT_READ | libc::PROT_WRITE,
            libc::MAP_ANONYMOUS | libc::MAP_PRIVATE,
            -1,
            0,
        );
        if raw == libc::MAP_FAILED {
            std::alloc::handle_alloc_error(std::alloc::Layout::array::<T>(len).unwrap());
        }

        // Trim to a 2 MiB-aligned region of exactly mmap_size bytes.
        let raw_addr = raw as usize;
        let aligned_addr = (raw_addr + PAGE_2MB - 1) & !(PAGE_2MB - 1);
        let lead = aligned_addr - raw_addr;
        if lead > 0 {
            libc::munmap(raw, lead);
        }
        let trail = PAGE_2MB - lead;
        if trail > 0 {
            libc::munmap((aligned_addr + mmap_size) as *mut libc::c_void, trail);
        }

        let ptr = aligned_addr as *mut T;

        // mmap(MAP_ANONYMOUS) already gives zero-initialised pages.

        #[cfg(target_os = "linux")]
        {
            // Enable fault-time THP: the 2 MiB-aligned VMA lets the kernel
            // allocate one huge page per 2 MiB block on the first access.
            let _ = libc::madvise(ptr.cast(), mmap_size, libc::MADV_HUGEPAGE);

            // Distribute huge pages across NUMA nodes in round-robin order.
            // With 2 MiB alignment each mbind unit is one huge page, so
            // interleaving does not split huge pages across nodes.
            #[cfg(target_arch = "x86_64")]
            {
                let nodemask: libc::c_ulong = !0;
                let _ = libc::syscall(
                    libc::SYS_mbind,
                    ptr as *mut libc::c_void,
                    mmap_size,
                    libc::MPOL_INTERLEAVE as libc::c_long,
                    &nodemask as *const libc::c_ulong,
                    65usize, // maxnode: covers up to 64 NUMA nodes
                    0u32,
                );
            }
        }

        MmapSlice {
            ptr: NonNull::new_unchecked(ptr),
            len,
            mmap_size,
        }
    }
}

#[cfg(not(target_family = "unix"))]
fn alloc_zeroed_slice<T: Copy>(len: usize) -> MmapSlice<T> {
    debug_assert!(len > 0);
    debug_assert!(len.is_power_of_two());

    let layout = std::alloc::Layout::array::<T>(len).unwrap();
    unsafe {
        let ptr = std::alloc::alloc_zeroed(layout).cast::<T>();
        if ptr.is_null() {
            std::alloc::handle_alloc_error(layout);
        }
        MmapSlice {
            ptr: NonNull::new_unchecked(ptr),
            len,
        }
    }
}

impl ShardedFlatMemo {
    fn with_per_shard_capacity(per_shard: usize) -> Self {
        let mut shards = Vec::with_capacity(NUM_SHARDS);
        for _ in 0..NUM_SHARDS {
            shards.push(FlatShard::with_capacity(per_shard));
        }
        Self {
            shards: shards.into_boxed_slice(),
        }
    }

    fn new() -> Self {
        Self::with_per_shard_capacity(8)
    }

    fn pre_allocate(&mut self, total_capacity: usize) {
        let per_shard = total_capacity.div_ceil(NUM_SHARDS);
        for shard in self.shards.iter_mut() {
            shard.pre_allocate(per_shard);
        }
    }

    #[inline(always)]
    fn get(&self, key: u64) -> Option<StepRange> {
        if key == FLAT_EMPTY_KEY {
            return None;
        }
        let shard = unsafe { self.shards.get_unchecked(shard_index(key)) };
        shard.get(key)
    }

    /// Issue a non-blocking prefetch for the FlatShard key slot that `key` will probe,
    /// so the cache line is warm by the time `get` is called. Prefetches only the
    /// keys array; values are read on hit (rare relative to probes).
    #[inline(always)]
    fn prefetch_key(&self, key: u64) {
        if key == FLAT_EMPTY_KEY {
            return;
        }
        let shard = unsafe { self.shards.get_unchecked(shard_index(key)) };
        let inner = unsafe { &*shard.inner.get() };
        let idx = (key as usize) & inner.mask;
        let key_ptr = unsafe { inner.keys.get_unchecked(idx) } as *const u64;
        #[cfg(target_arch = "x86_64")]
        unsafe {
            use std::arch::x86_64::{_mm_prefetch, _MM_HINT_T0};
            _mm_prefetch(key_ptr as *const i8, _MM_HINT_T0);
        }
        #[cfg(not(target_arch = "x86_64"))]
        let _ = key_ptr;
    }

    #[inline(always)]
    fn insert(&mut self, key: u64, value: StepRange) {
        if key == FLAT_EMPTY_KEY {
            return;
        }
        // SAFETY: &mut self ⇒ exclusive access.
        let idx = shard_index(key);
        unsafe {
            self.shards
                .get_unchecked(idx)
                .insert_unsynchronized(key, value)
        };
    }

    /// SAFETY: caller must ensure no concurrent insert/remove targets the same
    /// shard (i.e. same shard_index(key)). Concurrent ops on different shards
    /// are safe; concurrent reads are always safe.
    #[inline(always)]
    unsafe fn insert_unsynchronized(&self, key: u64, value: StepRange) {
        if key == FLAT_EMPTY_KEY {
            return;
        }
        let shard = unsafe { self.shards.get_unchecked(shard_index(key)) };
        unsafe { shard.insert_unsynchronized(key, value) };
    }

    /// SAFETY: same as `insert_unsynchronized`.
    unsafe fn remove_unsynchronized(&self, key: u64) -> Option<StepRange> {
        if key == FLAT_EMPTY_KEY {
            return None;
        }
        let shard = unsafe { self.shards.get_unchecked(shard_index(key)) };
        unsafe { shard.remove_unsynchronized(key) }
    }

    fn len(&self) -> usize {
        self.shards
            .iter()
            .map(|s| s.len.load(Ordering::Relaxed))
            .sum()
    }

    #[allow(dead_code)]
    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn iter(&self) -> impl Iterator<Item = (u64, StepRange)> + '_ {
        self.shards.iter().flat_map(|s| s.iter())
    }

    #[cfg(test)]
    fn contains_key(&self, key: u64) -> bool {
        self.get(key).is_some()
    }

    /// Shrinks the memo to keep ~`target_len` entries by score. Uses per-shard
    /// local selection + parallel rebuild (no global Vec materialization). Score
    /// distribution across shards is uniform (digests are zobrist hashes), so
    /// per-shard top-k is a close approximation of global top-k for large k.
    ///
    /// For very small `target_len` (where shard quantization matters), falls
    /// back to global selection.
    fn shrink_to_keep<F>(&mut self, target_len: usize, score_fn: F)
    where
        F: Fn(u64, StepRange) -> u64 + Sync,
    {
        let current = self.len();
        if current <= target_len {
            return;
        }

        // Small-target path: use global selection so per-shard quantization
        // doesn't lose entries the test/caller expects to keep.
        if target_len < NUM_SHARDS * 16 {
            let to_remove = current - target_len;
            let mut entries: Vec<(u64, u64, StepRange)> = self
                .shards
                .iter()
                .flat_map(|s| s.iter().map(|(k, v)| (score_fn(k, v), k, v)))
                .collect();
            entries.select_nth_unstable_by_key(to_remove, |&(score, _, _)| score);
            let mut per_shard_kept: Vec<Vec<(u64, StepRange)>> =
                (0..NUM_SHARDS).map(|_| Vec::new()).collect();
            for &(_, k, v) in &entries[to_remove..] {
                per_shard_kept[shard_index(k)].push((k, v));
            }
            for (shard, kept) in self.shards.iter().zip(per_shard_kept.iter()) {
                // SAFETY: &mut self ⇒ exclusive access.
                unsafe { shard.rebuild_with_unsynchronized(kept) };
            }
            return;
        }

        // Large-target fast path: per-shard local selection in parallel.
        let target_per_shard = target_len / NUM_SHARDS;
        // SAFETY: `&mut self` gives exclusive access; each rayon thread operates
        // on a distinct shard.
        self.shards.par_iter().for_each(|shard| {
            unsafe { shard.shrink_local_unsynchronized(target_per_shard, &score_fn) };
        });
    }
}

impl Default for ShardedFlatMemo {
    fn default() -> Self {
        Self::new()
    }
}

/// Insert `(key, packed)` into an open-addressing table that has at least one
/// empty slot.  Used when rehashing or rebuilding a freshly-cleared table where
/// no duplicate keys can exist, so the upsert check in `insert_unsynchronized`
/// is not needed.
///
/// # Safety
/// * `key != FLAT_EMPTY_KEY`
/// * `mask == keys.len() - 1` (keys.len() is a power of two)
/// * The table has at least one empty slot (otherwise this loops forever)
/// * `keys` and `values` have the same length
#[inline(always)]
unsafe fn probe_insert_into_clear(
    keys: &mut [u64],
    values: &mut [u32],
    mask: usize,
    key: u64,
    packed: u32,
) {
    let mut idx = (key as usize) & mask;
    loop {
        let slot = unsafe { keys.get_unchecked_mut(idx) };
        if *slot == FLAT_EMPTY_KEY {
            *slot = key;
            unsafe { *values.get_unchecked_mut(idx) = packed };
            return;
        }
        idx = (idx + 1) & mask;
    }
}

impl FlatShard {
    fn with_capacity(capacity: usize) -> Self {
        let min_size = capacity.saturating_mul(2).max(8);
        let size = min_size.next_power_of_two();
        let (keys, values) = alloc_slot_arrays(size);
        Self {
            inner: UnsafeCell::new(FlatShardInner {
                keys,
                values,
                mask: size - 1,
                capacity_threshold: size * 7 / 10,
            }),
            len: AtomicUsize::new(0),
        }
    }

    fn pre_allocate(&mut self, capacity: usize) {
        let needed_size = capacity.saturating_mul(2).max(8).next_power_of_two();
        // SAFETY: &mut self ⇒ exclusive access.
        let inner = unsafe { &mut *self.inner.get() };
        if needed_size <= inner.keys.len() {
            return;
        }
        let (new_keys, new_values) = alloc_slot_arrays(needed_size);
        let old_keys = std::mem::replace(&mut inner.keys, new_keys);
        let old_values = std::mem::replace(&mut inner.values, new_values);
        inner.mask = needed_size - 1;
        inner.capacity_threshold = needed_size * 7 / 10;
        let new_mask = inner.mask;
        for i in 0..old_keys.len() {
            let k = old_keys[i];
            if k == FLAT_EMPTY_KEY {
                continue;
            }
            unsafe {
                probe_insert_into_clear(
                    &mut inner.keys,
                    &mut inner.values,
                    new_mask,
                    k,
                    old_values[i],
                )
            };
        }
    }

    #[inline(always)]
    fn get(&self, key: u64) -> Option<StepRange> {
        debug_assert!(key != FLAT_EMPTY_KEY);
        // SAFETY: when `&self` is held, the public API requires `&mut self` for
        // mutations (`insert`/`remove`); the unsafe `*_unsynchronized` variants
        // document that callers must serialize with respect to other writers
        // for *this* shard.
        let inner = unsafe { &*self.inner.get() };
        let mask = inner.mask;
        let mut idx = (key as usize) & mask;
        loop {
            let slot_key = unsafe { *inner.keys.get_unchecked(idx) };
            if slot_key == FLAT_EMPTY_KEY {
                return None;
            }
            if slot_key == key {
                let packed = unsafe { *inner.values.get_unchecked(idx) };
                return Some(unpack_step_range(packed));
            }
            idx = (idx + 1) & mask;
        }
    }

    /// SAFETY: caller must ensure no concurrent writer on this shard.
    #[inline(always)]
    unsafe fn insert_unsynchronized(&self, key: u64, value: StepRange) {
        debug_assert!(key != FLAT_EMPTY_KEY);
        let inner = unsafe { &mut *self.inner.get() };
        let len_before = self.len.load(Ordering::Relaxed);
        if len_before >= inner.capacity_threshold {
            unsafe { Self::grow(inner) };
        }
        let packed = pack_step_range(value);
        let mask = inner.mask;
        let mut idx = (key as usize) & mask;
        loop {
            let slot_key = unsafe { inner.keys.get_unchecked_mut(idx) };
            if *slot_key == FLAT_EMPTY_KEY {
                *slot_key = key;
                unsafe {
                    *inner.values.get_unchecked_mut(idx) = packed;
                }
                self.len.fetch_add(1, Ordering::Relaxed);
                return;
            }
            if *slot_key == key {
                unsafe {
                    *inner.values.get_unchecked_mut(idx) = packed;
                }
                return;
            }
            idx = (idx + 1) & mask;
        }
    }

    #[cold]
    unsafe fn grow(inner: &mut FlatShardInner) {
        let new_size = (inner.keys.len() * 2).max(16);
        let (new_keys, new_values) = alloc_slot_arrays(new_size);
        let new_mask = new_size - 1;
        let old_keys = std::mem::replace(&mut inner.keys, new_keys);
        let old_values = std::mem::replace(&mut inner.values, new_values);
        inner.mask = new_mask;
        inner.capacity_threshold = new_size * 7 / 10;
        for i in 0..old_keys.len() {
            let k = old_keys[i];
            if k == FLAT_EMPTY_KEY {
                continue;
            }
            unsafe {
                probe_insert_into_clear(
                    &mut inner.keys,
                    &mut inner.values,
                    new_mask,
                    k,
                    old_values[i],
                )
            };
        }
    }

    /// SAFETY: caller must ensure no concurrent writer on this shard.
    unsafe fn remove_unsynchronized(&self, key: u64) -> Option<StepRange> {
        debug_assert!(key != FLAT_EMPTY_KEY);
        let inner = unsafe { &mut *self.inner.get() };
        let mask = inner.mask;
        let mut idx = (key as usize) & mask;
        let mut hole_idx;
        let removed_value;
        loop {
            let slot_key = unsafe { *inner.keys.get_unchecked(idx) };
            if slot_key == FLAT_EMPTY_KEY {
                return None;
            }
            if slot_key == key {
                removed_value = unpack_step_range(unsafe { *inner.values.get_unchecked(idx) });
                hole_idx = idx;
                break;
            }
            idx = (idx + 1) & mask;
        }
        unsafe { *inner.keys.get_unchecked_mut(hole_idx) = FLAT_EMPTY_KEY };

        // Backward shift deletion to preserve linear-probe invariants.
        let mut probe_idx = (hole_idx + 1) & mask;
        loop {
            let candidate_key = unsafe { *inner.keys.get_unchecked(probe_idx) };
            if candidate_key == FLAT_EMPTY_KEY {
                break;
            }
            let preferred = (candidate_key as usize) & mask;
            let belongs = if preferred <= probe_idx {
                hole_idx >= preferred && hole_idx < probe_idx
            } else {
                hole_idx >= preferred || hole_idx < probe_idx
            };
            if belongs {
                let candidate_value = unsafe { *inner.values.get_unchecked(probe_idx) };
                unsafe {
                    *inner.keys.get_unchecked_mut(hole_idx) = candidate_key;
                    *inner.values.get_unchecked_mut(hole_idx) = candidate_value;
                    *inner.keys.get_unchecked_mut(probe_idx) = FLAT_EMPTY_KEY;
                }
                hole_idx = probe_idx;
            }
            probe_idx = (probe_idx + 1) & mask;
        }
        self.len.fetch_sub(1, Ordering::Relaxed);
        Some(removed_value)
    }

    fn iter(&self) -> impl Iterator<Item = (u64, StepRange)> + '_ {
        let inner = unsafe { &*self.inner.get() };
        inner
            .keys
            .iter()
            .zip(inner.values.iter())
            .filter_map(|(k, v)| {
                if *k == FLAT_EMPTY_KEY {
                    None
                } else {
                    Some((*k, unpack_step_range(*v)))
                }
            })
    }

    /// Per-shard local shrink: collect entries with scores, partition to keep
    /// the top `target_len`, clear slots, re-insert kept entries. All work is
    /// local to the shard so multiple shards run in parallel without
    /// synchronization.
    ///
    /// SAFETY: caller must ensure no concurrent reads or writes on this shard.
    unsafe fn shrink_local_unsynchronized<F>(&self, target_len: usize, score_fn: &F)
    where
        F: Fn(u64, StepRange) -> u64,
    {
        let current = self.len.load(Ordering::Relaxed);
        if current <= target_len {
            return;
        }
        let inner = unsafe { &mut *self.inner.get() };

        let mut entries: Vec<(u64, u64, u32)> = inner
            .keys
            .iter()
            .zip(inner.values.iter())
            .filter_map(|(k, v)| {
                if *k == FLAT_EMPTY_KEY {
                    None
                } else {
                    Some((score_fn(*k, unpack_step_range(*v)), *k, *v))
                }
            })
            .collect();

        let to_remove = current - target_len;
        entries.select_nth_unstable_by_key(to_remove, |&(score, _, _)| score);

        // Clear and re-insert in one pass over the keys array.
        for slot_key in inner.keys.iter_mut() {
            *slot_key = FLAT_EMPTY_KEY;
        }
        let mask = inner.mask;
        for &(_, key, packed) in &entries[to_remove..] {
            unsafe {
                probe_insert_into_clear(&mut inner.keys, &mut inner.values, mask, key, packed)
            };
        }
        self.len.store(entries.len() - to_remove, Ordering::Relaxed);
    }

    /// Clear all slots and bulk-insert the given entries. Used by sharded shrink:
    /// the caller (ShardedFlatMemo::shrink_to_keep) performs global score-based
    /// selection and feeds each shard the entries it should keep.
    ///
    /// SAFETY: caller must ensure no concurrent reads or writes on this shard.
    unsafe fn rebuild_with_unsynchronized(&self, kept: &[(u64, StepRange)]) {
        let inner = unsafe { &mut *self.inner.get() };
        // Clear all keys in one pass.
        for slot_key in inner.keys.iter_mut() {
            *slot_key = FLAT_EMPTY_KEY;
        }
        // Re-insert kept entries (no resize, no threshold check — table is sized
        // to fit at least `current` entries, and `kept.len() <= current`).
        let mask = inner.mask;
        for &(key, value) in kept {
            let packed = pack_step_range(value);
            unsafe {
                probe_insert_into_clear(&mut inner.keys, &mut inner.values, mask, key, packed)
            };
        }
        self.len.store(kept.len(), Ordering::Relaxed);
    }
}

/// Hash key used for Bottom-K Sampling ordering.
///
/// CRITICAL: cannot be `digest` itself, because `shard_index = digest >>
/// SHARD_SHIFT` uses the **top** SHARD_BITS to route candidates. Sharding by
/// top bits means each shard holds a disjoint digest range — shard 0 has the
/// smallest digests, shard 63 the largest. A per-shard Bottom-K on raw digest
/// would then keep shard 0's smallest items only, not the global Bottom-K, so
/// `build_candidates` would emit ≈ shard-0-cap items (cap/NUM_SHARDS of W).
///
/// Using the **bottom** SHARD_SHIFT bits (orthogonal to the shard router)
/// gives each shard a uniformly distributed key in [0, 2^SHARD_SHIFT) → the
/// per-shard Bottom-K is statistically equivalent to a global Bottom-K under
/// the merge.
#[inline]
fn bottom_k_key(digest: u64) -> u64 {
    digest & ((1u64 << SHARD_SHIFT) - 1)
}

/// 16-byte candidate reference. Replaces materialised `Position` (88 B + Vec
/// overhead) inside Phase 1's shard buckets and the cross-shard candidate
/// pool. The q2 `Position` is reconstructed in Phase 2 from
/// `(frontier[frontier_idx], undo1_idx, undo2_idx)` via `previous()` — see
/// `previous_is_deterministic_2ply` for the determinism invariant this relies
/// on.
///
/// Layout (16 B total, naturally aligned):
///   digest: u64       (8 B) — full q2 digest for dedup + Bottom-K key
///   frontier_idx: u32 (4 B) — index into the Phase-1 input frontier snapshot
///   undo1_idx: u16    (2 B) — index into undo1[] produced by previous(q0)
///   undo2_idx: u16    (2 B) — index into undo2[] produced by previous(q1);
///                              `u16::MAX` is the sentinel for the 1-ply path
///                              (`advance_parallel_filtered`).
#[derive(Clone, Copy, Debug)]
struct CandRef {
    digest: u64,
    frontier_idx: u32,
    undo1_idx: u16,
    undo2_idx: u16,
}

/// Per-chunk scratch for the experimental mid-ply uniqueness prune
/// (`mid_uniqueness_prune`). Holds chunk-local (thread-private) memos so the
/// `solutions` verification of intermediate even-ply positions is parallel-safe
/// — the shared `self.memo`/`self.prev_memo` cannot be mutated concurrently, so
/// the prototype caches only within a chunk. `buf` accumulates one mid's
/// out-candidates so they can be committed or dropped atomically based on the
/// mid's verdict.
struct MidVerify {
    memo: Memo,
    prev_memo: Memo,
    killers: Killers,
    history: HistoryTable,
    scratch: Vec<Vec<Movement>>,
    buf: Vec<CandRef>,
}

impl MidVerify {
    fn new() -> Self {
        Self {
            memo: Memo::new(),
            prev_memo: Memo::new(),
            killers: Killers::new(),
            history: HistoryTable::new(),
            scratch: vec![],
            buf: vec![],
        }
    }
}

impl CandRef {
    /// Sentinel for `undo2_idx` when the candidate came from the 1-ply
    /// `advance_parallel_filtered` path — reconstruct stops after applying
    /// `undo1`.
    const UNDO2_NONE: u16 = u16::MAX;
}

/// Max-heap wrapper that orders `CandRef` by `bottom_k_key(digest)` ascending
/// under `BinaryHeap` (which is a max-heap): we want the root to be the
/// largest key so it's the eviction candidate when the bucket is full.
#[derive(Clone, Copy)]
struct HeapEntry(CandRef);

impl PartialEq for HeapEntry {
    fn eq(&self, other: &Self) -> bool {
        bottom_k_key(self.0.digest) == bottom_k_key(other.0.digest)
    }
}
impl Eq for HeapEntry {}
impl PartialOrd for HeapEntry {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for HeapEntry {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        bottom_k_key(self.0.digest).cmp(&bottom_k_key(other.0.digest))
    }
}

/// Per-shard candidate bucket used during Phase 1.
///
/// Two modes — chosen at construction by `cap`:
///  - `cap == usize::MAX`: plain Vec push, full dedup (legacy unbounded mode).
///  - `cap < usize::MAX`: Bottom-K Sampling. Each candidate gets ordering key
///    `bottom_k_key(digest)`; the bucket keeps the `cap` items with smallest
///    key. Cross-shard merge in `build_candidates` yields a global Bottom-K,
///    statistically equivalent to a uniform random sample (key is uniform).
///
/// `seen` is keyed by full digest (collision-free dedup across shards). In
/// Bottom-K mode it mirrors only currently-held items, so it stays bounded at
/// `cap` entries → memory O(cap) per shard, O(W) overall (independent of N).
/// A previously-evicted digest that re-arrives is re-processed; this wastes
/// work but doesn't break correctness (Bottom-K result is unchanged).
struct ShardBucket {
    seen: NoHashSet64,
    /// Used when `cap == usize::MAX`.
    vec: Vec<CandRef>,
    /// Used when `cap < usize::MAX`. Max-heap (under `bottom_k_key`) of
    /// CandRefs; root holds the largest key = eviction candidate.
    heap: BinaryHeap<HeapEntry>,
    cap: usize,
    /// Accepted-into-bucket count. In unbounded mode = distinct candidates.
    /// In Bottom-K mode = number of accepted inserts (≤ cap eventually);
    /// kept for stats only.
    count: usize,
}

impl ShardBucket {
    fn new(cap: usize) -> Self {
        Self {
            seen: NoHashSet64::default(),
            vec: Vec::new(),
            heap: BinaryHeap::new(),
            cap,
            count: 0,
        }
    }

    /// Try to insert `cand`. Dedup by full `digest` against currently-held
    /// items only (not all-ever-seen, so `seen` stays bounded in Bottom-K
    /// mode). Ordering for Bottom-K uses `bottom_k_key(digest)`.
    #[inline]
    fn try_insert(&mut self, cand: CandRef) {
        let d = cand.digest;
        if self.cap == usize::MAX {
            if self.seen.insert(d) {
                self.count += 1;
                self.vec.push(cand);
            }
            return;
        }
        // Bottom-K mode: `seen` mirrors digests currently in `heap`.
        if self.seen.contains(&d) {
            return;
        }
        if self.heap.len() < self.cap {
            self.seen.insert(d);
            self.heap.push(HeapEntry(cand));
            self.count += 1;
        } else {
            // SAFETY: heap is non-empty (len == cap > 0).
            let max_h = bottom_k_key(self.heap.peek().unwrap().0.digest);
            if bottom_k_key(d) < max_h {
                // pop returns the evicted CandRef; its full digest is what
                // sits in `seen` (not bottom_k_key, which collides).
                let evicted = self.heap.pop().unwrap().0;
                self.seen.remove(&evicted.digest);
                self.seen.insert(d);
                self.heap.push(HeapEntry(cand));
                self.count += 1;
            }
            // else: key ≥ max → drop. Same digest re-arriving is harmless.
        }
    }
}

/// Build the Phase-V input from the per-shard buckets.
///
/// When `pool_limit` is `None`, returns every kept candidate in arbitrary
/// order (legacy path).
///
/// When `pool_limit = Some(P)`, returns up to P candidates **sorted by
/// `bottom_k_key(digest)` ascending**. P should be the maximum number of
/// candidates the caller is willing to hand to Phase V — typically
/// `candidates_limit × candidates_pool_factor`. Phase V's own early-stop
/// (driven by `candidates_limit` = W) then keeps the final |next| ≈ W when
/// survival is high enough; with low survival, the larger pool lets Phase V
/// process more of mid before giving up.
///
/// Returns `(candidates, sampled)`. `sampled = true` means the result is a
/// strict subset of the true unique candidate set — either because Bottom-K
/// evicted items in some shard, or because the cross-shard merge truncated.
/// Callers must propagate this through `last_sampled` so checkpoint writers
/// refuse to persist a sampled frontier as "exact".
fn build_candidates(
    shard_data: Vec<ShardBucket>,
    pool_limit: Option<usize>,
) -> (Vec<CandRef>, bool) {
    match pool_limit {
        None => {
            let total: usize = shard_data.iter().map(|b| b.vec.len()).sum();
            let mut c = Vec::with_capacity(total);
            for bucket in shard_data {
                c.extend(bucket.vec);
            }
            (c, false)
        }
        Some(limit) => {
            // In Bottom-K mode `count` is the # of *accepted* inserts (≤ heap
            // size at any moment). If count > current heap.len(), at least one
            // eviction happened — strict-subset signal. (Same digest re-trying
            // after eviction inflates count, so this is "≥ true evicted" — a
            // safe over-estimate for the "sampled" flag.)
            let any_eviction = shard_data.iter().any(|b| b.count > b.heap.len());
            let total: usize = shard_data.iter().map(|b| b.heap.len()).sum();
            // Flatten directly into Vec<CandRef> — no intermediate
            // Vec<(u64, Position)> needed since CandRef carries the digest
            // and bottom_k_key is recomputed on sort.
            let mut all: Vec<CandRef> = Vec::with_capacity(total);
            for bucket in shard_data {
                all.extend(bucket.heap.into_iter().map(|e| e.0));
            }
            // Hash-ascending so Phase V can lazy-filter in optimal order.
            // par_sort_unstable is the dominant serial cost when SAFETY_FACTOR
            // × W is large; parallel sort eliminates it as an Amdahl bottleneck.
            all.par_sort_unstable_by_key(|c| bottom_k_key(c.digest));
            let truncated = all.len() > limit;
            if truncated {
                all.truncate(limit);
            }
            (all, any_eviction || truncated)
        }
    }
}

/// Sharded parallel merge: each delta is partitioned by shard, then each shard
/// is merged by a single thread (no synchronization needed across shards).
/// Both phases run with rayon parallelism.
fn merge_deltas_sharded(memo: &Memo, deltas: Vec<NoHashMap64<StepRange>>) {
    if deltas.is_empty() {
        return;
    }
    // Phase 1: partition each delta by shard.
    // Pre-allocate each shard Vec to delta.len() / NUM_SHARDS to avoid repeated
    // reallocations during the scatter loop (digests are uniform random, so each
    // shard gets ~1/NUM_SHARDS of the entries).
    let partitioned: Vec<[Vec<(u64, StepRange)>; NUM_SHARDS]> = deltas
        .into_par_iter()
        .map(|delta| {
            let per_shard = (delta.len() / NUM_SHARDS).max(1);
            let mut parts: [Vec<(u64, StepRange)>; NUM_SHARDS] =
                std::array::from_fn(|_| Vec::with_capacity(per_shard));
            for (k, v) in delta {
                parts[shard_index(k)].push((k, v));
            }
            parts
        })
        .collect();

    // Phase 2: merge per shard, in parallel (lock-free; each shard has 1 writer).
    (0..NUM_SHARDS).into_par_iter().for_each(|shard_idx| {
        for thread_parts in partitioned.iter() {
            for (k, v) in &thread_parts[shard_idx] {
                // SAFETY: each `shard_idx` value is processed by exactly one thread;
                // shard `shard_idx` therefore has at most one writer at a time, and
                // no other thread reads it during merge (Phase 2 of the search loop
                // is finished before this is called).
                unsafe { memo.insert_unsynchronized(*k, *v) };
            }
        }
    });
}
// ===== End ShardedFlatMemo =====

pub fn backward_initial_variants(initial_position: &PositionAux) -> Vec<PositionAux> {
    let mut variants = Vec::with_capacity(2);
    for pawn_drop in [false, true] {
        let mut position = initial_position.clone();
        position.set_pawn_drop(pawn_drop);
        if variants
            .iter()
            .all(|existing: &PositionAux| existing.digest() != position.digest())
        {
            variants.push(position);
        }
    }
    variants
}

pub fn backward_search(
    initial_position: &PositionAux,
    black_position: bool,
    forward: usize,
    one_way: bool,
) -> anyhow::Result<(u16, Vec<PositionAux>)> {
    backward_search_with_progress(
        initial_position,
        black_position,
        forward,
        one_way,
        |_s, _c, _u| {},
    )
}

pub fn backward_search_with_progress(
    initial_position: &PositionAux,
    black_position: bool,
    forward: usize,
    one_way: bool,
    progress: impl FnMut(u16, usize, String),
) -> anyhow::Result<(u16, Vec<PositionAux>)> {
    backward_search_with_progress_and_parallel(
        initial_position,
        black_position,
        forward,
        1,
        one_way,
        false,
        false,
        progress,
    )
}

pub fn backward_search_with_progress_and_parallel(
    initial_position: &PositionAux,
    black_position: bool,
    forward: usize,
    parallel: usize,
    one_way: bool,
    no_black_goldish: bool,
    bare_white_king: bool,
    mut progress: impl FnMut(u16, usize, String),
) -> anyhow::Result<(u16, Vec<PositionAux>)> {
    let mut best = (0, NoHashMap64::default());
    let mut last_error = None;

    for variant in backward_initial_variants(initial_position) {
        match backward_search_single(
            &variant,
            black_position,
            forward,
            parallel,
            one_way,
            no_black_goldish,
            bare_white_king,
            &mut progress,
        ) {
            Ok((step, positions)) => merge_backward_results(&mut best, step, positions),
            Err(err) if last_error.is_none() => last_error = Some(err),
            Err(_) => {}
        }
    }

    if best.1.is_empty() {
        return Err(last_error.unwrap_or_else(|| anyhow::anyhow!("No backward search result")));
    }

    let mut positions = best.1.into_values().collect::<Vec<_>>();
    positions.sort_by_cached_key(|p| p.sfen());
    Ok((best.0, positions))
}

fn backward_search_single(
    initial_position: &PositionAux,
    black_position: bool,
    forward: usize,
    parallel: usize,
    one_way: bool,
    no_black_goldish: bool,
    bare_white_king: bool,
    progress: &mut impl FnMut(u16, usize, String),
) -> anyhow::Result<(u16, Vec<PositionAux>)> {
    let mut search =
        BackwardSearch::new_with_parallel(initial_position, one_way, parallel, no_black_goldish)?;

    let initial_step = search.solution.len() as u16;
    let mut last_logged_step = search.step;

    let mut best = (0, NoHashMap64::default());

    for i in 0..=forward {
        if i > 0 {
            search.forward();
            info!("forward to {} ({}/{})", search.step, i, forward);
        }
        loop {
            if !search.advance()? {
                break;
            }
            if search.step != last_logged_step {
                last_logged_step = search.step;
                progress(
                    search.step,
                    search.positions.len(),
                    PositionAux::new(search.positions[0].clone(), *initial_position.stone())
                        .sfen_url(),
                );
            }
            if search.step > initial_step && search.step % 40 == 0 {
                info!(
                    "backward step={} count={} {}",
                    search.step,
                    search.positions.len(),
                    PositionAux::new(search.positions[0].clone(), *initial_position.stone())
                        .sfen_url()
                );
            } else if search.step > initial_step {
                debug!(
                    "backward step={} count={} {}",
                    search.step,
                    search.positions.len(),
                    PositionAux::new(search.positions[0].clone(), *initial_position.stone())
                        .sfen_url()
                );
            }
        }

        let step = if search.step > 0 && search.step % 2 == 0 && black_position {
            search.step - 1
        } else {
            search.step
        };

        let mut positions = search
            .positions
            .iter()
            .filter(|p| !p.pawn_drop())
            .map(|p| PositionAux::new(p.clone(), *initial_position.stone()))
            .collect::<Vec<_>>();

        let mut output_positions = Vec::new();
        if !black_position || search.step % 2 == 1 || search.step == 0 {
            for p in positions.iter_mut() {
                if !satisfies_backward_constraints(p, no_black_goldish) {
                    continue;
                }
                if !satisfies_output_constraints(p, bare_white_king) {
                    continue;
                }
                output_positions.push(p.clone());
            }
        } else {
            let mut black_positions = vec![];
            for p in positions.iter_mut() {
                debug_assert_eq!(p.turn(), Color::WHITE);
                let mut movements = vec![];
                advance_aux(p, &Default::default(), &mut movements)?;
                for m in movements.iter() {
                    let digest = p.moved_digest(m);
                    if search
                        .prev_memo
                        .get(digest)
                        .map_or(false, |x| x.is_uniquely(search.step - 1))
                    {
                        let mut np = p.clone();
                        np.do_move(m);
                        if !satisfies_backward_constraints(&np, no_black_goldish) {
                            continue;
                        }
                        if !satisfies_output_constraints(&np, bare_white_king) {
                            continue;
                        }
                        black_positions.push(np);
                    }
                }
            }
            for p in black_positions {
                output_positions.push(p);
            }
        }

        if output_positions.is_empty() || step < best.0 {
            continue;
        }
        if step > best.0 {
            best = (step, NoHashMap64::default());

            info!(
                "best={} count={} {}",
                best.0,
                search.positions.len(),
                PositionAux::new(search.positions[0].clone(), *initial_position.stone()).sfen_url()
            );
        }
        for p in output_positions {
            best.1.insert(p.digest(), p);
        }
    }
    // 呼び出し側 (`backward_search_with_progress_and_parallel`) で merge 後に
    // sort されるので、ここの sort は冗長。
    let positions = best.1.into_values().collect::<Vec<_>>();
    Ok((best.0, positions))
}

fn merge_backward_results(
    best: &mut (u16, NoHashMap64<PositionAux>),
    step: u16,
    positions: Vec<PositionAux>,
) {
    if step < best.0 {
        return;
    }
    if step > best.0 {
        best.0 = step;
        best.1.clear();
    }
    for position in positions {
        best.1.insert(position.digest(), position);
    }
}

/// Default `memo_retain_from_step`. Below this step the per-step memo is
/// discarded (fresh demand-zero pages beat carrying stale entries for short
/// searches: see the retention-policy comment in `advance_2ply_fused`); at or
/// above it the memo is carried across steps and bounded by `memo_entry_limit`.
/// Tunable via `BackwardSearch::set_memo_retain_from_step`.
const DEFAULT_MEMO_RETAIN_FROM_STEP: u16 = 10;

pub struct BackwardSearch {
    initial_position: PositionAux,
    solution: Vec<Movement>,
    seen_positions: usize,
    positions: Vec<Position>,
    prev_positions: Vec<Position>,
    memo: Memo,
    prev_memo: Memo,
    stone: Option<BitBoard>,
    step: u16,
    one_way: bool,
    no_black_goldish: bool,
    parallel: usize,
    pool: Option<rayon::ThreadPool>,
    memo_entry_limit: Option<usize>,
    /// Step at/above which the cross-step memo is retained (carried via
    /// `mem::take` and bounded by `memo_entry_limit`); below it the memo is
    /// discarded each step. See `DEFAULT_MEMO_RETAIN_FROM_STEP`. Raise it above
    /// the search depth to always discard (minimizes memo memory).
    memo_retain_from_step: u16,
    /// Experimental (default off): in `advance_2ply_fused`, verify the
    /// intermediate (even/mid) ply's uniqueness and drop it early when it is
    /// non-unique but produced at least one filtered out-candidate. A non-unique
    /// even ply cannot yield a unique odd ply, so this only prunes candidates
    /// Phase 2 would reject anyway (frontier-preserving) — it trades one mid V
    /// for the out V of that mid's children. Toggled by `set_mid_uniqueness_prune`.
    mid_uniqueness_prune: bool,
    /// When set, Phase 1 keeps at most this many dedup'd candidates using
    /// Bottom-K Sampling (uniform-equivalent via Zobrist hash). None = unlimited.
    candidates_limit: Option<usize>,
    /// Per-shard reservoir gets `candidates_limit × candidates_pool_factor /
    /// NUM_SHARDS` slots. Larger factor = more memory but better tolerance of
    /// low survival rate `s`: Phase V can keep early-stopping at W survivors
    /// only if pool ≥ W/s. Default 4 covers s ≥ 25%; raise it for searches
    /// where survival is consistently low (--candidates-pool-factor on CLI).
    candidates_pool_factor: usize,
    /// Set to `true` by the most recent advance when sampling or early-stop
    /// truncation actually changed the result vs an exact computation. Cleared
    /// at the start of each advance. Callers MUST read this to decide whether
    /// the current frontier is exact (safe to checkpoint as ground truth) or
    /// sampled (must be marked as such).
    last_sampled: bool,
    delta_trace: bool,
    canonicalize_attacker_goldish: bool,
    /// Precomputed stone contribution to digest (XOR of zobrist_stone for each
    /// stone square, 0 when stone=None). Lets Phase 2 compute pp_digest from
    /// core.digest() directly — no PositionAux construction needed on cache hits.
    stone_digest: u64,
    /// Measurement of the most recent `advance_parallel_filtered`: pre-advance
    /// frontier size, how many of those frontier positions produced zero
    /// filter-passing predecessors (a true backward dead-end — the key metric
    /// for the 2-ply-fusion prize), and the unique candidate count.
    last_frontier_in: usize,
    last_dead_end: usize,
    last_candidates: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BackwardSearchStats {
    pub step: u16,
    pub seen_positions: usize,
    pub positions_len: usize,
    pub prev_positions_len: usize,
    pub memo_len: usize,
    pub prev_memo_len: usize,
    /// Last `advance_parallel_filtered` measurement (0 if not measured this
    /// step, e.g. the serial small-frontier path).
    pub frontier_in: usize,
    pub dead_end_count: usize,
    pub candidate_count: usize,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct BackwardSearchResumeState {
    pub initial_position_sfen: String,
    pub remaining_solution_moves: Vec<String>,
    pub frontier_sfens: Vec<String>,
    pub step: u16,
    pub one_way: bool,
    pub no_black_goldish: bool,
}

impl BackwardSearch {
    pub fn new(initial_position: &PositionAux, one_way: bool) -> anyhow::Result<Self> {
        Self::new_with_parallel(initial_position, one_way, 1, false)
    }

    pub fn new_with_parallel(
        initial_position: &PositionAux,
        one_way: bool,
        parallel: usize,
        no_black_goldish: bool,
    ) -> anyhow::Result<Self> {
        if !satisfies_backward_constraints(initial_position, no_black_goldish) {
            bail!("Initial position has a black goldish piece");
        }

        let mut solution = standard_solve(initial_position.clone(), 2, true)?.solutions();
        if solution.len() != 1 {
            bail!("Not unique: {}", solution.len());
        }
        let solution = solution.remove(0);
        let mut p = initial_position.clone();
        for m in solution.iter() {
            p.do_move(m);
        }
        if !p.hands().is_empty(Color::BLACK) {
            bail!("Extra black pieces in checkmate");
        }

        let positions = vec![initial_position.core().clone()];

        let mut memo = Memo::new();
        let mut prev_memo = Memo::new();
        let mut p = initial_position.clone();
        memo.insert(p.digest(), StepRange::exact(solution.len() as u16));
        for (i, m) in solution.iter().enumerate() {
            p.do_move(m);
            if i % 2 == 0 {
                prev_memo.insert(
                    p.digest(),
                    StepRange::exact((solution.len() - i - 1) as u16),
                );
            } else {
                memo.insert(
                    p.digest(),
                    StepRange::exact((solution.len() - i - 1) as u16),
                );
            }
        }

        let step = solution.len() as u16;

        let parallel = parallel.max(1);
        let pool = if parallel > 1 {
            Some(
                rayon::ThreadPoolBuilder::new()
                    .num_threads(parallel)
                    .build()?,
            )
        } else {
            None
        };

        Ok(BackwardSearch {
            initial_position: initial_position.clone(),
            solution,
            seen_positions: 0,
            positions,
            prev_positions: vec![],
            memo,
            prev_memo,
            stone: *initial_position.stone(),
            step,
            one_way,
            no_black_goldish,
            parallel,
            pool,
            memo_entry_limit: None,
            candidates_limit: None,
            candidates_pool_factor: 4,
            last_sampled: false,
            delta_trace: false,
            last_frontier_in: 0,
            last_dead_end: 0,
            last_candidates: 0,
            canonicalize_attacker_goldish: false,
            stone_digest: initial_position.digest() ^ initial_position.core().digest(),
            memo_retain_from_step: DEFAULT_MEMO_RETAIN_FROM_STEP,
            mid_uniqueness_prune: false,
        })
    }

    /// Multi-seed constructor for the canonicalize-attacker-goldish flow.
    /// 全 seed が同じ solution.len() (= group_step) を持つことを要求する。
    /// canonical_digest が一致する seed 群は solution の move 構造も等しいため、
    /// 通常 group_step は揃う。memo は代表 seed の解路を canonical_digest で seed
    /// する (canonical-equivalent な seed の解路は同じ canonical_digest 列に collapse
    /// するため、代表 1 本で十分)。初期 frontier は全 seed の core を並べる
    /// (predecessor 生成は seed 個別に走り、canonicalize された digest で memo 共有
    /// が起こる)。
    ///
    /// 互換性: 単一 seed の `new_with_parallel` とは memo 種が異なる (canonical vs
    /// raw)。canonical 系の checkpoint は別形式 (現状未対応)。
    pub fn new_canonical_group(seeds: &[PositionAux], parallel: usize) -> anyhow::Result<Self> {
        if seeds.is_empty() {
            bail!("new_canonical_group: empty seed list");
        }
        let stone = *seeds[0].stone();

        let mut positions = Vec::with_capacity(seeds.len());
        let mut group_step: Option<u16> = None;
        let mut representative_solution: Option<Vec<Movement>> = None;

        for seed in seeds {
            if !satisfies_backward_constraints(seed, false) {
                bail!("Seed has black goldish constraint failure: {}", seed.sfen());
            }
            // 各 seed の uniqueness を verify (最大 2 解、unique 必須)。
            let mut sols = standard_solve(seed.clone(), 2, true)?.solutions();
            if sols.len() != 1 {
                bail!("Not unique seed: {}", seed.sfen());
            }
            let sol = sols.remove(0);
            let seed_step = sol.len() as u16;

            // 終端 (mated 状態) の黒手駒は空であること。
            let mut p = seed.clone();
            for m in sol.iter() {
                p.do_move(m);
            }
            if !p.hands().is_empty(Color::BLACK) {
                bail!("Extra black pieces in checkmate seed: {}", seed.sfen());
            }

            match group_step {
                None => {
                    group_step = Some(seed_step);
                    representative_solution = Some(sol);
                }
                Some(s) if s == seed_step => {}
                Some(s) => bail!(
                    "Step mismatch in canonical group: expected {} got {} for {}",
                    s,
                    seed_step,
                    seed.sfen()
                ),
            }
            positions.push(seed.core().clone());
        }

        let group_step = group_step.expect("non-empty seeds checked above");
        let representative_solution =
            representative_solution.expect("set together with group_step");

        let mut memo = Memo::new();
        let mut prev_memo = Memo::new();

        // 代表 seed の解路を canonical digest で memo に展開する。
        // distance-to-mate = group_step - i - 1 (i は move index)。
        // step (= group_step) と同 parity の距離は memo、反対 parity は prev_memo。
        let representative = &seeds[0];
        let mut p = representative.clone();
        memo.insert(
            crate::search::canonicalize::canonical_digest_for_smoke(&p),
            StepRange::exact(group_step),
        );
        for (i, m) in representative_solution.iter().enumerate() {
            p.do_move(m);
            let remaining = group_step - i as u16 - 1;
            let key = crate::search::canonicalize::canonical_digest_for_smoke(&p);
            if i % 2 == 0 {
                prev_memo.insert(key, StepRange::exact(remaining));
            } else {
                memo.insert(key, StepRange::exact(remaining));
            }
        }
        // group 内の他 seed も canonical_digest が一致する前提だが、念のため全 seed
        // の canonical_digest を group_step で memo に登録 (重複は no-op)。
        for seed in seeds {
            let key = crate::search::canonicalize::canonical_digest_for_smoke(seed);
            memo.insert(key, StepRange::exact(group_step));
        }

        let parallel = parallel.max(1);
        let pool = if parallel > 1 {
            Some(
                rayon::ThreadPoolBuilder::new()
                    .num_threads(parallel)
                    .build()?,
            )
        } else {
            None
        };

        Ok(BackwardSearch {
            initial_position: seeds[0].clone(),
            solution: vec![],
            seen_positions: 0,
            positions,
            prev_positions: vec![],
            memo,
            prev_memo,
            stone,
            step: group_step,
            one_way: false,
            no_black_goldish: false,
            parallel,
            pool,
            memo_entry_limit: None,
            candidates_limit: None,
            candidates_pool_factor: 4,
            last_sampled: false,
            delta_trace: false,
            last_frontier_in: 0,
            last_dead_end: 0,
            last_candidates: 0,
            canonicalize_attacker_goldish: true,
            stone_digest: seeds[0].digest() ^ seeds[0].core().digest(),
            memo_retain_from_step: DEFAULT_MEMO_RETAIN_FROM_STEP,
            mid_uniqueness_prune: false,
        })
    }

    /// Resume into the canonical-group flow. Memo seeding is regenerated by
    /// rerunning `new_canonical_group(seeds, parallel)` (deterministic given
    /// the same `seeds`), then the live frontier and step are overwritten
    /// from `state`. Caller must pass the same `seeds` group used to write
    /// the checkpoint — `seeds[0].sfen()` must match `state.initial_position_sfen`.
    pub fn from_resume_state_canonical_group(
        state: &BackwardSearchResumeState,
        seeds: &[PositionAux],
        parallel: usize,
    ) -> anyhow::Result<Self> {
        if seeds.is_empty() {
            bail!("from_resume_state_canonical_group: empty seed list");
        }
        let representative_sfen = seeds[0].sfen();
        if representative_sfen != state.initial_position_sfen {
            bail!(
                "Resume state initial_position mismatch: state={} seeds[0]={}",
                state.initial_position_sfen,
                representative_sfen
            );
        }
        let mut search = Self::new_canonical_group(seeds, parallel)?;
        let positions = state
            .frontier_sfens
            .par_iter()
            .map(|sfen| PositionAux::from_sfen(sfen).map(|p| p.core().clone()))
            .collect::<anyhow::Result<Vec<_>>>()?;
        search.positions = positions;
        search.step = state.step;
        search.seen_positions = 0;
        Ok(search)
    }

    pub fn from_resume_state(
        state: &BackwardSearchResumeState,
        parallel: usize,
    ) -> anyhow::Result<Self> {
        let initial_position = PositionAux::from_sfen(&state.initial_position_sfen)?;
        // Frontier can have millions of SFENs; SFEN→Position is independent so
        // parse in parallel to cut resume time.
        let positions = state
            .frontier_sfens
            .par_iter()
            .map(|sfen| PositionAux::from_sfen(sfen).map(|p| p.core().clone()))
            .collect::<anyhow::Result<Vec<_>>>()?;
        let solution = state
            .remaining_solution_moves
            .iter()
            .map(|mv| crate::sfen::decode_move(mv))
            .collect::<anyhow::Result<Vec<_>>>()?;

        let parallel = parallel.max(1);
        let pool = if parallel > 1 {
            Some(
                rayon::ThreadPoolBuilder::new()
                    .num_threads(parallel)
                    .build()?,
            )
        } else {
            None
        };

        Ok(BackwardSearch {
            initial_position: initial_position.clone(),
            solution,
            seen_positions: 0,
            positions,
            prev_positions: vec![],
            memo: Memo::new(),
            prev_memo: Memo::new(),
            stone: *initial_position.stone(),
            step: state.step,
            one_way: state.one_way,
            no_black_goldish: state.no_black_goldish,
            parallel,
            pool,
            memo_entry_limit: None,
            candidates_limit: None,
            candidates_pool_factor: 4,
            last_sampled: false,
            delta_trace: false,
            last_frontier_in: 0,
            last_dead_end: 0,
            last_candidates: 0,
            canonicalize_attacker_goldish: false,
            stone_digest: initial_position.digest() ^ initial_position.core().digest(),
            memo_retain_from_step: DEFAULT_MEMO_RETAIN_FROM_STEP,
            mid_uniqueness_prune: false,
        })
    }

    pub fn resume_state(&self) -> BackwardSearchResumeState {
        BackwardSearchResumeState {
            initial_position_sfen: self.initial_position.sfen(),
            remaining_solution_moves: self.solution.iter().map(crate::sfen::encode_move).collect(),
            frontier_sfens: self
                .positions
                .iter()
                .map(|p| PositionAux::new(p.clone(), self.stone).sfen())
                .collect(),
            step: self.step,
            one_way: self.one_way,
            no_black_goldish: self.no_black_goldish,
        }
    }

    /// Like `resume_state()` but omits `frontier_sfens` (empty vec).
    /// Use together with `frontier_to_binary()` when writing binary checkpoints.
    pub fn resume_state_header(&self) -> BackwardSearchResumeState {
        BackwardSearchResumeState {
            initial_position_sfen: self.initial_position.sfen(),
            remaining_solution_moves: self.solution.iter().map(crate::sfen::encode_move).collect(),
            frontier_sfens: vec![],
            step: self.step,
            one_way: self.one_way,
            no_black_goldish: self.no_black_goldish,
        }
    }

    /// Encode the current frontier as a flat byte buffer (88 bytes per `Position`).
    /// Pair with `resume_state_header()` for binary checkpoints.
    pub fn frontier_to_binary(&self) -> Vec<u8> {
        let mut buf = vec![0u8; self.positions.len() * 88];
        for (i, pos) in self.positions.iter().enumerate() {
            buf[i * 88..(i + 1) * 88].copy_from_slice(&pos.to_bytes());
        }
        buf
    }

    /// Resume from a header `state` (frontier_sfens may be empty) plus a
    /// binary frontier buffer produced by `frontier_to_binary()`.
    pub fn from_resume_state_with_frontier_bytes(
        state: &BackwardSearchResumeState,
        frontier_bytes: &[u8],
        parallel: usize,
    ) -> anyhow::Result<Self> {
        let initial_position = PositionAux::from_sfen(&state.initial_position_sfen)?;
        let n = frontier_bytes.len() / 88;
        let positions: Vec<Position> = (0..n)
            .into_par_iter()
            .map(|i| {
                let chunk: &[u8; 88] = frontier_bytes[i * 88..(i + 1) * 88].try_into().unwrap();
                Position::from_bytes(chunk)
            })
            .collect();
        let solution = state
            .remaining_solution_moves
            .iter()
            .map(|mv| crate::sfen::decode_move(mv))
            .collect::<anyhow::Result<Vec<_>>>()?;
        let parallel = parallel.max(1);
        let pool = if parallel > 1 {
            Some(
                rayon::ThreadPoolBuilder::new()
                    .num_threads(parallel)
                    .build()?,
            )
        } else {
            None
        };
        Ok(BackwardSearch {
            initial_position: initial_position.clone(),
            solution,
            seen_positions: 0,
            positions,
            prev_positions: vec![],
            memo: Memo::new(),
            prev_memo: Memo::new(),
            stone: *initial_position.stone(),
            step: state.step,
            one_way: state.one_way,
            no_black_goldish: state.no_black_goldish,
            parallel,
            pool,
            memo_entry_limit: None,
            candidates_limit: None,
            candidates_pool_factor: 4,
            last_sampled: false,
            delta_trace: false,
            last_frontier_in: 0,
            last_dead_end: 0,
            last_candidates: 0,
            canonicalize_attacker_goldish: false,
            stone_digest: initial_position.digest() ^ initial_position.core().digest(),
            memo_retain_from_step: DEFAULT_MEMO_RETAIN_FROM_STEP,
            mid_uniqueness_prune: false,
        })
    }

    /// Canonical-group variant of `from_resume_state_with_frontier_bytes`.
    pub fn from_resume_state_canonical_group_with_frontier_bytes(
        state: &BackwardSearchResumeState,
        frontier_bytes: &[u8],
        seeds: &[PositionAux],
        parallel: usize,
    ) -> anyhow::Result<Self> {
        if seeds.is_empty() {
            bail!("from_resume_state_canonical_group_with_frontier_bytes: empty seed list");
        }
        let representative_sfen = seeds[0].sfen();
        if representative_sfen != state.initial_position_sfen {
            bail!(
                "Resume state initial_position mismatch: state={} seeds[0]={}",
                state.initial_position_sfen,
                representative_sfen
            );
        }
        let mut search = Self::new_canonical_group(seeds, parallel)?;
        let n = frontier_bytes.len() / 88;
        let positions: Vec<Position> = (0..n)
            .into_par_iter()
            .map(|i| {
                let chunk: &[u8; 88] = frontier_bytes[i * 88..(i + 1) * 88].try_into().unwrap();
                Position::from_bytes(chunk)
            })
            .collect();
        search.positions = positions;
        search.step = state.step;
        search.seen_positions = 0;
        Ok(search)
    }

    pub fn advance(&mut self) -> anyhow::Result<bool> {
        if !self.one_way && self.parallel > 1 && self.seen_positions == 0 {
            return self.advance_parallel_filtered(&|_, _| true, &|_| true);
        }
        self.advance_upto(usize::MAX / 2)
    }

    pub fn set_memo_entry_limit(&mut self, max_entries: Option<usize>) {
        self.memo_entry_limit = max_entries.map(|limit| (limit / 2).max(1));
        if let Some(limit) = self.memo_entry_limit {
            // Pre-allocate to avoid resize/rehash overhead during merges. With
            // alloc_zeroed (lazy zero pages on Linux), the unused capacity costs
            // virtual address space only — physical pages fault in on first write.
            self.memo.pre_allocate(limit);
            self.prev_memo.pre_allocate(limit);
        }
    }

    /// Step at/above which the cross-step memo is retained (and bounded by the
    /// memo entry limit) instead of discarded each step. Defaults to
    /// `DEFAULT_MEMO_RETAIN_FROM_STEP`. Set above the search depth to always
    /// discard, minimizing memo memory at the cost of cross-step cache hits.
    pub fn set_memo_retain_from_step(&mut self, step: u16) {
        self.memo_retain_from_step = step;
    }

    /// Enable/disable the experimental mid-ply uniqueness prune in
    /// `advance_2ply_fused`. See the field doc on `mid_uniqueness_prune`.
    pub fn set_mid_uniqueness_prune(&mut self, enabled: bool) {
        self.mid_uniqueness_prune = enabled;
    }

    /// Update the memo entry limit without pre-allocating capacity. Use when
    /// the limit may change frequently (e.g., dynamic per-seed budget that
    /// grows as other seeds finish): pre_allocate's cost is +44.5% on the
    /// parallel path because the memos get reset each step anyway, so the
    /// up-front allocation is wasted. The lazy-grow Memo handles capacity
    /// adjustment on demand.
    pub fn set_memo_entry_limit_lazy(&mut self, max_entries: Option<usize>) {
        self.memo_entry_limit = max_entries.map(|limit| (limit / 2).max(1));
    }

    /// Limit the number of dedup'd candidates kept after Phase 1 using
    /// Bottom-K Sampling (uniform-equivalent, via Zobrist hash). Set to `None`
    /// to disable (default). When set, Phase 1 keeps at most `limit` candidates
    /// in O(limit) memory regardless of the true candidate count, and Phase V
    /// early-stops once `limit` survivors accumulate.
    pub fn set_candidates_limit(&mut self, limit: Option<usize>) {
        self.candidates_limit = limit;
    }

    /// Whether the most recent `advance_*` call's result is a sampled subset
    /// (Bottom-K eviction in Phase 1, build_candidates truncation, or Phase V
    /// early-stop) rather than the exact predecessor closure. Callers must
    /// treat a sampled frontier as approximate — in particular, must NOT
    /// persist it as an exact checkpoint.
    pub fn last_sampled(&self) -> bool {
        self.last_sampled
    }

    /// Set the per-shard pool overshoot factor used in Bottom-K Sampling.
    /// shard_cap = candidates_limit × factor / NUM_SHARDS. Larger absorbs
    /// lower survival rates `s` (Phase V can fill |next|=W as long as pool ≥
    /// W/s) at the cost of more Phase-1 memory. Must be ≥ 1.
    pub fn set_candidates_pool_factor(&mut self, factor: usize) {
        self.candidates_pool_factor = factor.max(1);
    }

    pub fn set_delta_trace(&mut self, enabled: bool) {
        self.delta_trace = enabled;
    }

    /// Smoke 用の正規化を uniqueness 判定の境界で適用する。
    /// see `crate::search::canonicalize::canonicalize_attacker_goldish`。
    pub fn set_canonicalize_attacker_goldish(&mut self, enabled: bool) {
        self.canonicalize_attacker_goldish = enabled;
    }

    pub fn set_pool(&mut self, pool: rayon::ThreadPool) {
        self.parallel = pool.current_num_threads();
        self.pool = Some(pool);
    }

    pub fn take_pool(&mut self) -> Option<rayon::ThreadPool> {
        self.pool.take()
    }

    pub fn set_parallel(&mut self, parallel: usize) {
        self.parallel = parallel.max(1);
    }

    fn install_or_run<T: Send>(&self, f: impl FnOnce() -> T + Send) -> T {
        if let Some(pool) = &self.pool {
            pool.install(f)
        } else {
            f()
        }
    }

    pub fn advance_upto(&mut self, upto: usize) -> anyhow::Result<bool> {
        self.advance_upto_with_candidate_filter(upto, |_, _| true, |_| true)
    }

    pub fn advance_upto_with_candidate_filter(
        &mut self,
        upto: usize,
        mut candidate_filter: impl FnMut(&PositionAux, &UndoMove) -> bool,
        mut filter: impl FnMut(&PositionAux) -> bool,
    ) -> anyhow::Result<bool> {
        // Serial small-frontier path: dead-end measurement is parallel-only,
        // so publish "not measured" (0) instead of stale parallel values.
        self.last_frontier_in = self.positions.len();
        self.last_dead_end = 0;
        self.last_candidates = 0;
        // This path does no Bottom-K Sampling; clear the flag so a `true`
        // left by a previous advance_2ply_fused / advance_parallel_filtered
        // call doesn't bleed into the caller's "is this exact?" check.
        self.last_sampled = false;
        let range = self.seen_positions..(self.seen_positions + upto).min(self.positions.len());
        self.seen_positions = range.end;
        let mut undo_moves = vec![];
        let mut solution_scratch = vec![];
        let mut killers = Killers::new();
        let mut history = HistoryTable::new();
        // Inline dedup set: prevents prev_positions from growing to O(N_total)
        // before the end-of-step retain. Keeps peak memory at O(N_unique).
        let mut prev_added: NoHashSet64 = Default::default();
        for core in self.positions[range].iter() {
            let mut position = PositionAux::new(core.clone(), self.stone);
            undo_moves.clear();
            previous(&mut position, self.step > 0, &mut undo_moves);

            for m in undo_moves.iter() {
                if !candidate_filter(&position, m) {
                    continue;
                }
                let mut pp = position.clone();
                pp.undo_move(m);

                if !is_backward_candidate_legal(&mut pp) {
                    continue;
                }
                if !satisfies_backward_constraints(&pp, self.no_black_goldish) {
                    continue;
                }

                if !filter(&pp) {
                    continue;
                }

                if self.one_way {
                    let mut branches = vec![];
                    let options = crate::position::AdvanceOptions {
                        max_allowed_branches: Some(1),
                        ..Default::default()
                    };
                    if crate::position::advance::advance::advance_aux(
                        &mut pp,
                        &options,
                        &mut branches,
                    )
                    .is_ok()
                    {
                        if !branches.is_empty() {
                            let d = pp.digest();
                            if prev_added.insert(d) {
                                self.prev_positions.push(pp.core().clone());
                            }
                            self.prev_memo.insert(d, StepRange::exact(self.step + 1));
                        }
                    }
                    continue;
                }

                let mate_in = self.step + 1;
                // smoke 用 canonicalize: 黒 goldish の駒種違いを 1 つの canonical に潰し
                // memo の cache hit 率を引き上げる。frontier には元の pp を push するが、
                // uniqueness 判定 (memo lookup + solutions) は canonical で行う。
                // Optimization: hit を期待して digest だけ先に計算 (mutation 不要)、
                // miss 時のみ実際に clone+canonicalize して solutions に渡す。
                let pp_digest = if self.canonicalize_attacker_goldish {
                    crate::search::canonicalize::canonical_digest_for_smoke(&pp)
                } else {
                    pp.digest()
                };
                let ans = if let Some(ans) = self
                    .prev_memo
                    .get(pp_digest)
                    .filter(|ans| !ans.needs_investigation(mate_in))
                {
                    ans
                } else if self.canonicalize_attacker_goldish {
                    let mut pp_canonical = pp.clone();
                    crate::search::canonicalize::canonicalize_attacker_goldish(&mut pp_canonical);
                    debug_assert_eq!(pp_canonical.digest(), pp_digest);
                    solutions(
                        &mut pp_canonical,
                        &self.prev_memo,
                        &self.memo,
                        mate_in,
                        &mut solution_scratch,
                        self.memo_entry_limit,
                        &mut killers,
                        &mut history,
                    )
                } else {
                    solutions(
                        &mut pp,
                        &self.prev_memo,
                        &self.memo,
                        mate_in,
                        &mut solution_scratch,
                        self.memo_entry_limit,
                        &mut killers,
                        &mut history,
                    )
                };
                if ans.is_uniquely(mate_in) {
                    #[cfg(debug_assertions)]
                    if !self.canonicalize_attacker_goldish {
                        // canonicalize を適用すると正当性が緩むため、debug 検証は OFF 時のみ。
                        let sol = standard_solve(pp.clone(), 2, true).unwrap().solutions();
                        if sol.len() != 1 {
                            eprintln!("Not unique: {} {}", sol.len(), pp.sfen_url());
                            for sol in sol.iter() {
                                let m = &sol[0];
                                let mut p = pp.clone();
                                p.do_move(m);
                                eprintln!(
                                    "{} {} {:?} {:?}",
                                    self.step,
                                    p.sfen_url(),
                                    m,
                                    self.memo.get(p.digest())
                                );
                            }
                            debug_assert_eq!(sol.len(), 1);
                        }
                    }

                    if prev_added.insert(pp_digest) {
                        self.prev_positions.push(pp.core().clone());
                    }
                }
            }
        }

        if self.seen_positions < self.positions.len() {
            return Ok(true);
        }

        if self.prev_positions.is_empty() {
            return Ok(false);
        }

        self.positions = std::mem::take(&mut self.prev_positions);
        std::mem::swap(&mut self.memo, &mut self.prev_memo);
        self.seen_positions = 0;

        self.step += 1;

        Ok(true)
    }

    /// Fused 2-ply backward step (frontier N → N+2), used as the smoke
    /// search's odd→odd advance.
    ///
    /// Phase 1 generates predecessors inline two plies deep WITHOUT ever
    /// materialising the intermediate (white/even) layer as a Vec: for each
    /// frontier position q0 we generate the mid-filtered predecessor q1 and
    /// immediately, in the same pass, generate q1's out-filtered predecessors
    /// q2 (only q2 is dedup'd/stored). Both plies apply the per-step smoke
    /// filters (they bound the predecessor set — unfiltered, the intermediate
    /// explodes and the verified ply's V becomes intractable). The
    /// intermediate ply runs no `solutions_overlay`: per spec only the kept
    /// output positions must be unique, so Phase 2's verification at depth
    /// `step+2` is the source of truth and the expensive intermediate-parity
    /// V is skipped (the 2-ply prize). Not materialising q1 also drops the
    /// intermediate's storage, its dedup pass, and its checkpoint.
    ///
    /// Memo/parity: one `swap` here (mirroring a 1-ply step that added no
    /// entries) plus Phase 2's end `swap` = two swaps per fused iteration,
    /// matching two 1-ply steps, so memo parity and the cross-step
    /// verification amortisation are preserved across the 2-ply boundary.
    ///
    /// Phase 2 below is byte-identical to `advance_parallel_filtered`'s
    /// verification (kept as a deliberate copy so the heavily-tuned 1-ply
    /// method stays untouched and its equivalence test stays valid).
    ///
    /// Applies whether the pool is parallel or not (`install_or_run` /
    /// `set_parallel(1)` degrade `par_chunks` to effectively serial).
    #[allow(clippy::too_many_arguments)]
    pub fn advance_2ply_fused(
        &mut self,
        mid_candidate_filter: &(impl Fn(&PositionAux, &UndoMove) -> bool + Sync),
        mid_filter: &(impl Fn(&PositionAux) -> bool + Sync),
        out_candidate_filter: &(impl Fn(&PositionAux, &UndoMove) -> bool + Sync),
        out_filter: &(impl Fn(&PositionAux) -> bool + Sync),
    ) -> anyhow::Result<bool> {
        self.last_sampled = false;
        if self.positions.is_empty() {
            self.last_frontier_in = 0;
            self.last_dead_end = 0;
            self.last_candidates = 0;
            set_progress_phase(0);
            return Ok(false);
        }

        let step = self.step;
        let stone = self.stone;
        let no_black_goldish = self.no_black_goldish;
        // Experimental mid-ply uniqueness prune (default off). Captured here so
        // the parallel Phase-1 closure can read them without borrowing self.
        let mid_uniqueness_prune = self.mid_uniqueness_prune;
        let memo_entry_limit = self.memo_entry_limit;
        let position_parallel = self.parallel.min(self.positions.len());
        let position_chunk_size = self.positions.len().div_ceil(position_parallel * 8).max(1);

        // Phase 1: inline double-previous (no intermediate Vec).
        set_progress_phase(1);
        let positions = &self.positions;
        let frontier_in = positions.len();
        let dedup_count = AtomicUsize::new(0);
        // Frontier positions that produced zero output candidate over the two
        // plies: true 2-ply backward dead-ends.
        let dead_end_count = AtomicUsize::new(0);
        // Per-shard cap for Bottom-K Sampling. candidates_pool_factor > 1 leaves
        // headroom for Phase V to lazy-filter from the smallest-digest end
        // (later we keep only the W candidates whose survivors make it to
        // |next| = W). Memory is O(pool_factor × W). cap = usize::MAX means
        // "unbounded" (legacy mode, no sampling).
        let shard_cap = self
            .candidates_limit
            .map(|lim| (lim.saturating_mul(self.candidates_pool_factor)).div_ceil(NUM_SHARDS))
            .unwrap_or(usize::MAX);
        let shard_buckets: Vec<Mutex<ShardBucket>> = (0..NUM_SHARDS)
            .map(|_| Mutex::new(ShardBucket::new(shard_cap)))
            .collect();

        self.install_or_run(|| {
            positions
                .par_chunks(position_chunk_size)
                .enumerate()
                .for_each(|(chunk_idx, chunk)| {
                    let mut undo1 = vec![];
                    let mut undo2 = vec![];
                    // Experimental mid-ply uniqueness-prune scratch; `None` when
                    // the flag is off so the default path pays nothing.
                    let mut midv = mid_uniqueness_prune.then(MidVerify::new);
                    let mut local_seens: [NoHashSet64; NUM_SHARDS] =
                        std::array::from_fn(|_| NoHashSet64::default());
                    let mut local_outs: [Vec<CandRef>; NUM_SHARDS] =
                        std::array::from_fn(|_| Vec::new());
                    let mut chunk_dedup = 0usize;
                    let mut chunk_dead = 0usize;

                    // par_chunks gives equal-sized chunks except the last; the
                    // chunk's first frontier index is `chunk_idx *
                    // position_chunk_size`. par_chunks is contiguous (Rayon
                    // docs), so `chunk_idx * chunk_size + pos_in_chunk`
                    // recovers the global frontier index for any item in
                    // `chunk`.
                    let chunk_base = chunk_idx * position_chunk_size;
                    for (pos_in_chunk, core) in chunk.iter().enumerate() {
                        let frontier_idx = (chunk_base + pos_in_chunk) as u32;
                        let mut q0 = PositionAux::new(core.clone(), stone);
                        undo1.clear();
                        previous(&mut q0, step > 0, &mut undo1);

                        let mut any_survived = false;
                        for (i1, m1) in undo1.iter().enumerate() {
                            if !mid_candidate_filter(&q0, m1) {
                                continue;
                            }
                            let mut q1 = q0.clone();
                            q1.undo_move(m1);
                            if !is_backward_candidate_legal(&mut q1) {
                                continue;
                            }
                            if !satisfies_backward_constraints(&q1, no_black_goldish) {
                                continue;
                            }
                            if !mid_filter(&q1) {
                                continue;
                            }
                            // q1 is a valid (filtered, unverified) intermediate.
                            // Expand it one more ply without storing it.
                            undo2.clear();
                            previous(&mut q1, step + 1 > 0, &mut undo2);
                            if let Some(midv) = midv.as_mut() {
                                // Experimental mid-ply uniqueness prune: buffer
                                // this mid's filtered out-candidates, then commit
                                // them only if q1 is a unique mate in step+1.
                                // A non-unique even ply cannot yield a unique odd
                                // ply, so the dropped candidates would all fail
                                // Phase-2 V anyway (frontier-preserving). The mid
                                // V is paid only when the mid is not a dead end.
                                midv.buf.clear();
                                for (i2, m2) in undo2.iter().enumerate() {
                                    if !out_candidate_filter(&q1, m2) {
                                        continue;
                                    }
                                    let mut q2 = q1.clone();
                                    q2.undo_move(m2);
                                    if !is_backward_candidate_legal(&mut q2) {
                                        continue;
                                    }
                                    if !satisfies_backward_constraints(&q2, no_black_goldish) {
                                        continue;
                                    }
                                    if !out_filter(&q2) {
                                        continue;
                                    }
                                    midv.buf.push(CandRef {
                                        digest: q2.core().digest(),
                                        frontier_idx,
                                        undo1_idx: i1 as u16,
                                        undo2_idx: i2 as u16,
                                    });
                                }
                                if midv.buf.is_empty() {
                                    continue;
                                }
                                // Chunk-local memos (thread-private) make this V
                                // parallel-safe; it caches only within the chunk.
                                let mut q1_v = q1.clone();
                                let ans = solutions(
                                    &mut q1_v,
                                    &midv.prev_memo,
                                    &midv.memo,
                                    step + 1,
                                    &mut midv.scratch,
                                    memo_entry_limit,
                                    &mut midv.killers,
                                    &mut midv.history,
                                );
                                if !ans.is_uniquely(step + 1) {
                                    continue;
                                }
                                for cand in midv.buf.iter() {
                                    any_survived = true;
                                    let shard_idx = shard_index(cand.digest);
                                    if local_seens[shard_idx].insert(cand.digest) {
                                        local_outs[shard_idx].push(*cand);
                                        chunk_dedup += 1;
                                    }
                                }
                            } else {
                                for (i2, m2) in undo2.iter().enumerate() {
                                    if !out_candidate_filter(&q1, m2) {
                                        continue;
                                    }
                                    let mut q2 = q1.clone();
                                    q2.undo_move(m2);
                                    if !is_backward_candidate_legal(&mut q2) {
                                        continue;
                                    }
                                    if !satisfies_backward_constraints(&q2, no_black_goldish) {
                                        continue;
                                    }
                                    if !out_filter(&q2) {
                                        continue;
                                    }
                                    any_survived = true;
                                    let digest = q2.core().digest();
                                    let shard_idx = shard_index(digest);
                                    if local_seens[shard_idx].insert(digest) {
                                        local_outs[shard_idx].push(CandRef {
                                            digest,
                                            frontier_idx,
                                            undo1_idx: i1 as u16,
                                            undo2_idx: i2 as u16,
                                        });
                                        chunk_dedup += 1;
                                    }
                                    // q2 dropped here — the candidate Position is
                                    // reconstructed in Phase 2 from
                                    // (frontier_idx, i1, i2).
                                }
                            }
                        }
                        if !any_survived {
                            chunk_dead += 1;
                        }
                    }
                    dedup_count.fetch_add(chunk_dedup, Ordering::Relaxed);
                    dead_end_count.fetch_add(chunk_dead, Ordering::Relaxed);

                    drop(local_seens);
                    let stagger = chunk_idx % NUM_SHARDS;
                    for i in 0..NUM_SHARDS {
                        let shard_idx = (i + stagger) % NUM_SHARDS;
                        let local = std::mem::take(&mut local_outs[shard_idx]);
                        if local.is_empty() {
                            continue;
                        }
                        let mut guard = shard_buckets[shard_idx].lock().unwrap();
                        for cand in local {
                            guard.try_insert(cand);
                        }
                    }
                });
        });

        let candidate_len = dedup_count.into_inner();
        let dead_end = dead_end_count.into_inner();
        let shard_data: Vec<ShardBucket> = shard_buckets
            .into_iter()
            .map(|m| m.into_inner().unwrap())
            .collect();
        let total_unique: usize = shard_data.iter().map(|b| b.count).sum();
        // build_candidates' truncation limit is `pool_size = candidates_limit
        // × candidates_pool_factor`, NOT candidates_limit itself.
        // candidates_limit (= W) is what Phase V uses for its early-stop
        // target; build_candidates instead caps at the pool size so the
        // mid-set handed to Phase V can be larger than W (necessary when
        // survival rate < 1/pool_factor — otherwise |next| would be capped at
        // W × s < W and the frontier collapses).
        let pool_size = self
            .candidates_limit
            .map(|w| w.saturating_mul(self.candidates_pool_factor));
        let (candidates, p1_sampled) = build_candidates(shard_data, pool_size);
        self.last_sampled = p1_sampled;

        // Mirror the intermediate ply's end-of-step swap (no entries added),
        // then advance the step so Phase 2 below verifies at depth `step+1`
        // (= original step + 2) exactly as the 1-ply scheme would after the
        // intermediate ply. Two swaps total per fused iteration.
        std::mem::swap(&mut self.memo, &mut self.prev_memo);
        self.seen_positions = 0;
        self.step += 1;

        // ---- Phase 2 (verbatim copy of advance_parallel_filtered's) ----
        // Snapshot of the Phase-1 frontier: candidates reconstruct their q2
        // Position from `frontier[cand.frontier_idx]`. Phase 2 only reads
        // `self.positions` indirectly via this slice, and `self.positions`
        // is only reassigned at the bottom of this function, so the borrow
        // is safe for the entire wave loop.
        let frontier: &[Position] = &self.positions;
        // step at which Phase 2 runs the *first* of the two reconstruction
        // plies. `previous(q0, step > 0)` reproduces undo1 exactly as Phase
        // 1 generated it (Phase 1 captured `step` before incrementing).
        let phase1_step = step;
        let step = self.step;
        if candidates.is_empty() {
            self.last_frontier_in = frontier_in;
            self.last_dead_end = dead_end;
            self.last_candidates = 0;
            set_progress_phase(0);
            return Ok(false);
        }

        let parallel = self.parallel.min(candidates.len());
        let chunk_size = candidates.len().div_ceil(parallel * 64).max(1);
        let mut memo;
        let mut prev_memo;
        if step >= self.memo_retain_from_step {
            memo = std::mem::take(&mut self.memo);
            prev_memo = std::mem::take(&mut self.prev_memo);
        } else {
            self.memo = Memo::new();
            self.prev_memo = Memo::new();
            memo = Memo::new();
            prev_memo = Memo::new();
        }

        let phase2_start = std::time::Instant::now();

        let mut all_positions = vec![];
        let mut delta_total_count = 0usize;
        let mut phase2_only_ms = 0u128;
        let mut merge_ms = 0u128;

        let wave_chunk_count = parallel * 8;
        let wave_size = chunk_size * wave_chunk_count;
        let canonicalize = self.canonicalize_attacker_goldish;
        let stone_digest = self.stone_digest;
        // See advance_parallel_filtered for the lazy-filter rationale.
        let target_w = self.candidates_limit;
        set_progress_phase(3);
        for wave in candidates.chunks(wave_size) {
            if target_w.is_some_and(|w| all_positions.len() >= w) {
                self.last_sampled = true;
                break;
            }
            let memo_ref: &Memo = &memo;
            let prev_memo_ref: &Memo = &prev_memo;
            // Wave-scoped shared cache: canonical digest -> V answer. Goldish-
            // equivalent candidates split across parallel chunks reuse one
            // solutions_overlay result instead of each recomputing it (the
            // chunk-local prev_memo_delta does not cross chunk boundaries
            // within a wave). The frontier still keeps every distinct position
            // -- we only skip the redundant V, then push `core` per candidate
            // exactly as before, so the search space is unchanged. Dropped at
            // wave end => memory returned (OOM-safe). No-op when
            // canonicalization is off (shard locks never taken).
            let shared_v: Option<Vec<Mutex<NoHashMap64<StepRange>>>> = if canonicalize {
                Some(
                    (0..NUM_SHARDS)
                        .map(|_| {
                            Mutex::new(NoHashMap64::with_capacity_and_hasher(
                                256,
                                Default::default(),
                            ))
                        })
                        .collect(),
                )
            } else {
                None
            };
            let shared_v_ref = shared_v.as_deref();

            let wave_start = std::time::Instant::now();
            let wave_results: Vec<(
                Vec<Position>,
                NoHashMap64<StepRange>,
                NoHashMap64<StepRange>,
            )> = self.install_or_run(|| {
                wave.par_chunks(chunk_size)
                    .map(|chunk| {
                        let mut memo_delta =
                            NoHashMap64::with_capacity_and_hasher(4096, Default::default());
                        let mut prev_memo_delta =
                            NoHashMap64::with_capacity_and_hasher(4096, Default::default());
                        let mut prev_positions = vec![];
                        let mut solution_scratch = vec![];
                        let mut killers = Killers::new();
                        let mut history = HistoryTable::new();
                        // Per-chunk undo move buffers reused for each q2
                        // reconstruction. Each `previous()` call clears and
                        // refills these in-place.
                        let mut undo1_buf: Vec<UndoMove> = vec![];
                        let mut undo2_buf: Vec<UndoMove> = vec![];

                        for cand in chunk.iter() {
                            // Reconstruct q2 from
                            // (frontier[frontier_idx], undo1_idx, undo2_idx).
                            // Determinism of `previous()` guarantees the indexed
                            // undo move matches the one Phase 1 selected (see
                            // previous_is_deterministic_2ply). The
                            // `phase1_step` value is captured before
                            // `self.step += 1`, so the `allow_drop_pawn`
                            // arguments match Phase 1's exactly.
                            let frontier_core = &frontier[cand.frontier_idx as usize];
                            let mut q0 = PositionAux::new(frontier_core.clone(), stone);
                            undo1_buf.clear();
                            previous(&mut q0, phase1_step > 0, &mut undo1_buf);
                            let mut q1 = q0;
                            q1.undo_move(&undo1_buf[cand.undo1_idx as usize]);
                            undo2_buf.clear();
                            previous(&mut q1, phase1_step + 1 > 0, &mut undo2_buf);
                            let mut pp = q1;
                            pp.undo_move(&undo2_buf[cand.undo2_idx as usize]);
                            debug_assert_eq!(
                                pp.core().digest(),
                                cand.digest,
                                "reconstructed q2 digest mismatch: \
                                 frontier_idx={} i1={} i2={}",
                                cand.frontier_idx,
                                cand.undo1_idx,
                                cand.undo2_idx,
                            );

                            // memo / shared_v lookup keys. Non-canonicalize
                            // path can compute pp_digest directly from the
                            // CandRef without re-hashing.
                            let pp_digest = if canonicalize {
                                crate::search::canonicalize::canonical_digest_for_smoke(&pp)
                            } else {
                                cand.digest ^ stone_digest
                            };
                            if let Some(ans) =
                                get_overlay(&prev_memo_delta, prev_memo_ref, pp_digest)
                                    .filter(|ans| !ans.needs_investigation(step + 1))
                            {
                                if ans.is_uniquely(step + 1) {
                                    prev_positions.push(pp.core().clone());
                                }
                                continue;
                            }

                            // Cross-chunk reuse: a sibling chunk in this wave
                            // may have already computed V for this canonical
                            // class. Same trust model as get_overlay above
                            // (skip only when the cached answer is final at
                            // this depth), just sourced from another chunk.
                            if let Some(shards) = shared_v_ref {
                                let cached = shards[shard_index(pp_digest)]
                                    .lock()
                                    .unwrap()
                                    .get(&pp_digest)
                                    .copied();
                                if let Some(ans) = cached
                                    .filter(|ans| !ans.needs_investigation(step + 1))
                                {
                                    // Self-check: the cached verdict must equal
                                    // a fresh recomputation. This turns the
                                    // "memoization soundness + canonical
                                    // determinism" argument into a runtime
                                    // invariant exercised by every debug/test
                                    // run. Zero release cost.
                                    #[cfg(debug_assertions)]
                                    {
                                        let mut pp_chk = pp.clone();
                                        crate::search::canonicalize::canonicalize_attacker_goldish(
                                            &mut pp_chk,
                                        );
                                        let fresh = solutions_overlay(
                                            &mut pp_chk,
                                            prev_memo_ref,
                                            &mut prev_memo_delta,
                                            memo_ref,
                                            &mut memo_delta,
                                            step + 1,
                                            &mut solution_scratch,
                                            &mut killers,
                                            &mut history,
                                        );
                                        debug_assert_eq!(
                                            ans.is_uniquely(step + 1),
                                            fresh.is_uniquely(step + 1),
                                            "shared-V cache verdict mismatch: \
                                             digest={:#x} depth={}",
                                            pp_digest,
                                            step + 1
                                        );
                                    }
                                    if ans.is_uniquely(step + 1) {
                                        prev_positions.push(pp.core().clone());
                                    }
                                    continue;
                                }
                            }

                            let ans = if canonicalize {
                                let mut pp_canonical = pp.clone();
                                crate::search::canonicalize::canonicalize_attacker_goldish(
                                    &mut pp_canonical,
                                );
                                debug_assert_eq!(pp_canonical.digest(), pp_digest);
                                solutions_overlay(
                                    &mut pp_canonical,
                                    prev_memo_ref,
                                    &mut prev_memo_delta,
                                    memo_ref,
                                    &mut memo_delta,
                                    step + 1,
                                    &mut solution_scratch,
                                    &mut killers,
                                    &mut history,
                                )
                            } else {
                                solutions_overlay(
                                    &mut pp,
                                    prev_memo_ref,
                                    &mut prev_memo_delta,
                                    memo_ref,
                                    &mut memo_delta,
                                    step + 1,
                                    &mut solution_scratch,
                                    &mut killers,
                                    &mut history,
                                )
                            };
                            // Publish for sibling chunks in this wave. Only
                            // cache answers that are final at this depth, so
                            // the reuse path's filter is exactly get_overlay's.
                            if let Some(shards) = shared_v_ref {
                                if !ans.needs_investigation(step + 1) {
                                    shards[shard_index(pp_digest)]
                                        .lock()
                                        .unwrap()
                                        .insert(pp_digest, ans);
                                }
                            }
                            if ans.is_uniquely(step + 1) {
                                prev_positions.push(pp.core().clone());
                            }
                        }

                        (prev_positions, memo_delta, prev_memo_delta)
                    })
                    .collect()
            });
            phase2_only_ms += wave_start.elapsed().as_millis();

            let mut wave_memo_deltas = Vec::with_capacity(wave_chunk_count);
            let mut wave_prev_deltas = Vec::with_capacity(wave_chunk_count);
            for (positions, memo_delta, prev_memo_delta) in wave_results {
                all_positions.extend(positions);
                delta_total_count += memo_delta.len() + prev_memo_delta.len();
                wave_memo_deltas.push(memo_delta);
                wave_prev_deltas.push(prev_memo_delta);
            }

            let merge_wave_start = std::time::Instant::now();
            self.install_or_run(|| {
                merge_deltas_sharded(&memo, wave_memo_deltas);
                merge_deltas_sharded(&prev_memo, wave_prev_deltas);
            });
            merge_ms += merge_wave_start.elapsed().as_millis();
        }

        set_progress_phase(4);
        let shrink_start = std::time::Instant::now();
        if let Some(limit) = self.memo_entry_limit {
            if memo.len() >= limit {
                shrink_memo(&mut memo, limit / 2);
            }
            if prev_memo.len() >= limit {
                shrink_memo(&mut prev_memo, limit / 2);
            }
        }
        let shrink_ms = shrink_start.elapsed().as_millis();

        let mut all_positions = all_positions;
        // Truncate to exactly W: the early-stop loop may overshoot by up to
        // one wave. Items are in digest-ascending order so truncating from
        // the tail keeps the smallest-digest W (= uniform random W).
        if let Some(w) = target_w {
            if all_positions.len() > w {
                all_positions.truncate(w);
                self.last_sampled = true;
            }
        }

        if self.delta_trace {
            eprintln!(
                "delta_trace step={} candidates={} phase2_elapsed_ms={} phase2_only_ms={} merge_ms={} shrink_ms={} delta_total={} \
                 memo_size={} prev_memo_size={}",
                step,
                candidate_len,
                phase2_start.elapsed().as_millis(),
                phase2_only_ms,
                merge_ms,
                shrink_ms,
                delta_total_count,
                memo.len(),
                prev_memo.len(),
            );
        }

        self.memo = memo;
        self.prev_memo = prev_memo;

        if all_positions.is_empty() {
            self.last_frontier_in = frontier_in;
            self.last_dead_end = dead_end;
            self.last_candidates = total_unique;
            set_progress_phase(0);
            return Ok(false);
        }

        self.positions = all_positions;
        self.prev_positions = Vec::new();
        std::mem::swap(&mut self.memo, &mut self.prev_memo);
        self.seen_positions = 0;
        self.step += 1;

        self.last_frontier_in = frontier_in;
        self.last_dead_end = dead_end;
        self.last_candidates = total_unique;
        set_progress_phase(0);
        Ok(true)
    }

    pub fn advance_parallel_filtered(
        &mut self,
        candidate_filter: &(impl Fn(&PositionAux, &UndoMove) -> bool + Sync),
        filter: &(impl Fn(&PositionAux) -> bool + Sync),
    ) -> anyhow::Result<bool> {
        self.last_sampled = false;
        if self.positions.is_empty() {
            self.last_frontier_in = 0;
            self.last_dead_end = 0;
            self.last_candidates = 0;
            set_progress_phase(0);
            return Ok(false);
        }

        let step = self.step;
        let stone = self.stone;
        let no_black_goldish = self.no_black_goldish;
        let position_parallel = self.parallel.min(self.positions.len());
        let position_chunk_size = self.positions.len().div_ceil(position_parallel * 8).max(1);

        // Phase 1: generate candidates in parallel (with filters).
        //
        // Sharded shared dedup: candidates are routed by digest shard to one of
        // NUM_SHARDS shared buckets, each guarded by its own Mutex. Each chunk
        // thread accumulates per-shard locals lock-free, then batch-merges into
        // the shared buckets at chunk end (cross-chunk dedup happens inside the
        // lock). This replaces the older "collect Vec<Vec<Position>>, extend
        // into a single Vec, then global retain" pattern, which materialized
        // every chunk-local-dedupped candidate (~75% later dropped by global
        // dedup) and contributed ~10–20 GB of transient RSS at deep steps.
        //
        // When candidates_limit (= W) is set, each shard holds at most
        // 4 × W / NUM_SHARDS items via Algorithm R. See advance_2ply_fused
        // for the capacity rationale.
        set_progress_phase(1); // P: candidate generation
        let positions = &self.positions;
        let frontier_in = positions.len();
        let dedup_count = AtomicUsize::new(0);
        // Frontier positions with zero filter-passing predecessors: true
        // backward dead-ends (the 2-ply-fusion prize metric).
        let dead_end_count = AtomicUsize::new(0);
        // See advance_2ply_fused for the Bottom-K Sampling rationale.
        let shard_cap = self
            .candidates_limit
            .map(|lim| (lim.saturating_mul(self.candidates_pool_factor)).div_ceil(NUM_SHARDS))
            .unwrap_or(usize::MAX);
        let shard_buckets: Vec<Mutex<ShardBucket>> = (0..NUM_SHARDS)
            .map(|_| Mutex::new(ShardBucket::new(shard_cap)))
            .collect();

        self.install_or_run(|| {
            positions
                .par_chunks(position_chunk_size)
                .enumerate()
                .for_each(|(chunk_idx, chunk)| {
                    let mut undo_moves = vec![];
                    let mut local_seens: [NoHashSet64; NUM_SHARDS] =
                        std::array::from_fn(|_| NoHashSet64::default());
                    let mut local_outs: [Vec<CandRef>; NUM_SHARDS] =
                        std::array::from_fn(|_| Vec::new());
                    let mut chunk_dedup = 0usize;
                    let mut chunk_dead = 0usize;

                    let chunk_base = chunk_idx * position_chunk_size;
                    for (pos_in_chunk, core) in chunk.iter().enumerate() {
                        let frontier_idx = (chunk_base + pos_in_chunk) as u32;
                        let mut position = PositionAux::new(core.clone(), stone);
                        undo_moves.clear();
                        previous(&mut position, step > 0, &mut undo_moves);

                        let mut any_survived = false;
                        for (i1, m) in undo_moves.iter().enumerate() {
                            if !candidate_filter(&position, m) {
                                continue;
                            }
                            let mut pp = position.clone();
                            pp.undo_move(m);
                            if !is_backward_candidate_legal(&mut pp) {
                                continue;
                            }
                            if !satisfies_backward_constraints(&pp, no_black_goldish) {
                                continue;
                            }
                            if !filter(&pp) {
                                continue;
                            }
                            // A constraint-satisfying backward move exists for
                            // this frontier position: not a dead-end.
                            any_survived = true;
                            let digest = pp.core().digest();
                            let shard_idx = shard_index(digest);
                            if local_seens[shard_idx].insert(digest) {
                                local_outs[shard_idx].push(CandRef {
                                    digest,
                                    frontier_idx,
                                    undo1_idx: i1 as u16,
                                    undo2_idx: CandRef::UNDO2_NONE,
                                });
                                chunk_dedup += 1;
                            }
                        }
                        if !any_survived {
                            chunk_dead += 1;
                        }
                    }
                    dedup_count.fetch_add(chunk_dedup, Ordering::Relaxed);
                    dead_end_count.fetch_add(chunk_dead, Ordering::Relaxed);

                    // Release per-thread chunk-local seen sets before taking
                    // shared shard locks.
                    drop(local_seens);

                    // Stagger shard visit order to spread lock contention.
                    let stagger = chunk_idx % NUM_SHARDS;
                    for i in 0..NUM_SHARDS {
                        let shard_idx = (i + stagger) % NUM_SHARDS;
                        let local = std::mem::take(&mut local_outs[shard_idx]);
                        if local.is_empty() {
                            continue;
                        }
                        let mut guard = shard_buckets[shard_idx].lock().unwrap();
                        for cand in local {
                            guard.try_insert(cand);
                        }
                    }
                });
        });

        set_progress_phase(2); // C: collect/extend unique candidates
        let candidate_len = dedup_count.into_inner();
        let dead_end = dead_end_count.into_inner();

        let shard_data: Vec<ShardBucket> = shard_buckets
            .into_iter()
            .map(|m| m.into_inner().unwrap())
            .collect();
        let total_unique: usize = shard_data.iter().map(|b| b.count).sum();
        // build_candidates' truncation limit is `pool_size = candidates_limit
        // × candidates_pool_factor`, NOT candidates_limit itself.
        // candidates_limit (= W) is what Phase V uses for its early-stop
        // target; build_candidates instead caps at the pool size so the
        // mid-set handed to Phase V can be larger than W (necessary when
        // survival rate < 1/pool_factor — otherwise |next| would be capped at
        // W × s < W and the frontier collapses).
        let pool_size = self
            .candidates_limit
            .map(|w| w.saturating_mul(self.candidates_pool_factor));
        let (candidates, p1_sampled) = build_candidates(shard_data, pool_size);
        self.last_sampled = p1_sampled;

        if candidates.is_empty() {
            self.last_frontier_in = frontier_in;
            self.last_dead_end = dead_end;
            self.last_candidates = 0;
            set_progress_phase(0);
            return Ok(false);
        }

        // Phase 2: verify uniqueness in parallel
        let parallel = self.parallel.min(candidates.len());
        // chunk_size = candidates / (parallel*64) で 1 thread あたり ~64 chunks。
        // chunks のコストが大きく不均一 (deep memo searches vs cheap lookups) なので
        // 細かめに分割すると work-stealing が効いて並列効率が改善する。
        // この workload では `*8` (default rayon-ish) → `*32` で wall ~6% 改善。
        let chunk_size = candidates.len().div_ceil(parallel * 64).max(1);
        // Cross-step memo retention policy (threshold = memo_retain_from_step,
        // default DEFAULT_MEMO_RETAIN_FROM_STEP=10, tunable via
        // set_memo_retain_from_step / `--memo-retain-from-step`):
        //  - below threshold: discard. Fresh demand-zero mmap pages beat carrying
        //    stale entries that bloat the table for little benefit in short
        //    searches. (bench_backward_search_seed_sfen at max-step 11 regressed
        //    18% with unconditional retention.)
        //  - at/above threshold: carry forward via std::mem::take. At deep steps
        //    the DFS per candidate is expensive enough that cross-step cache hits
        //    pay off; bench_backward_search_seed_sfen_allowed_kinds at max-step 19
        //    improved 3.3% with retention. Default threshold lowered from 15 to 10
        //    since memo reuse becomes valuable a few steps earlier than originally
        //    tuned. Raising it above the search depth forces always-discard, which
        //    minimizes memo memory (OOM escape hatch) at the cost of cache hits.
        //    StepRange::needs_investigation() guards against stale entries;
        //    shrink_memo() below keeps memory bounded by memo_entry_limit.
        //
        // Tried alternatives (all regressed for short searches):
        //  - clear()/memset on existing buffers (+15%): memset eagerly touches
        //    every page, defeating the demand-zero laziness mmap gives us.
        //  - pre_allocate(limit) on fresh empty memos (+44.5%): heavy upfront
        //    alloc on every step, since most steps don't need full-limit cap.
        let mut memo;
        let mut prev_memo;
        if step >= self.memo_retain_from_step {
            memo = std::mem::take(&mut self.memo);
            prev_memo = std::mem::take(&mut self.prev_memo);
        } else {
            self.memo = Memo::new();
            self.prev_memo = Memo::new();
            memo = Memo::new();
            prev_memo = Memo::new();
        }

        let phase2_start = std::time::Instant::now();

        let mut all_positions = vec![];
        let mut delta_total_count = 0usize;
        let mut phase2_only_ms = 0u128;
        let mut merge_ms = 0u128;

        // Process candidates in waves of (parallel * 8) chunks each.
        // After every wave, deltas are merged into `memo`/`prev_memo` and dropped
        // before the next wave allocates new deltas. This bounds peak delta memory
        // to O(parallel * avg_chunk_delta) instead of O(total_chunks * avg_chunk_delta).
        // Later waves also benefit from earlier waves' merged results as read-cache.
        let wave_chunk_count = parallel * 8;
        let wave_size = chunk_size * wave_chunk_count;
        let canonicalize = self.canonicalize_attacker_goldish;
        let stone_digest = self.stone_digest;
        // Lazy-filter target: when candidates_limit (= W) is set,
        // build_candidates sorted by digest ascending and Phase V processes
        // waves in that order. Each digest prefix is a uniform-random subset
        // of all candidates, so once enough survivors accumulate to fill
        // |next| = W, remaining waves can be skipped without bias.
        let target_w = self.candidates_limit;
        set_progress_phase(3); // V: uniqueness verification waves
        for wave in candidates.chunks(wave_size) {
            if target_w.is_some_and(|w| all_positions.len() >= w) {
                self.last_sampled = true;
                break;
            }
            let memo_ref: &Memo = &memo;
            let prev_memo_ref: &Memo = &prev_memo;
            // Wave-scoped shared cache: canonical digest -> V answer. Goldish-
            // equivalent candidates split across parallel chunks reuse one
            // solutions_overlay result instead of each recomputing it (the
            // chunk-local prev_memo_delta does not cross chunk boundaries
            // within a wave). The frontier still keeps every distinct position
            // -- we only skip the redundant V, then push `core` per candidate
            // exactly as before, so the search space is unchanged. Dropped at
            // wave end => memory returned (OOM-safe). No-op when
            // canonicalization is off (shard locks never taken).
            let shared_v: Option<Vec<Mutex<NoHashMap64<StepRange>>>> = if canonicalize {
                Some(
                    (0..NUM_SHARDS)
                        .map(|_| {
                            Mutex::new(NoHashMap64::with_capacity_and_hasher(
                                256,
                                Default::default(),
                            ))
                        })
                        .collect(),
                )
            } else {
                None
            };
            let shared_v_ref = shared_v.as_deref();

            let wave_start = std::time::Instant::now();
            let wave_results: Vec<(
                Vec<Position>,
                NoHashMap64<StepRange>,
                NoHashMap64<StepRange>,
            )> = self.install_or_run(|| {
                wave.par_chunks(chunk_size)
                    .map(|chunk| {
                        // 初期 capacity を高めに取って rehash (memset 込み) を回避。
                        // 4096 で数 KB の確保コストと引き換えに数回の rehash を skip。
                        // 1024 / 4096 / 16384 を試した結果、4096 が sweet spot。
                        let mut memo_delta =
                            NoHashMap64::with_capacity_and_hasher(4096, Default::default());
                        let mut prev_memo_delta =
                            NoHashMap64::with_capacity_and_hasher(4096, Default::default());
                        let mut prev_positions = vec![];
                        let mut solution_scratch = vec![];
                        let mut killers = Killers::new();
                        let mut history = HistoryTable::new();
                        // Per-chunk undo move buffer reused across q1
                        // reconstructions. 1-ply path only needs one buffer.
                        let mut undo1_buf: Vec<UndoMove> = vec![];

                        // Software prefetch for prev_memo slot lookups.
                        // canonicalize=true 時は実 key (canonical_digest_for_smoke) と
                        // 異なる line を warm するので perf 上は無駄になるが、prefetch
                        // 自体に副作用は無く correctness には影響しない。
                        const PREFETCH_AHEAD: usize = 8;
                        for j in 0..PREFETCH_AHEAD.min(chunk.len()) {
                            prev_memo_ref.prefetch_key(chunk[j].digest ^ stone_digest);
                        }

                        for (i, cand) in chunk.iter().enumerate() {
                            if i + PREFETCH_AHEAD < chunk.len() {
                                prev_memo_ref.prefetch_key(
                                    chunk[i + PREFETCH_AHEAD].digest ^ stone_digest,
                                );
                            }
                            // Reconstruct q1 from
                            // (frontier[frontier_idx], undo1_idx). Same step
                            // value as Phase 1 because advance_parallel_filtered
                            // does not increment self.step between phases (the
                            // increment happens after Phase 2, at end of fn).
                            let frontier_core = &positions[cand.frontier_idx as usize];
                            let mut q0 = PositionAux::new(frontier_core.clone(), stone);
                            undo1_buf.clear();
                            previous(&mut q0, step > 0, &mut undo1_buf);
                            let mut pp = q0;
                            pp.undo_move(&undo1_buf[cand.undo1_idx as usize]);
                            debug_assert_eq!(
                                pp.core().digest(),
                                cand.digest,
                                "reconstructed q1 digest mismatch: \
                                 frontier_idx={} i1={}",
                                cand.frontier_idx,
                                cand.undo1_idx,
                            );

                            let pp_digest = if canonicalize {
                                crate::search::canonicalize::canonical_digest_for_smoke(&pp)
                            } else {
                                cand.digest ^ stone_digest
                            };
                            if let Some(ans) =
                                get_overlay(&prev_memo_delta, prev_memo_ref, pp_digest)
                                    .filter(|ans| !ans.needs_investigation(step + 1))
                            {
                                if ans.is_uniquely(step + 1) {
                                    prev_positions.push(pp.core().clone());
                                }
                                continue;
                            }

                            // Cross-chunk reuse: a sibling chunk in this wave
                            // may have already computed V for this canonical
                            // class. Same trust model as get_overlay above
                            // (skip only when the cached answer is final at
                            // this depth), just sourced from another chunk.
                            if let Some(shards) = shared_v_ref {
                                let cached = shards[shard_index(pp_digest)]
                                    .lock()
                                    .unwrap()
                                    .get(&pp_digest)
                                    .copied();
                                if let Some(ans) = cached
                                    .filter(|ans| !ans.needs_investigation(step + 1))
                                {
                                    // Self-check: the cached verdict must equal
                                    // a fresh recomputation. This turns the
                                    // "memoization soundness + canonical
                                    // determinism" argument into a runtime
                                    // invariant exercised by every debug/test
                                    // run. Zero release cost.
                                    #[cfg(debug_assertions)]
                                    {
                                        let mut pp_chk = pp.clone();
                                        crate::search::canonicalize::canonicalize_attacker_goldish(
                                            &mut pp_chk,
                                        );
                                        let fresh = solutions_overlay(
                                            &mut pp_chk,
                                            prev_memo_ref,
                                            &mut prev_memo_delta,
                                            memo_ref,
                                            &mut memo_delta,
                                            step + 1,
                                            &mut solution_scratch,
                                            &mut killers,
                                            &mut history,
                                        );
                                        debug_assert_eq!(
                                            ans.is_uniquely(step + 1),
                                            fresh.is_uniquely(step + 1),
                                            "shared-V cache verdict mismatch: \
                                             digest={:#x} depth={}",
                                            pp_digest,
                                            step + 1
                                        );
                                    }
                                    if ans.is_uniquely(step + 1) {
                                        prev_positions.push(pp.core().clone());
                                    }
                                    continue;
                                }
                            }

                            let ans = if canonicalize {
                                let mut pp_canonical = pp.clone();
                                crate::search::canonicalize::canonicalize_attacker_goldish(
                                    &mut pp_canonical,
                                );
                                debug_assert_eq!(pp_canonical.digest(), pp_digest);
                                solutions_overlay(
                                    &mut pp_canonical,
                                    prev_memo_ref,
                                    &mut prev_memo_delta,
                                    memo_ref,
                                    &mut memo_delta,
                                    step + 1,
                                    &mut solution_scratch,
                                    &mut killers,
                                    &mut history,
                                )
                            } else {
                                solutions_overlay(
                                    &mut pp,
                                    prev_memo_ref,
                                    &mut prev_memo_delta,
                                    memo_ref,
                                    &mut memo_delta,
                                    step + 1,
                                    &mut solution_scratch,
                                    &mut killers,
                                    &mut history,
                                )
                            };
                            // Publish for sibling chunks in this wave. Only
                            // cache answers that are final at this depth, so
                            // the reuse path's filter is exactly get_overlay's.
                            if let Some(shards) = shared_v_ref {
                                if !ans.needs_investigation(step + 1) {
                                    shards[shard_index(pp_digest)]
                                        .lock()
                                        .unwrap()
                                        .insert(pp_digest, ans);
                                }
                            }
                            if ans.is_uniquely(step + 1) {
                                prev_positions.push(pp.core().clone());
                            }
                        }

                        (prev_positions, memo_delta, prev_memo_delta)
                    })
                    .collect()
            });
            phase2_only_ms += wave_start.elapsed().as_millis();

            let mut wave_memo_deltas = Vec::with_capacity(wave_chunk_count);
            let mut wave_prev_deltas = Vec::with_capacity(wave_chunk_count);
            for (positions, memo_delta, prev_memo_delta) in wave_results {
                all_positions.extend(positions);
                delta_total_count += memo_delta.len() + prev_memo_delta.len();
                wave_memo_deltas.push(memo_delta);
                wave_prev_deltas.push(prev_memo_delta);
            }

            let merge_wave_start = std::time::Instant::now();
            self.install_or_run(|| {
                merge_deltas_sharded(&memo, wave_memo_deltas);
                merge_deltas_sharded(&prev_memo, wave_prev_deltas);
            });
            merge_ms += merge_wave_start.elapsed().as_millis();
        }

        set_progress_phase(4); // F: memo shrink / finalize
        let shrink_start = std::time::Instant::now();
        if let Some(limit) = self.memo_entry_limit {
            if memo.len() >= limit {
                shrink_memo(&mut memo, limit / 2);
            }
            if prev_memo.len() >= limit {
                shrink_memo(&mut prev_memo, limit / 2);
            }
        }
        let shrink_ms = shrink_start.elapsed().as_millis();

        let mut all_positions = all_positions;
        // Truncate to exactly W: the early-stop loop may overshoot by up to
        // one wave. Items are in digest-ascending order so truncating from
        // the tail keeps the smallest-digest W (= uniform random W).
        if let Some(w) = target_w {
            if all_positions.len() > w {
                all_positions.truncate(w);
                self.last_sampled = true;
            }
        }

        if self.delta_trace {
            eprintln!(
                "delta_trace step={} candidates={} phase2_elapsed_ms={} phase2_only_ms={} merge_ms={} shrink_ms={} delta_total={} \
                 memo_size={} prev_memo_size={}",
                step,
                candidate_len,
                phase2_start.elapsed().as_millis(),
                phase2_only_ms,
                merge_ms,
                shrink_ms,
                delta_total_count,
                memo.len(),
                prev_memo.len(),
            );
        }

        self.memo = memo;
        self.prev_memo = prev_memo;

        if all_positions.is_empty() {
            self.last_frontier_in = frontier_in;
            self.last_dead_end = dead_end;
            self.last_candidates = total_unique;
            set_progress_phase(0);
            return Ok(false);
        }

        self.positions = all_positions;
        self.prev_positions = Vec::new();
        std::mem::swap(&mut self.memo, &mut self.prev_memo);
        self.seen_positions = 0;
        self.step += 1;

        self.last_frontier_in = frontier_in;
        self.last_dead_end = dead_end;
        self.last_candidates = total_unique;
        set_progress_phase(0);
        Ok(true)
    }

    pub fn step(&self) -> u16 {
        self.step
    }

    pub fn positions(&self) -> (/* stone */ Option<BitBoard>, &[Position]) {
        (self.stone, &self.positions)
    }

    /// Replace the current frontier with `new`. Used by beam search to
    /// prune the frontier between steps. Resets the per-step processed
    /// counter so the next `advance()` starts fresh.
    pub fn replace_positions(&mut self, new: Vec<Position>) {
        self.positions = new;
        self.seen_positions = 0;
    }

    pub fn stats(&self) -> BackwardSearchStats {
        BackwardSearchStats {
            step: self.step,
            seen_positions: self.seen_positions,
            positions_len: self.positions.len(),
            prev_positions_len: self.prev_positions.len(),
            memo_len: self.memo.len(),
            prev_memo_len: self.prev_memo.len(),
            frontier_in: self.last_frontier_in,
            dead_end_count: self.last_dead_end,
            candidate_count: self.last_candidates,
        }
    }

    pub fn output_positions(
        &self,
        black_position: bool,
        bare_white_king: bool,
    ) -> anyhow::Result<(u16, Vec<PositionAux>)> {
        let step = if self.step > 0 && self.step % 2 == 0 && black_position {
            self.step - 1
        } else {
            self.step
        };

        let mut output_positions = vec![];
        let no_black_goldish = self.no_black_goldish;
        let stone = self.stone;
        // Work directly on self.positions to avoid cloning the entire frontier into
        // an intermediate Vec<PositionAux> before filtering (potential OOM at large frontiers).
        let raw_positions: &[Position] = &self.positions;
        if !black_position || self.step % 2 == 1 || self.step == 0 {
            if self.parallel > 1 && raw_positions.len() > 1 {
                let parallel = self.parallel.min(raw_positions.len());
                let chunk_size = raw_positions.len().div_ceil(parallel * 8).max(1);
                let chunks = self.install_or_run(|| {
                    raw_positions
                        .par_chunks(chunk_size)
                        .map(|chunk| {
                            let mut out = Vec::new();
                            for p in chunk.iter() {
                                if p.pawn_drop() {
                                    continue;
                                }
                                let pa = PositionAux::new(p.clone(), stone);
                                if !satisfies_backward_constraints(&pa, no_black_goldish) {
                                    continue;
                                }
                                if !satisfies_output_constraints(&pa, bare_white_king) {
                                    continue;
                                }
                                out.push(pa);
                            }
                            out
                        })
                        .collect::<Vec<_>>()
                });
                for chunk in chunks {
                    output_positions.extend(chunk);
                }
            } else {
                for p in raw_positions.iter() {
                    if p.pawn_drop() {
                        continue;
                    }
                    let pa = PositionAux::new(p.clone(), stone);
                    if !satisfies_backward_constraints(&pa, no_black_goldish) {
                        continue;
                    }
                    if !satisfies_output_constraints(&pa, bare_white_king) {
                        continue;
                    }
                    output_positions.push(pa);
                }
            }
        } else {
            let desired_step = self.step - 1;
            if self.parallel > 1 && raw_positions.len() > 1 {
                let parallel = self.parallel.min(raw_positions.len());
                let chunk_size = raw_positions.len().div_ceil(parallel * 8).max(1);
                let prev_memo = &self.prev_memo;
                let chunks = self.install_or_run(|| {
                    raw_positions
                        .par_chunks(chunk_size)
                        .map(|chunk| -> anyhow::Result<Vec<PositionAux>> {
                            let mut out = Vec::new();
                            for p in chunk.iter() {
                                if p.pawn_drop() {
                                    continue;
                                }
                                debug_assert_eq!(p.turn(), Color::WHITE);
                                let mut position = PositionAux::new(p.clone(), stone);
                                let mut movements = vec![];
                                advance_aux(&mut position, &Default::default(), &mut movements)?;
                                for m in movements.iter() {
                                    let digest = position.moved_digest(m);
                                    let unique = if let Some(range) = prev_memo.get(digest) {
                                        range.is_uniquely(desired_step)
                                    } else {
                                        let mut np = position.clone();
                                        np.do_move(m);
                                        let sols = standard_solve(np, 2, true)?.solutions();
                                        sols.len() == 1 && sols[0].len() == desired_step as usize
                                    };
                                    if !unique {
                                        continue;
                                    }
                                    let mut np = position.clone();
                                    np.do_move(m);
                                    if !satisfies_backward_constraints(&np, no_black_goldish) {
                                        continue;
                                    }
                                    if !satisfies_output_constraints(&np, bare_white_king) {
                                        continue;
                                    }
                                    out.push(np);
                                }
                            }
                            Ok(out)
                        })
                        .collect::<Vec<_>>()
                });
                for chunk in chunks {
                    output_positions.extend(chunk?);
                }
            } else {
                for p in raw_positions.iter() {
                    if p.pawn_drop() {
                        continue;
                    }
                    debug_assert_eq!(p.turn(), Color::WHITE);
                    let mut position = PositionAux::new(p.clone(), stone);
                    let mut movements = vec![];
                    advance_aux(&mut position, &Default::default(), &mut movements)?;
                    for m in movements.iter() {
                        let digest = position.moved_digest(m);
                        let unique = if let Some(range) = self.prev_memo.get(digest) {
                            range.is_uniquely(desired_step)
                        } else {
                            let mut np = position.clone();
                            np.do_move(m);
                            let sols = standard_solve(np, 2, true)?.solutions();
                            sols.len() == 1 && sols[0].len() == desired_step as usize
                        };
                        if !unique {
                            continue;
                        }
                        let mut np = position.clone();
                        np.do_move(m);
                        if !satisfies_backward_constraints(&np, no_black_goldish) {
                            continue;
                        }
                        if !satisfies_output_constraints(&np, bare_white_king) {
                            continue;
                        }
                        output_positions.push(np);
                    }
                }
            }
        }

        // 呼び出し側 (`merge_best` → `finalize_best`、`dedup_positions` 経由) で
        // 最終的に sfen sort されるので、ここでの sort は冗長。
        Ok((step, output_positions))
    }

    pub fn forward(&mut self) {
        if self.solution.is_empty() {
            return;
        }
        self.initial_position.do_move(&self.solution.remove(0));
        self.positions = vec![self.initial_position.core().clone()];
        self.prev_positions = Vec::new();
        std::mem::swap(&mut self.memo, &mut self.prev_memo);
        self.seen_positions = 0;
        self.step = self.solution.len() as u16;
    }
}

#[inline(always)]
fn is_backward_candidate_legal(position: &mut PositionAux) -> bool {
    if position.turn().is_white() {
        let Some(att) =
            crate::position::advance::attack_prevent::attacker(position, Color::WHITE, false)
        else {
            return false;
        };
        if position.checked_slow(Color::BLACK) {
            return false;
        }
        if let Some((pos2, kind2)) = att.double_check {
            let king_pos = position.king_pos(Color::WHITE).unwrap();
            let (pos1, kind1) = (att.pos, att.kind);

            let dist = |pos: crate::position::Square| -> usize {
                let dx = (pos.col() as isize - king_pos.col() as isize).abs();
                let dy = (pos.row() as isize - king_pos.row() as isize).abs();
                std::cmp::max(dx, dy) as usize
            };

            let is_slider = |kind: crate::piece::Kind| -> bool {
                matches!(
                    kind,
                    crate::piece::Kind::Lance
                        | crate::piece::Kind::Bishop
                        | crate::piece::Kind::Rook
                        | crate::piece::Kind::ProBishop
                        | crate::piece::Kind::ProRook
                )
            };

            let possible =
                (is_slider(kind1) && dist(pos1) >= 2) || (is_slider(kind2) && dist(pos2) >= 2);
            if !possible {
                return false;
            }
        }
    } else if position.checked_slow(Color::WHITE) {
        return false;
    }
    true
}

#[inline(always)]
fn satisfies_backward_constraints(position: &PositionAux, no_black_goldish: bool) -> bool {
    !no_black_goldish || black_goldish(position).is_empty()
}

#[inline(always)]
fn black_goldish(position: &PositionAux) -> BitBoard {
    position.bitboard(Color::BLACK, Kind::Gold)
        | position.bitboard(Color::BLACK, Kind::ProPawn)
        | position.bitboard(Color::BLACK, Kind::ProLance)
        | position.bitboard(Color::BLACK, Kind::ProKnight)
        | position.bitboard(Color::BLACK, Kind::ProSilver)
}

#[inline(always)]
fn satisfies_output_constraints(position: &PositionAux, bare_white_king: bool) -> bool {
    !bare_white_king || is_bare_white_king(position)
}

#[inline(always)]
fn is_bare_white_king(position: &PositionAux) -> bool {
    position.white_bb() == position.bitboard(Color::WHITE, Kind::King)
}

const INF_START: u16 = u16::MAX - 2;
const INF_END: u16 = u16::MAX - 1;

/// Apply killer-move and history-heuristic ordering in-place.
/// Swaps up to KILLER_COUNT remembered cutoff moves to the front, then brings
/// the top-3 non-killer moves by history score forward. Used by both the
/// sequential (`solutions_inner`) and parallel (`solutions_overlay_inner`) paths.
#[inline]
fn apply_move_ordering(
    movements: &mut Vec<Movement>,
    killers: &Killers,
    history: &HistoryTable,
    mate_in: u16,
) {
    let killer_slots = killers.slots(mate_in);
    let mut next_swap = 0;
    for k in killer_slots.iter().flatten() {
        if let Some(rel_idx) = movements[next_swap..].iter().position(|m| m == k) {
            let idx = next_swap + rel_idx;
            if idx != next_swap {
                movements.swap(next_swap, idx);
            }
            next_swap += 1;
            if next_swap >= movements.len() {
                break;
            }
        }
    }
    let hist_start = next_swap;
    for slot in 0..3usize {
        let from = hist_start + slot;
        if from >= movements.len() {
            break;
        }
        let mut best_score = history.score(mate_in, &movements[from]);
        let mut best_idx = from;
        for j in (from + 1)..movements.len() {
            let s = history.score(mate_in, &movements[j]);
            if s > best_score {
                best_score = s;
                best_idx = j;
            }
        }
        if best_score > 0 && best_idx != from {
            movements.swap(from, best_idx);
        }
    }
}

fn solutions(
    position: &mut PositionAux,
    memo: &Memo,
    next_memo: &Memo,
    mate_in: u16,
    scratch: &mut Vec<Vec<Movement>>,
    memo_entry_limit: Option<usize>,
    killers: &mut Killers,
    history: &mut HistoryTable,
) -> StepRange {
    if scratch.len() <= mate_in as usize {
        scratch.resize_with(mate_in as usize + 1, Vec::new);
    }
    solutions_inner(
        position,
        memo,
        next_memo,
        mate_in,
        scratch,
        memo_entry_limit,
        killers,
        history,
    )
}

fn solutions_inner(
    position: &mut PositionAux,
    memo: &Memo,
    next_memo: &Memo,
    mate_in: u16,
    scratch: &mut [Vec<Movement>],
    memo_entry_limit: Option<usize>,
    killers: &mut Killers,
    history: &mut HistoryTable,
) -> StepRange {
    let mut ans = StepRange::unknown();
    if let Some(a) = memo.get(position.digest()) {
        if !a.needs_investigation(mate_in) {
            return a;
        }
        ans = a;
    }

    if mate_in == 0 {
        let mut movements = std::mem::take(&mut scratch[0]);
        movements.clear();
        let options = crate::position::AdvanceOptions {
            max_allowed_branches: Some(0),
            ..Default::default()
        };
        let advance_result = advance_aux(position, &options, &mut movements);
        let hint = if advance_result.is_err() {
            StepRange::non_zero()
        } else if advance_result.unwrap() {
            StepRange::exact(0)
        } else if movements.is_empty() {
            StepRange::unsolvable()
        } else {
            StepRange::non_zero()
        };
        let ans = ans.intersection(&hint);
        debug_assert!(!ans.needs_investigation(mate_in));
        memo_insert(memo, position.digest(), ans, memo_entry_limit);
        scratch[0] = movements;
        return ans;
    }

    let scratch_index = mate_in as usize;
    let mut movements = std::mem::take(&mut scratch[scratch_index]);
    movements.clear();
    let is_mate = advance_aux(position, &Default::default(), &mut movements).unwrap();

    let mut hint = StepRange::unknown();
    if is_mate {
        hint = StepRange::exact(0);
        debug_assert!(!hint.needs_investigation(mate_in));
    } else if movements.is_empty() {
        hint = StepRange::unsolvable();
        debug_assert!(!hint.needs_investigation(mate_in));
    } else if mate_in == 0 {
        hint = StepRange::non_zero();
    }
    ans = ans.intersection(&hint);
    if !ans.needs_investigation(mate_in) {
        memo_insert(memo, position.digest(), ans, memo_entry_limit);
        scratch[scratch_index] = movements;
        return ans;
    }

    let mut res = StepRange::unsolvable();

    apply_move_ordering(&mut movements, killers, history, mate_in);

    // Pre-compute child digests and issue software prefetches so the memo
    // cache lines are warm before the pass-1 lookup loop (same technique as
    // solutions_overlay_inner; hides DRAM latency behind moved_digest work).
    let nchildren = movements.len().min(128);
    let mut child_digests = [0u64; 128];
    for i in 0..nchildren {
        let d = position.moved_digest(&movements[i]);
        child_digests[i] = d;
        next_memo.prefetch_key(d);
    }

    // Two-pass move ordering: memoized children first; skip recursion if those
    // alone prove non-uniqueness or a shorter mate. hit_mask records pass-1 hits.
    let mut hit_mask = [0u64; 2];
    for (i, m) in movements.iter().enumerate() {
        let child_digest = if i < nchildren {
            child_digests[i]
        } else {
            position.moved_digest(m)
        };
        if let Some(child) = next_memo
            .get(child_digest)
            .filter(|a| !a.needs_investigation(mate_in - 1))
        {
            hit_mask[i / 64] |= 1u64 << (i % 64);
            let a = child.inc();
            res.update_with_child(&a);
            if res.definitely_shorter_or_non_unique(mate_in) {
                killers.record(mate_in, *m);
                history.record(mate_in, m);
                res.shortest_start = 1;
                res.next_start = 1;
                break;
            }
        }
    }

    if !res.definitely_shorter_or_non_unique(mate_in) {
        for (i, m) in movements.iter().enumerate() {
            if hit_mask[i / 64] & (1u64 << (i % 64)) != 0 {
                continue;
            }
            let mut np = position.clone();
            np.do_move(m);
            let a = solutions_inner(
                &mut np,
                next_memo,
                memo,
                mate_in - 1,
                scratch,
                memo_entry_limit,
                killers,
                history,
            )
            .inc();
            debug_assert!(!a.needs_investigation(mate_in));

            res.update_with_child(&a);

            if res.definitely_shorter_or_non_unique(mate_in) {
                killers.record(mate_in, *m);
                history.record(mate_in, m);
                res.shortest_start = 1;
                res.next_start = 1;
                break;
            }
        }
    }

    res = res.intersection(&ans);

    debug_assert!(
        !res.needs_investigation(mate_in),
        "{:?} {:?} {:?} {}",
        res,
        hint,
        position,
        mate_in
    );

    memo_insert(memo, position.digest(), res, memo_entry_limit);
    scratch[scratch_index] = movements;
    res
}

#[inline(always)]
fn memo_insert(memo: &Memo, digest: u64, value: StepRange, memo_entry_limit: Option<usize>) {
    if let Some(limit) = memo_entry_limit {
        if memo.len() >= limit {
            // SAFETY: sequential path is single-threaded; no concurrent writers.
            unsafe { shrink_memo_unsynchronized(memo, limit / 2) };
        }
    }
    // SAFETY: sequential path is single-threaded; no concurrent writers.
    unsafe { memo.insert_unsynchronized(digest, value) };
}

fn shrink_memo(memo: &mut Memo, target_len: usize) {
    memo.shrink_to_keep(target_len, memo_retention_score);
}

/// Sequential-path shrink (called from `memo_insert` in `solutions_inner`).
/// SAFETY: caller must ensure no concurrent writers (other readers OK).
unsafe fn shrink_memo_unsynchronized(memo: &Memo, target_len: usize) {
    if memo.len() <= target_len {
        return;
    }
    let to_remove = memo.len() - target_len;
    let mut entries = memo
        .iter()
        .map(|(k, v)| (memo_retention_score(k, v), k))
        .collect::<Vec<_>>();
    entries.select_nth_unstable_by_key(to_remove - 1, |&(score, _)| score);
    for &(_, key) in &entries[..to_remove] {
        unsafe { memo.remove_unsynchronized(key) };
    }
}

fn memo_retention_score(digest: u64, range: StepRange) -> u64 {
    let class = if range.is_unknown() {
        0
    } else if range.is_non_zero_hint() {
        1
    } else if range.is_unsolvable() {
        5
    } else if range.is_exact_shortest() {
        6
    } else if range.has_finite_shortest() {
        4
    } else {
        3
    };
    let specificity = u32::MAX - range.uncertainty_width();
    let tie_breaker = digest.wrapping_mul(0x9E37_79B9_7F4A_7C15) >> 48;
    (class << 56) | ((specificity as u64) << 16) | tie_breaker
}

#[inline(always)]
fn get_overlay(delta: &NoHashMap64<StepRange>, base: &Memo, digest: u64) -> Option<StepRange> {
    delta.get(&digest).copied().or_else(|| base.get(digest))
}

/// Per-chunk killer move table indexed by mate_in.
/// Records up to KILLER_COUNT recent moves that caused a cutoff at each mate_in
/// level so pass-1 can try them first for subsequent calls. Persistent across
/// the chunk (thousands of candidates) but reset per chunk to avoid stale moves
/// leaking across thread boundaries.
const KILLER_DEPTH: usize = 64;
const KILLER_COUNT: usize = 5;

struct Killers {
    by_mate_in: [[Option<Movement>; KILLER_COUNT]; KILLER_DEPTH],
}

impl Killers {
    fn new() -> Self {
        Self {
            by_mate_in: [const { [const { None }; KILLER_COUNT] }; KILLER_DEPTH],
        }
    }

    #[inline(always)]
    fn slots(&self, mate_in: u16) -> &[Option<Movement>; KILLER_COUNT] {
        // Out-of-range safe-guard; in practice mate_in stays well below 64.
        self.by_mate_in
            .get(mate_in as usize)
            .unwrap_or(&[const { None }; KILLER_COUNT])
    }

    #[inline(always)]
    fn record(&mut self, mate_in: u16, m: Movement) {
        if let Some(slot) = self.by_mate_in.get_mut(mate_in as usize) {
            if slot[0] == Some(m) {
                return; // already at top
            }
            // LRU shift: push older killer down, new at top.
            for i in (1..KILLER_COUNT).rev() {
                slot[i] = slot[i - 1];
            }
            slot[0] = Some(m);
        }
    }
}

/// History heuristic table indexed by (mate_in, kind, dest_square).
///
/// Accumulates cutoff counts per (piece_kind × dest_square) pair across all
/// positions in a chunk.  After killer swaps, the top-3 non-killer moves by
/// history score are swapped to the front so pass-1 evaluates them early.
///
/// Table size: 64 × (14 × 81) × u16 ≈ 145 KB — fits in L2.
const HIST_DEPTH: usize = KILLER_DEPTH;
const HIST_STRIDE: usize = crate::piece::NUM_KIND * 81; // 14 × 81 = 1134

struct HistoryTable {
    counts: [[u16; HIST_STRIDE]; HIST_DEPTH],
}

impl HistoryTable {
    fn new() -> Self {
        Self {
            counts: [[0u16; HIST_STRIDE]; HIST_DEPTH],
        }
    }

    #[inline(always)]
    fn record(&mut self, mate_in: u16, m: &Movement) {
        let idx = movement_hist_idx(m);
        if let Some(row) = self.counts.get_mut(mate_in as usize) {
            row[idx] = row[idx].saturating_add(1);
        }
    }

    #[inline(always)]
    fn score(&self, mate_in: u16, m: &Movement) -> u16 {
        let idx = movement_hist_idx(m);
        self.counts.get(mate_in as usize).map_or(0, |row| row[idx])
    }
}

/// Compute the history table index for a movement: kind.index() * 81 + dest.index().
/// For Move without source_kind_hint, we fall back to dest-only (kind_idx = 0).
#[inline(always)]
fn movement_hist_idx(m: &Movement) -> usize {
    match m {
        Movement::Drop(sq, kind) => kind.index() * 81 + sq.index(),
        Movement::Move {
            dest,
            source_kind_hint,
            ..
        } => {
            let kind_idx = source_kind_hint.map_or(0, |k| k.index());
            kind_idx * 81 + dest.index()
        }
    }
}

fn solutions_overlay(
    position: &mut PositionAux,
    memo_base: &Memo,
    memo_delta: &mut NoHashMap64<StepRange>,
    next_memo_base: &Memo,
    next_memo_delta: &mut NoHashMap64<StepRange>,
    mate_in: u16,
    scratch: &mut Vec<Vec<Movement>>,
    killers: &mut Killers,
    history: &mut HistoryTable,
) -> StepRange {
    if scratch.len() <= mate_in as usize {
        scratch.resize_with(mate_in as usize + 1, Vec::new);
    }
    solutions_overlay_inner(
        position,
        memo_base,
        memo_delta,
        next_memo_base,
        next_memo_delta,
        mate_in,
        scratch,
        killers,
        history,
    )
}

#[inline]
fn solutions_overlay_inner(
    position: &mut PositionAux,
    memo_base: &Memo,
    memo_delta: &mut NoHashMap64<StepRange>,
    next_memo_base: &Memo,
    next_memo_delta: &mut NoHashMap64<StepRange>,
    mate_in: u16,
    scratch: &mut [Vec<Movement>],
    killers: &mut Killers,
    history: &mut HistoryTable,
) -> StepRange {
    let digest = position.digest();
    let mut ans = StepRange::unknown();
    if let Some(a) = get_overlay(memo_delta, memo_base, digest) {
        if !a.needs_investigation(mate_in) {
            return a;
        }
        ans = a;
    }

    if mate_in == 0 {
        let mut movements = std::mem::take(&mut scratch[0]);
        movements.clear();
        let options = crate::position::AdvanceOptions {
            max_allowed_branches: Some(0),
            ..Default::default()
        };
        let advance_result = advance_aux(position, &options, &mut movements);
        let hint = if advance_result.is_err() {
            StepRange::non_zero()
        } else if advance_result.unwrap() {
            StepRange::exact(0)
        } else if movements.is_empty() {
            StepRange::unsolvable()
        } else {
            StepRange::non_zero()
        };
        let ans = ans.intersection(&hint);
        debug_assert!(!ans.needs_investigation(mate_in));
        if should_memoize(ans) {
            memo_delta.insert(digest, ans);
        }
        scratch[0] = movements;
        return ans;
    }

    let scratch_index = mate_in as usize;
    let mut movements = std::mem::take(&mut scratch[scratch_index]);
    movements.clear();
    let is_mate = advance_aux(position, &Default::default(), &mut movements).unwrap();

    let mut hint = StepRange::unknown();
    if is_mate {
        hint = StepRange::exact(0);
    } else if movements.is_empty() {
        hint = StepRange::unsolvable();
    } else if mate_in == 0 {
        hint = StepRange::non_zero();
    }
    ans = ans.intersection(&hint);
    if !ans.needs_investigation(mate_in) {
        if should_memoize(ans) {
            memo_delta.insert(digest, ans);
        }
        scratch[scratch_index] = movements;
        return ans;
    }

    let mut res = StepRange::unsolvable();

    // Two-pass move ordering: first pass checks memoized children only.
    // If any combination of those is enough to prove non-uniqueness or a
    // shorter mate, we skip the recursive descent for the unmemoized moves.
    // hit_mask records pass-1 hits so pass 2 can skip them. Stack-allocated
    // [u64; 2] supports up to 128 movements; any practical position has fewer.
    //
    // Pre-compute child digests and issue software prefetches so the base-memo
    // FlatShard cache lines are warm before the pass-1 lookup loop.  The
    // FlatShard arrays are multi-hundred-MB and essentially never in L3 cache;
    // without prefetch every `base.get()` call stalls ~200 cycles waiting for
    // DRAM.  With prefetch the stalls overlap with the (cheap) moved_digest
    // computations, cutting per-position latency from O(N × miss) to O(miss).
    // Killer heuristic: swap up to KILLER_COUNT remembered cutoff moves to the
    // front of the movements list so pass-1 evaluates them first. The full
    // prefetch loop still warms cache lines for ALL children — we just bias
    // which one's lookup result is consumed first by the cutoff check.
    // History heuristic: after killers, swap the non-killer move with the
    // highest history score (cutoff frequency by dest square) to the next slot.
    apply_move_ordering(&mut movements, killers, history, mate_in);

    let nchildren = movements.len().min(128);
    let mut child_digests = [0u64; 128];
    for i in 0..nchildren {
        let d = position.moved_digest(&movements[i]);
        child_digests[i] = d;
        next_memo_base.prefetch_key(d);
    }

    let mut hit_mask = [0u64; 2];
    for (i, m) in movements.iter().enumerate() {
        let child_digest = if i < nchildren {
            child_digests[i]
        } else {
            position.moved_digest(m)
        };
        if let Some(child) = get_overlay(next_memo_delta, next_memo_base, child_digest)
            .filter(|a| !a.needs_investigation(mate_in - 1))
        {
            hit_mask[i / 64] |= 1u64 << (i % 64);
            let a = child.inc();
            res.update_with_child(&a);
            if res.definitely_shorter_or_non_unique(mate_in) {
                killers.record(mate_in, *m);
                history.record(mate_in, m);
                res.shortest_start = 1;
                res.next_start = 1;
                break;
            }
        }
    }

    if !res.definitely_shorter_or_non_unique(mate_in) {
        for (i, m) in movements.iter().enumerate() {
            if hit_mask[i / 64] & (1u64 << (i % 64)) != 0 {
                continue;
            }
            let mut np = position.clone();
            np.do_move(m);
            let a = solutions_overlay_inner(
                &mut np,
                next_memo_base,
                next_memo_delta,
                memo_base,
                memo_delta,
                mate_in - 1,
                scratch,
                killers,
                history,
            )
            .inc();
            debug_assert!(!a.needs_investigation(mate_in));

            res.update_with_child(&a);

            if res.definitely_shorter_or_non_unique(mate_in) {
                killers.record(mate_in, *m);
                history.record(mate_in, m);
                res.shortest_start = 1;
                res.next_start = 1;
                break;
            }
        }
    }

    res = res.intersection(&ans);

    debug_assert!(
        !res.needs_investigation(mate_in),
        "{:?} {:?} {:?} {}",
        res,
        hint,
        position,
        mate_in
    );

    if should_memoize(res) {
        memo_delta.insert(digest, res);
    }
    scratch[scratch_index] = movements;
    res
}

/// Gate: skip memoizing entries that contribute little to future cache hits.
///
/// `non_zero_hint` (e.g. "this position takes >0 moves to mate") is one of the
/// most common results at depth-0 leaves, but its information content is small
/// — it's a quick fact callers can recompute via a single `advance_aux`. Storing
/// these in the memo balloons the delta to ~100M entries on heavy steps and
/// dominates merge + shrink cost.
///
/// We still memoize:
///   - exact(K)              (definitive shortest-mate)
///   - has_finite_shortest   (bounded shortest range)
///   - unsolvable            (definitive no-mate)
///   - other refinements with finite info
#[inline(always)]
fn should_memoize(range: StepRange) -> bool {
    !range.is_non_zero_hint() && !range.is_unknown()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct StepRange {
    // Second shortest solution range
    next_start: u16,
    next_end: u16,
    // Shortest solution range
    shortest_start: u16,
    shortest_end: u16,
}

#[inline(always)]
fn intersection_bounds(a_start: u16, a_end: u16, b_start: u16, b_end: u16) -> (u16, u16) {
    let start = a_start.max(b_start);
    let end = a_end.min(b_end);
    if start >= end {
        (0, 0)
    } else {
        (start, end)
    }
}

#[inline(always)]
fn definitely_shorter(start: u16, end: u16, step: u16) -> bool {
    let (start, end) = intersection_bounds(start, end, step, INF_END);
    start >= end
}

#[inline(always)]
fn definitely_longer(start: u16, end: u16, step: u16) -> bool {
    let (start, end) = intersection_bounds(start, end, 0, step + 1);
    start >= end
}

#[inline(always)]
fn exactly(start: u16, end: u16, step: u16) -> bool {
    start == step && end == step + 1
}

impl StepRange {
    #[inline(always)]
    fn new(mut shortest: Range<u16>, mut next: Range<u16>) -> Self {
        debug_assert!(shortest.start <= next.start);
        debug_assert!(shortest.end <= next.end);

        shortest.start = shortest.start.min(INF_START);
        shortest.end = shortest.end.min(INF_END);
        next.start = next.start.min(INF_START);
        next.end = next.end.min(INF_END);

        StepRange {
            shortest_start: shortest.start,
            shortest_end: shortest.end,
            next_start: next.start,
            next_end: next.end,
        }
    }

    #[inline(always)]
    fn exact(step: u16) -> Self {
        Self::new(step..step + 1, step + 1..INF_END)
    }

    #[inline(always)]
    fn unsolvable() -> Self {
        Self::new(INF_START..INF_END, INF_START..INF_END)
    }

    #[inline(always)]
    fn unknown() -> Self {
        Self::new(0..INF_END, 0..INF_END)
    }

    #[inline(always)]
    fn non_zero() -> Self {
        Self::new(1..INF_END, 1..INF_END)
    }

    #[inline(always)]
    fn is_unknown(&self) -> bool {
        self.shortest_start == 0
            && self.shortest_end == INF_END
            && self.next_start == 0
            && self.next_end == INF_END
    }

    #[inline(always)]
    fn is_non_zero_hint(&self) -> bool {
        self.shortest_start == 1
            && self.shortest_end == INF_END
            && self.next_start == 1
            && self.next_end == INF_END
    }

    #[inline(always)]
    fn is_unsolvable(&self) -> bool {
        self.shortest_start >= INF_START && self.next_start >= INF_START
    }

    #[inline(always)]
    fn has_finite_shortest(&self) -> bool {
        self.shortest_start < INF_START
    }

    #[inline(always)]
    fn is_exact_shortest(&self) -> bool {
        self.has_finite_shortest() && self.shortest_end == self.shortest_start + 1
    }

    #[inline(always)]
    fn uncertainty_width(&self) -> u32 {
        u32::from(self.shortest_end - self.shortest_start)
            + u32::from(self.next_end - self.next_start)
    }

    #[inline(always)]
    fn inc(&self) -> Self {
        Self::new(
            self.shortest_start + 1..self.shortest_end + 1,
            self.next_start + 1..self.next_end + 1,
        )
    }

    #[inline(always)]
    fn definitely_shorter_or_non_unique(&self, step: u16) -> bool {
        self.shortest_end <= step || self.shortest_end == step + 1 && self.next_end == step + 1
    }

    #[inline(always)]
    fn needs_investigation(&self, mate_in: u16) -> bool {
        if self.definitely_shorter_or_non_unique(mate_in)
            || definitely_longer(self.shortest_start, self.shortest_end, mate_in)
        {
            return false;
        }
        if exactly(self.shortest_start, self.shortest_end, mate_in) {
            debug_assert!(!definitely_shorter(self.next_start, self.next_end, mate_in));
            if definitely_longer(self.next_start, self.next_end, mate_in)
                || exactly(self.next_start, self.next_end, mate_in)
            {
                return false;
            }
        }
        true
    }

    #[inline(always)]
    fn intersection(&self, hint: &StepRange) -> StepRange {
        let (shortest_start, shortest_end) = intersection_bounds(
            self.shortest_start,
            self.shortest_end,
            hint.shortest_start,
            hint.shortest_end,
        );
        let (next_start, next_end) = intersection_bounds(
            self.next_start,
            self.next_end,
            hint.next_start,
            hint.next_end,
        );
        Self::new(shortest_start..shortest_end, next_start..next_end)
    }

    #[inline(always)]
    fn update_with_child(&mut self, c: &StepRange) {
        for (start, end) in [
            (c.shortest_start, c.shortest_end),
            (c.next_start, c.next_end),
        ] {
            if start < self.shortest_start {
                self.next_start = self.shortest_start;
                self.shortest_start = start;
            } else if start < self.next_start {
                self.next_start = start;
            }

            if end < self.shortest_end {
                self.next_end = self.shortest_end;
                self.shortest_end = end;
            } else if end < self.next_end {
                self.next_end = end;
            }
        }
    }

    #[inline(always)]
    fn is_uniquely(&self, step: u16) -> bool {
        exactly(self.shortest_start, self.shortest_end, step)
            && definitely_longer(self.next_start, self.next_end, step)
    }
}

#[cfg(test)]
mod tests {
    use super::{memo_retention_score, shrink_memo, Memo, StepRange};
    use crate::{
        position::position::PositionAux,
        search::backward::{backward_initial_variants, backward_search},
    };

    #[test]
    fn memo_shrink_keeps_more_informative_entries() {
        let mut memo = Memo::new();
        memo.insert(1, StepRange::unknown());
        memo.insert(2, StepRange::non_zero());
        memo.insert(3, StepRange::unsolvable());
        memo.insert(4, StepRange::exact(7));

        assert!(
            memo_retention_score(4, StepRange::exact(7))
                > memo_retention_score(1, StepRange::unknown())
        );
        shrink_memo(&mut memo, 2);

        assert_eq!(memo.len(), 2);
        assert!(memo.contains_key(3));
        assert!(memo.contains_key(4));
    }

    #[test]
    fn test_backward_search() {
        for (sfen, (want_step, mut want_sfens)) in [
            (
                "9/9/9/9/9/6OOO/6O1k/6OO+P/8P w - 1",
                (1, vec!["9/9/9/9/9/6OOO/6O1k/6OO1/7+PP b - 1"]),
            ),
            (
                "9/9/9/7OO/7Ok/7OP/7O1/7O1/7OL w - 1",
                (3, vec!["9/9/9/7OO/7O1/7Ok/7O1/7OP/7OL b - 1"]),
            ),
            (
                "9/9/9/7OO/7Ok/7O1/7OP/7O1/7OL b - 1",
                (3, vec!["9/9/9/7OO/7O1/7Ok/7O1/7OP/7OL b - 1"]),
            ),
            (
                "9/9/9/9/9/5OOOO/5OR1k/5O1p1/5O2P w - 1",
                (
                    19,
                    vec![
                        "9/9/9/9/9/5OOOO/5O2+p/5Ok+p1/5O2R b - 1",
                        "9/9/9/9/9/5OOOO/5O2R/5Ok+p1/5O2+p b - 1",
                        "9/9/9/9/9/5OOOO/5O2p/5Ok+p1/5O2R b - 1",
                    ],
                ),
            ),
            (
                "6ppp/6P2/9/9/9/5OOOO/5O2k/5O1PR/5O2P w - 1",
                (0, vec!["6ppp/6P2/9/9/9/5OOOO/5O2k/5O1PR/5O2P w - 1"]),
            ),
        ] {
            let initial_position = PositionAux::from_sfen(sfen).unwrap();
            let (step, mut positions) = backward_search(&initial_position, true, 0, false).unwrap();

            assert_eq!(step, want_step, "{:?}", initial_position);

            want_sfens.sort();
            let want_positions = want_sfens
                .iter()
                .map(|sfen| PositionAux::from_sfen(sfen).unwrap())
                .collect::<Vec<_>>();

            positions.sort_by_key(|a| a.clone().sfen());

            assert_eq!(positions, want_positions)
        }
    }

    #[test]
    fn test_backward_parallel_dashmap_vs_legacy() {
        let sfen = "9/9/9/9/9/5OOOO/5OR1k/5O1p1/5O2P w - 1";
        let initial_position = PositionAux::from_sfen(sfen).unwrap();

        let mut search =
            super::BackwardSearch::new_with_parallel(&initial_position, false, 2, false).unwrap();

        while search.advance().unwrap() {}

        assert!(search.step() > 0);
    }

    /// Sharded Phase 1 (advance_parallel_filtered) と sequential path
    /// (advance_upto) が同じ frontier に到達することを各 step で確認する。
    /// 直前の "Vec<Vec<Position>> + global retain" → sharded shared dedup へ
    /// の置き換えで、digest 分割や cross-chunk dedup の欠落、shard 単位の
    /// 取りこぼしが起きないことを保証する。
    #[test]
    fn advance_parallel_filtered_matches_sequential_each_step() {
        let sfen = "9/9/9/9/9/5OOOO/5OR1k/5O1p1/5O2P w - 1";
        let initial_position = PositionAux::from_sfen(sfen).unwrap();

        let mut seq =
            super::BackwardSearch::new_with_parallel(&initial_position, false, 1, false).unwrap();
        let mut par =
            super::BackwardSearch::new_with_parallel(&initial_position, false, 4, false).unwrap();

        loop {
            let seq_advanced = seq.advance().unwrap();
            let par_advanced = par.advance().unwrap();
            assert_eq!(
                seq_advanced, par_advanced,
                "advance() return differs at step {}",
                seq.step()
            );
            assert_eq!(seq.step(), par.step(), "step diverged");

            let mut seq_pos: Vec<_> = seq.positions().1.to_vec();
            let mut par_pos: Vec<_> = par.positions().1.to_vec();
            assert_eq!(
                seq_pos.len(),
                par_pos.len(),
                "frontier size diverged at step {} (seq={} par={})",
                seq.step(),
                seq_pos.len(),
                par_pos.len(),
            );
            seq_pos.sort_by_key(|p| p.digest());
            par_pos.sort_by_key(|p| p.digest());
            assert_eq!(
                seq_pos,
                par_pos,
                "frontier content diverged at step {}",
                seq.step()
            );

            if !seq_advanced {
                break;
            }
        }
        assert!(seq.step() > 1, "needs a deep enough search to exercise dedup");
    }

    /// 各 step で frontier の digest がすべて unique であることを確認する。
    /// Sharded Phase 1 の cross-chunk dedup が抜け落ちると同じ position が
    /// 複数 shard 経由ではなく同じ shard 内で重複して残る可能性があるので
    /// 直接の保証として欲しい。
    #[test]
    fn advance_parallel_filtered_frontier_is_unique() {
        let sfen = "9/9/9/9/9/5OOOO/5OR1k/5O1p1/5O2P w - 1";
        let initial_position = PositionAux::from_sfen(sfen).unwrap();
        let mut search =
            super::BackwardSearch::new_with_parallel(&initial_position, false, 4, false).unwrap();

        while search.advance().unwrap() {
            let positions = search.positions().1;
            let original_len = positions.len();
            let mut digests: Vec<u64> = positions.iter().map(|p| p.digest()).collect();
            digests.sort_unstable();
            digests.dedup();
            assert_eq!(
                digests.len(),
                original_len,
                "frontier has duplicates at step {}",
                search.step()
            );
        }
    }

    /// Parallel 度を変えても最終 frontier が一致することを確認 (NUM_SHARDS
    /// との相互作用を含む sharded 振り分けの不変性検証)。
    #[test]
    fn advance_parallel_filtered_invariant_under_parallelism() {
        let sfen = "9/9/9/9/9/5OOOO/5OR1k/5O1p1/5O2P w - 1";
        let initial_position = PositionAux::from_sfen(sfen).unwrap();

        let run = |parallel: usize| -> (u16, Vec<super::Position>) {
            let mut search =
                super::BackwardSearch::new_with_parallel(&initial_position, false, parallel, false)
                    .unwrap();
            while search.advance().unwrap() {}
            let mut positions = search.positions().1.to_vec();
            positions.sort_by_key(|p| p.digest());
            (search.step(), positions)
        };

        let (step1, pos1) = run(1);
        let (step2, pos2) = run(2);
        let (step4, pos4) = run(4);
        let (step8, pos8) = run(8);

        assert_eq!(step1, step2);
        assert_eq!(step1, step4);
        assert_eq!(step1, step8);
        assert_eq!(pos1, pos2);
        assert_eq!(pos1, pos4);
        assert_eq!(pos1, pos8);
    }

    #[test]
    fn new_canonical_group_accepts_n_step_seed() {
        // Regression test: smoke の `--seed-sfen` + `--canonicalize-attacker-goldish`
        // で multi-step seed (0-step 詰みでない) を渡したとき、過去は
        // `new_canonical_group` が "Multi-seed only supports 0-step" で bail し
        // scheduler/search 側がそれを silently 握りつぶして即終了していた。
        // N-step seed でも構築でき step が解の長さに揃うことを保証する。
        //
        // 1-step seed: black to move, 7c の +P が 7b に動いて 7a の白玉を詰ます一手詰。
        let seed = PositionAux::from_sfen("2k6/9/2+P6/9/9/9/9/2L6/9 b 2r2b4g4s4n3l17p 1").unwrap();
        let search =
            super::BackwardSearch::new_canonical_group(std::slice::from_ref(&seed), 1).unwrap();
        assert_eq!(search.step(), 1, "step should equal solution length");
        let stats = search.stats();
        assert_eq!(stats.positions_len, 1, "frontier should contain the seed");
        // memo にはこの seed (canonical_digest) が group_step=1 で入っている。
        // prev_memo には move 後の mated 状態 (距離 0) が入っている。
        assert!(stats.memo_len >= 1);
        assert!(stats.prev_memo_len >= 1);

        // 0-step seed もこれまで通り受理されること (既存挙動の確認)。
        let mated_seed = PositionAux::from_sfen("9/9/9/9/9/6OOO/6O1k/6OO+P/8P w - 1").unwrap();
        let mated_search =
            super::BackwardSearch::new_canonical_group(std::slice::from_ref(&mated_seed), 1)
                .unwrap();
        assert_eq!(mated_search.step(), 0);
    }

    #[test]
    fn from_resume_state_canonical_group_preserves_progress() {
        // Run a canonical-group search a few steps, snapshot resume_state, then
        // reconstruct via `from_resume_state_canonical_group` and continue the
        // search. The reconstructed run should reach the same terminal step as
        // a fresh full run (memo loss only causes redundant work, not a
        // different terminus).
        let seed = PositionAux::from_sfen("9/9/9/9/9/5OOOO/5OR1k/5O1p1/5O2P w - 1").unwrap();
        let seeds = std::slice::from_ref(&seed);

        // Reference: fresh full run.
        let mut reference = super::BackwardSearch::new_canonical_group(seeds, 1).unwrap();
        while reference.advance().unwrap() {}
        let reference_step = reference.step();

        // Run a few steps, then snapshot.
        let mut staged = super::BackwardSearch::new_canonical_group(seeds, 1).unwrap();
        for _ in 0..2 {
            if !staged.advance().unwrap() {
                break;
            }
        }
        let snapshot_step = staged.step();
        let snapshot_frontier_len = staged.stats().positions_len;
        let resume_state = staged.resume_state();

        let mut resumed =
            super::BackwardSearch::from_resume_state_canonical_group(&resume_state, seeds, 1)
                .unwrap();
        assert_eq!(resumed.step(), snapshot_step);
        assert_eq!(resumed.stats().positions_len, snapshot_frontier_len);
        assert!(resumed.canonicalize_attacker_goldish);

        while resumed.advance().unwrap() {}
        assert_eq!(resumed.step(), reference_step);
    }

    #[test]
    fn from_resume_state_canonical_group_rejects_seed_mismatch() {
        let seed_a = PositionAux::from_sfen("9/9/9/9/9/5OOOO/5OR1k/5O1p1/5O2P w - 1").unwrap();
        let seed_b =
            PositionAux::from_sfen("2k6/9/2+P6/9/9/9/9/2L6/9 b 2r2b4g4s4n3l17p 1").unwrap();
        let search =
            super::BackwardSearch::new_canonical_group(std::slice::from_ref(&seed_a), 1).unwrap();
        let state = search.resume_state();
        // seed_b は state.initial_position_sfen と異なる → bail。
        assert!(super::BackwardSearch::from_resume_state_canonical_group(
            &state,
            std::slice::from_ref(&seed_b),
            1
        )
        .is_err());
    }

    #[test]
    fn test_backward_initial_variants() {
        let position = PositionAux::from_sfen("9/9/9/9/9/9/9/9/4k4 b - 1").unwrap();
        let variants = backward_initial_variants(&position);
        assert_eq!(variants.len(), 2);
        assert!(variants.iter().any(|p| !p.pawn_drop()));
        assert!(variants.iter().any(|p| p.pawn_drop()));

        let position = PositionAux::from_sfen("9/9/9/9/9/9/9/9/4k4 b - -1").unwrap();
        let variants = backward_initial_variants(&position);
        assert_eq!(variants.len(), 2);
        assert!(variants.iter().any(|p| !p.pawn_drop()));
        assert!(variants.iter().any(|p| p.pawn_drop()));
    }

    /// stone_digest cache の前提 — PositionAux::digest() は
    /// `core.digest() ^ stone_dependent` という構造で、stone-dependent 部分は
    /// stone (BitBoard) のみに依存し core (盤面/手駒) には依存しない。
    ///
    /// Phase 2 の最適化はこの不変量に依拠して、cache hit 時の PositionAux 構築を
    /// 省く: 任意の seed から `seed.digest() ^ seed.core().digest()` で
    /// stone_digest を一度だけ取り出しておけば、別 core でも
    /// `core.digest() ^ stone_digest` が `PositionAux::new(core, stone).digest()`
    /// と等しい。この test はその invariant を pin する。
    #[test]
    fn stone_digest_independent_of_core() {
        use crate::position::position::PositionAux;
        let seed = PositionAux::from_sfen("9/9/9/9/9/9/9/9/4k4 b - 1").unwrap();
        let stone_digest = seed.digest() ^ seed.core().digest();
        // 別の局面を同じ stone で構築。
        let other_sfens = [
            "9/9/9/9/9/9/9/9/G6k1 b - 1",
            "9/9/9/9/9/9/9/9/+P6k1 b - 1",
            "4k4/9/9/9/9/9/9/9/9 b 2r2b4g4s4n4l18p 1",
        ];
        for sfen in other_sfens {
            let other = PositionAux::from_sfen(sfen).unwrap();
            assert_eq!(
                *other.stone(),
                *seed.stone(),
                "test setup: stones must match for {sfen}"
            );
            let direct = other.digest();
            let via_cache = other.core().digest() ^ stone_digest;
            assert_eq!(
                direct, via_cache,
                "stone_digest invariant violated for {sfen}"
            );
        }
    }

    /// Regression test for the MADV_DONTNEED + write_bytes-removal optimization
    /// in `alloc_zeroed_slice`. Anonymous Linux mappings are zero-fill-on-demand,
    /// so a slice returned from `alloc_zeroed_slice` — whether freshly mmap'd or
    /// reused from the pool after a MADV_DONTNEED'd Drop — must read as zero.
    /// If MADV_DONTNEED is ever swapped back to MADV_FREE (or removed) without
    /// reinstating the `write_bytes` zeroing, the second alloc here may observe
    /// the leftover pattern and the assert fires.
    #[test]
    #[cfg(all(not(target_arch = "wasm32"), target_os = "linux"))]
    fn alloc_zeroed_slice_reuse_returns_zeroed_memory() {
        let len: usize = 1 << 18; // 256 Ki × u64 = 2 MiB (exactly one huge page)
        let pattern = 0xDEADBEEF_DEADBEEFu64;
        {
            let mut slice = super::alloc_zeroed_slice::<u64>(len);
            for x in slice.iter_mut() {
                *x = pattern;
            }
        } // Drop -> MADV_DONTNEED -> pool put
        let slice = super::alloc_zeroed_slice::<u64>(len);
        for (i, &x) in slice.iter().enumerate() {
            assert_eq!(x, 0, "u64 at index {i} not zero after pool reuse");
        }
    }

    /// Regression test for the Bottom-K + shard-routing bug.
    ///
    /// `shard_index` routes by the TOP SHARD_BITS of `digest`; if Bottom-K
    /// also keys on full `digest`, each shard's "smallest K" lies entirely
    /// inside its disjoint digest range, so the cross-shard merge produces
    /// only ~K candidates from shard 0 (= W/16 instead of W=`limit`).
    ///
    /// The fix: Bottom-K orders on `bottom_k_key(digest)` = lower SHARD_SHIFT
    /// bits, which is orthogonal to the shard router and uniformly
    /// distributed inside each shard, so merging the per-shard Bottom-K
    /// reproduces the global Bottom-K.
    #[test]
    fn build_candidates_returns_close_to_limit_under_uniform_digests() {
        use super::{bottom_k_key, shard_index, CandRef, ShardBucket, NUM_SHARDS};

        let n_total = 10_000usize;
        let limit = 1_000usize;
        let pool_factor = 4usize;
        let cap = (limit * pool_factor).div_ceil(NUM_SHARDS);

        let digests: Vec<u64> = (0..n_total)
            .map(|i| {
                // Splitmix64 — uniform-ish 64-bit hash from i.
                let mut x = (i as u64).wrapping_add(0x9E3779B97F4A7C15);
                x = (x ^ (x >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
                x = (x ^ (x >> 27)).wrapping_mul(0x94D049BB133111EB);
                x ^ (x >> 31)
            })
            .collect();

        let mut buckets: Vec<ShardBucket> =
            (0..NUM_SHARDS).map(|_| ShardBucket::new(cap)).collect();
        for &d in digests.iter() {
            buckets[shard_index(d)].try_insert(CandRef {
                digest: d,
                frontier_idx: 0,
                undo1_idx: 0,
                undo2_idx: 0,
            });
        }

        let (candidates, sampled) = super::build_candidates(buckets, Some(limit));
        eprintln!(
            "build_candidates: n_total={} limit={} pool_factor={} cap={} -> got {} (sampled={})",
            n_total,
            limit,
            pool_factor,
            cap,
            candidates.len(),
            sampled,
        );
        // Pre-fix bug yielded ~cap × NUM_SHARDS_used = up to limit*factor only
        // from shard 0's range, which translates to ~ cap candidates total
        // (since other shards' minima are all larger than shard 0's maximum).
        // With the bottom_k_key fix we expect exactly `limit` because total
        // uniques (10k) far exceed limit (1k).
        assert_eq!(candidates.len(), limit, "frontier should hit exactly W");
        assert!(sampled, "should report sampled=true since n_total > limit");

        // Sanity: ordering key (bottom_k_key) must be ascending.
        let keys: Vec<u64> = candidates.iter().map(|c| bottom_k_key(c.digest)).collect();
        assert!(
            keys.windows(2).all(|w| w[0] <= w[1]),
            "candidates not sorted by bottom_k_key"
        );
    }

    /// When `n_total <= limit`, no truncation occurs and the entire set is
    /// returned. The `sampled` flag MUST be false (otherwise checkpoints get
    /// suppressed unnecessarily).
    #[test]
    fn build_candidates_no_sampling_when_under_limit() {
        use super::{shard_index, CandRef, ShardBucket, NUM_SHARDS};

        let n_total = 200usize;
        let limit = 1_000usize;
        let pool_factor = 4usize;
        let cap = (limit * pool_factor).div_ceil(NUM_SHARDS);

        let digests: Vec<u64> = (0..n_total)
            .map(|i| {
                let mut x = (i as u64).wrapping_add(0x9E3779B97F4A7C15);
                x = (x ^ (x >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
                x = (x ^ (x >> 27)).wrapping_mul(0x94D049BB133111EB);
                x ^ (x >> 31)
            })
            .collect();

        let mut buckets: Vec<ShardBucket> =
            (0..NUM_SHARDS).map(|_| ShardBucket::new(cap)).collect();
        for &d in digests.iter() {
            buckets[shard_index(d)].try_insert(CandRef {
                digest: d,
                frontier_idx: 0,
                undo1_idx: 0,
                undo2_idx: 0,
            });
        }

        let (candidates, sampled) = super::build_candidates(buckets, Some(limit));
        assert_eq!(candidates.len(), n_total);
        assert!(!sampled, "no sampling should be reported when total ≤ limit");
    }
}
