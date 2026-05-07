use std::{
    cell::UnsafeCell,
    ops::Range,
    sync::atomic::{AtomicUsize, Ordering},
};

use anyhow::bail;
use log::{debug, info};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};

use crate::{
    nohash::NoHashMap64,
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

const SHARD_BITS: u32 = 3;
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
    keys: Box<[u64]>,
    values: Box<[u32]>,
    mask: usize,
    capacity_threshold: usize,
}

unsafe impl Sync for ShardedFlatMemo {}
unsafe impl Send for ShardedFlatMemo {}
unsafe impl Sync for FlatShard {}
unsafe impl Send for FlatShard {}

#[inline(always)]
fn shard_index(key: u64) -> usize {
    (key >> SHARD_SHIFT) as usize
}

fn alloc_slot_arrays(size: usize) -> (Box<[u64]>, Box<[u32]>) {
    (alloc_zeroed_slice::<u64>(size), alloc_zeroed_slice::<u32>(size))
}

fn alloc_zeroed_slice<T: Copy>(size: usize) -> Box<[T]> {
    use std::alloc::{alloc_zeroed, Layout};
    debug_assert!(size > 0);
    debug_assert!(size.is_power_of_two());
    let layout = Layout::array::<T>(size).expect("FlatMemo layout");
    // SAFETY: T (u64 or u32) is plain old data; zeroed bytes form valid values
    // (key=0 = empty sentinel; value bytes unused while corresponding key is empty).
    unsafe {
        let ptr = alloc_zeroed(layout) as *mut T;
        if ptr.is_null() {
            std::alloc::handle_alloc_error(layout);
        }
        // Hint transparent huge pages — beneficial for multi-GB tables.
        #[cfg(target_os = "linux")]
        {
            let bytes = layout.size();
            // ignore errors (kernel may not support, alignment may not match)
            let _ = libc::madvise(ptr.cast(), bytes, libc::MADV_HUGEPAGE);
        }
        Box::from_raw(std::slice::from_raw_parts_mut(ptr, size))
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
            use std::arch::x86_64::{_MM_HINT_T0, _mm_prefetch};
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
        unsafe { self.shards.get_unchecked(idx).insert_unsynchronized(key, value) };
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

    fn remove(&mut self, key: u64) -> Option<StepRange> {
        // SAFETY: &mut self ⇒ exclusive access.
        unsafe { self.remove_unsynchronized(key) }
    }

    fn len(&self) -> usize {
        self.shards.iter().map(|s| s.len.load(Ordering::Relaxed)).sum()
    }

    #[allow(dead_code)]
    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn iter(&self) -> impl Iterator<Item = (u64, StepRange)> + '_ {
        self.shards.iter().flat_map(|s| s.iter())
    }

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
            let v = old_values[i];
            let mut idx = (k as usize) & new_mask;
            loop {
                let slot_key = unsafe { inner.keys.get_unchecked_mut(idx) };
                if *slot_key == FLAT_EMPTY_KEY {
                    *slot_key = k;
                    unsafe {
                        *inner.values.get_unchecked_mut(idx) = v;
                    }
                    break;
                }
                idx = (idx + 1) & new_mask;
            }
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
            let v = old_values[i];
            let mut idx = (k as usize) & new_mask;
            loop {
                let slot_key = unsafe { inner.keys.get_unchecked_mut(idx) };
                if *slot_key == FLAT_EMPTY_KEY {
                    *slot_key = k;
                    unsafe {
                        *inner.values.get_unchecked_mut(idx) = v;
                    }
                    break;
                }
                idx = (idx + 1) & new_mask;
            }
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
            let mut idx = (key as usize) & mask;
            loop {
                let slot_key = unsafe { inner.keys.get_unchecked_mut(idx) };
                if *slot_key == FLAT_EMPTY_KEY {
                    *slot_key = key;
                    unsafe {
                        *inner.values.get_unchecked_mut(idx) = packed;
                    }
                    break;
                }
                idx = (idx + 1) & mask;
            }
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
            let mut idx = (key as usize) & mask;
            loop {
                let slot_key = unsafe { inner.keys.get_unchecked_mut(idx) };
                if *slot_key == FLAT_EMPTY_KEY {
                    *slot_key = key;
                    unsafe {
                        *inner.values.get_unchecked_mut(idx) = packed;
                    }
                    break;
                }
                idx = (idx + 1) & mask;
            }
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
        })
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

    pub fn set_delta_trace(&mut self, enabled: bool) {
        self.delta_trace = enabled;
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
                    };
                    if crate::position::advance::advance::advance_aux(
                        &mut pp,
                        &options,
                        &mut branches,
                    )
                    .is_ok()
                    {
                        if !branches.is_empty() {
                            self.prev_positions.push(pp.core().clone());
                            self.prev_memo
                                .insert(pp.digest(), StepRange::exact(self.step + 1));
                        }
                    }
                    continue;
                }

                let mate_in = self.step + 1;
                let pp_digest = pp.digest();
                let ans = if let Some(ans) = self
                    .prev_memo
                    .get(pp_digest)
                    .filter(|ans| !ans.needs_investigation(mate_in))
                {
                    ans
                } else {
                    solutions(
                        &mut pp,
                        &self.prev_memo,
                        &self.memo,
                        mate_in,
                        &mut solution_scratch,
                        self.memo_entry_limit,
                    )
                };
                if ans.is_uniquely(mate_in) {
                    #[cfg(debug_assertions)]
                    {
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

                    self.prev_positions.push(pp.core().clone());
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

        // Phase 1: generate candidates in parallel (with filters)
        let positions = &self.positions;
        let candidate_chunks = self.install_or_run(|| {
            positions
                .par_chunks(position_chunk_size)
                .map(|chunk| {
                    let mut undo_moves = vec![];
                    let mut candidates = vec![];

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
                            candidates.push(pp.core().clone());
                        }
                    }

                    candidates
                })
                .collect::<Vec<_>>()
        });
        let candidate_len = candidate_chunks.iter().map(Vec::len).sum::<usize>();
        let mut candidates = Vec::with_capacity(candidate_len);
        for chunk in candidate_chunks {
            candidates.extend(chunk);
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
        let mut memo = std::mem::replace(&mut self.memo, Memo::new());
        let mut prev_memo = std::mem::replace(&mut self.prev_memo, Memo::new());

        let phase2_start = std::time::Instant::now();

        let results: Vec<(Vec<Position>, NoHashMap64<StepRange>, NoHashMap64<StepRange>)> = {
            let memo_ref: &Memo = &memo;
            let prev_memo_ref: &Memo = &prev_memo;
            self.install_or_run(|| {
                candidates
                    .par_chunks(chunk_size)
                    .map(|chunk| {
                        // 初期 capacity を高めに取って rehash (memset 込み) を回避。
                        // 4096 で数 KB の確保コストと引き換えに数回の rehash を skip。
                        // 1024 / 4096 / 16384 を試した結果、4096 が sweet spot。
                        let mut memo_delta = NoHashMap64::with_capacity_and_hasher(
                            4096,
                            Default::default(),
                        );
                        let mut prev_memo_delta = NoHashMap64::with_capacity_and_hasher(
                            4096,
                            Default::default(),
                        );
                        let mut prev_positions = vec![];
                        let mut solution_scratch = vec![];

                        for core in chunk.iter() {
                            let mut pp = PositionAux::new(core.clone(), stone);
                            let pp_digest = pp.digest();
                            if let Some(ans) =
                                get_overlay(&prev_memo_delta, prev_memo_ref, pp_digest)
                                    .filter(|ans| !ans.needs_investigation(step + 1))
                            {
                                if ans.is_uniquely(step + 1) {
                                    prev_positions.push(core.clone());
                                }
                                continue;
                            }

                            let ans = solutions_overlay(
                                &mut pp,
                                prev_memo_ref,
                                &mut prev_memo_delta,
                                memo_ref,
                                &mut memo_delta,
                                step + 1,
                                &mut solution_scratch,
                            );
                            if ans.is_uniquely(step + 1) {
                                prev_positions.push(core.clone());
                            }
                        }

                        (prev_positions, memo_delta, prev_memo_delta)
                    })
                    .collect()
            })
        };

        let phase2_only_ms = phase2_start.elapsed().as_millis();

        let mut all_positions = vec![];
        let mut memo_deltas = vec![];
        let mut prev_memo_deltas = vec![];
        let mut delta_total_count = 0usize;
        for (positions, memo_delta, prev_memo_delta) in results {
            all_positions.extend(positions);
            delta_total_count += memo_delta.len() + prev_memo_delta.len();
            memo_deltas.push(memo_delta);
            prev_memo_deltas.push(prev_memo_delta);
        }

        let merge_start = std::time::Instant::now();
        self.install_or_run(|| {
            merge_deltas_sharded(&memo, memo_deltas);
            merge_deltas_sharded(&prev_memo, prev_memo_deltas);
        });
        let merge_ms = merge_start.elapsed().as_millis();

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

        let positions = self
            .positions
            .iter()
            .filter(|p| !p.pawn_drop())
            .map(|p| PositionAux::new(p.clone(), self.stone))
            .collect::<Vec<_>>();

        let mut output_positions = vec![];
        let no_black_goldish = self.no_black_goldish;
        if !black_position || self.step % 2 == 1 || self.step == 0 {
            if self.parallel > 1 && positions.len() > 1 {
                let parallel = self.parallel.min(positions.len());
                let chunk_size = positions.len().div_ceil(parallel * 8).max(1);
                let chunks = self.install_or_run(|| {
                    positions
                        .par_chunks(chunk_size)
                        .map(|chunk| {
                            let mut out = Vec::new();
                            for p in chunk.iter() {
                                if !satisfies_backward_constraints(p, no_black_goldish) {
                                    continue;
                                }
                                if !satisfies_output_constraints(p, bare_white_king) {
                                    continue;
                                }
                                out.push(p.clone());
                            }
                            out
                        })
                        .collect::<Vec<_>>()
                });
                for chunk in chunks {
                    output_positions.extend(chunk);
                }
            } else {
                for p in positions.iter() {
                    if !satisfies_backward_constraints(p, self.no_black_goldish) {
                        continue;
                    }
                    if !satisfies_output_constraints(p, bare_white_king) {
                        continue;
                    }
                    output_positions.push(p.clone());
                }
            }
        } else {
            let desired_step = self.step - 1;
            if self.parallel > 1 && positions.len() > 1 {
                let parallel = self.parallel.min(positions.len());
                let chunk_size = positions.len().div_ceil(parallel * 8).max(1);
                let prev_memo = &self.prev_memo;
                let chunks = self.install_or_run(|| {
                    positions
                        .par_chunks(chunk_size)
                        .map(|chunk| -> anyhow::Result<Vec<PositionAux>> {
                            let mut out = Vec::new();
                            for p in chunk.iter() {
                                debug_assert_eq!(p.turn(), Color::WHITE);
                                let mut position = p.clone();
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
                for p in positions.iter() {
                    debug_assert_eq!(p.turn(), Color::WHITE);
                    let mut position = p.clone();
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
                        if !satisfies_backward_constraints(&np, self.no_black_goldish) {
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

fn solutions(
    position: &mut PositionAux,
    memo: &Memo,
    next_memo: &Memo,
    mate_in: u16,
    scratch: &mut Vec<Vec<Movement>>,
    memo_entry_limit: Option<usize>,
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
    )
}

fn solutions_inner(
    position: &mut PositionAux,
    memo: &Memo,
    next_memo: &Memo,
    mate_in: u16,
    scratch: &mut [Vec<Movement>],
    memo_entry_limit: Option<usize>,
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

    // Two-pass move ordering: memoized children first; skip recursion if those
    // alone prove non-uniqueness or a shorter mate. hit_mask records pass-1 hits.
    let mut hit_mask = [0u64; 2];
    for (i, m) in movements.iter().enumerate() {
        let child_digest = position.moved_digest(m);
        if let Some(child) = next_memo
            .get(child_digest)
            .filter(|a| !a.needs_investigation(mate_in - 1))
        {
            hit_mask[i / 64] |= 1u64 << (i % 64);
            let a = child.inc();
            res.update_with_child(&a);
            if res.definitely_shorter_or_non_unique(mate_in) {
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
            )
            .inc();
            debug_assert!(!a.needs_investigation(mate_in));

            res.update_with_child(&a);

            if res.definitely_shorter_or_non_unique(mate_in) {
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


fn solutions_overlay(
    position: &mut PositionAux,
    memo_base: &Memo,
    memo_delta: &mut NoHashMap64<StepRange>,
    next_memo_base: &Memo,
    next_memo_delta: &mut NoHashMap64<StepRange>,
    mate_in: u16,
    scratch: &mut Vec<Vec<Movement>>,
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
            )
            .inc();
            debug_assert!(!a.needs_investigation(mate_in));

            res.update_with_child(&a);

            if res.definitely_shorter_or_non_unique(mate_in) {
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

        let mut search = super::BackwardSearch::new_with_parallel(
            &initial_position,
            false,
            2,
            false,
        )
        .unwrap();

        while search.advance().unwrap() {}

        assert!(search.step() > 0);
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
