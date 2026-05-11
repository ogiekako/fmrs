use std::{
    cell::UnsafeCell,
    ops::Range,
    ptr::NonNull,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Mutex,
    },
};

use anyhow::bail;
use log::{debug, info};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};

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

#[cfg(target_family = "unix")]
impl<T> Drop for MmapSlice<T> {
    fn drop(&mut self) {
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
    (
        alloc_zeroed_slice::<u64>(size),
        alloc_zeroed_slice::<u32>(size),
    )
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
    delta_trace: bool,
    canonicalize_attacker_goldish: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BackwardSearchStats {
    pub step: u16,
    pub seen_positions: usize,
    pub positions_len: usize,
    pub prev_positions_len: usize,
    pub memo_len: usize,
    pub prev_memo_len: usize,
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
            delta_trace: false,
            canonicalize_attacker_goldish: false,
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
            delta_trace: false,
            canonicalize_attacker_goldish: true,
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
            delta_trace: false,
            canonicalize_attacker_goldish: false,
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
            delta_trace: false,
            canonicalize_attacker_goldish: false,
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
            return self.advance_parallel_filtered(&|_, _| true, &|_, _| true);
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

    /// Update the memo entry limit without pre-allocating capacity. Use when
    /// the limit may change frequently (e.g., dynamic per-seed budget that
    /// grows as other seeds finish): pre_allocate's cost is +44.5% on the
    /// parallel path because the memos get reset each step anyway, so the
    /// up-front allocation is wasted. The lazy-grow Memo handles capacity
    /// adjustment on demand.
    pub fn set_memo_entry_limit_lazy(&mut self, max_entries: Option<usize>) {
        self.memo_entry_limit = max_entries.map(|limit| (limit / 2).max(1));
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
        self.advance_upto_with_filter(upto, |_, _| true)
    }

    pub fn advance_upto_with_filter(
        &mut self,
        upto: usize,
        mut filter: impl FnMut(&Position, Option<BitBoard>) -> bool,
    ) -> anyhow::Result<bool> {
        self.advance_upto_with_candidate_filter(
            upto,
            |_, _| true,
            |position, stone| filter(position, stone),
        )
    }

    pub fn advance_upto_with_candidate_filter(
        &mut self,
        upto: usize,
        mut candidate_filter: impl FnMut(&PositionAux, &UndoMove) -> bool,
        mut filter: impl FnMut(&Position, Option<BitBoard>) -> bool,
    ) -> anyhow::Result<bool> {
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

                if !filter(pp.core(), self.stone) {
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

    pub fn advance_parallel_filtered(
        &mut self,
        candidate_filter: &(impl Fn(&PositionAux, &UndoMove) -> bool + Sync),
        filter: &(impl Fn(&Position, Option<BitBoard>) -> bool + Sync),
    ) -> anyhow::Result<bool> {
        if self.positions.is_empty() {
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
        // Peak memory is now bounded by (per-thread chunk output) + (final
        // unique candidates), not by total raw candidates.
        let positions = &self.positions;
        let dedup_count = AtomicUsize::new(0);
        let shard_buckets: Vec<Mutex<(NoHashSet64, Vec<Position>)>> = (0..NUM_SHARDS)
            .map(|_| Mutex::new((NoHashSet64::default(), Vec::new())))
            .collect();

        self.install_or_run(|| {
            positions
                .par_chunks(position_chunk_size)
                .enumerate()
                .for_each(|(chunk_idx, chunk)| {
                    let mut undo_moves = vec![];
                    let mut local_seens: [NoHashSet64; NUM_SHARDS] =
                        std::array::from_fn(|_| NoHashSet64::default());
                    let mut local_outs: [Vec<Position>; NUM_SHARDS] =
                        std::array::from_fn(|_| Vec::new());
                    let mut chunk_dedup = 0usize;

                    for core in chunk.iter() {
                        let mut position = PositionAux::new(core.clone(), stone);
                        undo_moves.clear();
                        previous(&mut position, step > 0, &mut undo_moves);

                        for m in undo_moves.iter() {
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
                            if !filter(pp.core(), stone) {
                                continue;
                            }
                            let digest = pp.core().digest();
                            let shard_idx = shard_index(digest);
                            if local_seens[shard_idx].insert(digest) {
                                local_outs[shard_idx].push(pp.core().clone());
                                chunk_dedup += 1;
                            }
                        }
                    }
                    dedup_count.fetch_add(chunk_dedup, Ordering::Relaxed);

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
                        let (seen, out) = &mut *guard;
                        out.reserve(local.len());
                        for pos in local {
                            if seen.insert(pos.digest()) {
                                out.push(pos);
                            }
                        }
                    }
                });
        });

        let candidate_len = dedup_count.into_inner();

        let total_unique: usize = shard_buckets
            .iter()
            .map(|m| m.lock().unwrap().1.len())
            .sum();
        let mut candidates = Vec::with_capacity(total_unique);
        for bucket in shard_buckets {
            let (_, out) = bucket.into_inner().unwrap();
            candidates.extend(out);
        }

        if candidates.is_empty() {
            return Ok(false);
        }

        // Phase 2: verify uniqueness in parallel
        let parallel = self.parallel.min(candidates.len());
        // chunk_size = candidates / (parallel*64) で 1 thread あたり ~64 chunks。
        // chunks のコストが大きく不均一 (deep memo searches vs cheap lookups) なので
        // 細かめに分割すると work-stealing が効いて並列効率が改善する。
        // この workload では `*8` (default rayon-ish) → `*32` で wall ~6% 改善。
        let chunk_size = candidates.len().div_ceil(parallel * 64).max(1);
        // Cross-step memo retention policy:
        //  - step < 10: discard. Fresh demand-zero mmap pages beat carrying stale
        //    entries that bloat the table for little benefit in short searches.
        //    (bench_backward_search_seed_sfen at max-step 11 regressed 18% with
        //    unconditional retention.)
        //  - step >= 10: carry forward via std::mem::take. At deep steps the DFS
        //    per candidate is expensive enough that cross-step cache hits pay off;
        //    bench_backward_search_seed_sfen_allowed_kinds at max-step 19 improved
        //    3.3% with retention.  Threshold lowered from 15 to 10 since memo
        //    reuse becomes valuable a few steps earlier than originally tuned.
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
        if step >= 10 {
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
        for wave in candidates.chunks(wave_size) {
            let memo_ref: &Memo = &memo;
            let prev_memo_ref: &Memo = &prev_memo;

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

                        for core in chunk.iter() {
                            let mut pp = PositionAux::new(core.clone(), stone);
                            // smoke 用 canonicalize: hit を期待して digest 先取得、
                            // miss 時のみ実 mutation して solutions に渡す。
                            let pp_digest = if canonicalize {
                                crate::search::canonicalize::canonical_digest_for_smoke(&pp)
                            } else {
                                pp.digest()
                            };
                            if let Some(ans) =
                                get_overlay(&prev_memo_delta, prev_memo_ref, pp_digest)
                                    .filter(|ans| !ans.needs_investigation(step + 1))
                            {
                                if ans.is_uniquely(step + 1) {
                                    prev_positions.push(core.clone());
                                }
                                continue;
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
                            if ans.is_uniquely(step + 1) {
                                prev_positions.push(core.clone());
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

        let all_positions = all_positions;

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
            return Ok(false);
        }

        self.positions = all_positions;
        self.prev_positions = Vec::new();
        std::mem::swap(&mut self.memo, &mut self.prev_memo);
        self.seen_positions = 0;
        self.step += 1;

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
}
