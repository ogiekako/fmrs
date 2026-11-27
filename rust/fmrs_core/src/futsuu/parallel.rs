//! Multi-threaded cooperative df-pn (`bn_search`) engine with lock-free optimistic seqlock `SharedTBase`,
//! L1-resident per-thread `LocalPathTable` cycle detection, lock-free `InFlightTable`
//! virtual proof/disproof diversification, and ancestor-solved unwinding.

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, AtomicU16, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};

use crate::futsuu::time::Instant;

use crate::futsuu::backend::{HsOracle, PositionBackend};
use crate::futsuu::cycle_skip::{
    rollout_corridor, shift_move, shift_sq, CertEdge, CertOrNode, CovariantCert, CycleDetection,
    CycleStats, CycleTracker, SharedCorridorTable, MAX_CERT_NODES,
};
use crate::futsuu::ni_cache::{
    alloc_zeroed_slice, mkey_bound_dec_if_bounded, mkey_bound_inc, mkey_min_dec_sat,
    proof_tbase_mkey, BoardPlanes, LocalMetaTable, NiFootprint, NiHit, SharedNiCache,
    MIN_NI_INSERT_SH, MIN_NI_INSERT_WORK,
};
use crate::futsuu::proof::{
    and_sort_key, disproof_koma, disproof_koma_and, disproof_koma_or, or_sort_key, proof_koma,
    proof_koma_and, proof_koma_and_simple, proof_koma_or, proof_koma_or_simple, reorder_first_and,
    reorder_first_and_simple, reorder_first_or, reorder_first_or_simple, sort_and, sort_and_simple,
    sort_or, sort_or_simple,
};
use crate::futsuu::shared_tbase::{LocalTerminalEntry, SharedTBase, LOCAL_TT_SIZE};
use crate::futsuu::solver::{
    analyze_tsume_pv, compute_spare_mask, copy_proof_fields, initial_or_pn0, sort_and_simple_ab,
    DefaultHeuristic, SearchHeuristic, SimpleDag, SimpleDagEntry, NO_CU,
};
use crate::futsuu::types::{
    sq_file, sq_rank, KeyState, MKey, Move, MoveChain, MvItem, SearchConfig, SearchResult, TData,
    Turn, DISTANCE, INFINATE, IS_SKOMA, MAKE_TREE_PN_PLUS, PRE_DISPROOF_MAX, PRE_PROOF_MAX,
    PROMOTED, PROOF_MAX, TSUME_MAX_DEPTH,
};

const IF_SLOTS: usize = 65536;
const IF_MASK: usize = IF_SLOTS - 1;
const IF_STRIDE_SHIFT: usize = 2;
const PATH_BUCKETS: usize = 2048;
const PATH_MASK: usize = PATH_BUCKETS - 1;
const SIMPLE_MAX_DEPTH: u16 = 8000;
const ADD_SEARCH_SH: u16 = 30;
const WARMUP_NODES: u64 = 2048;
const PROBE_WARMUP_NODES: u64 = 2048;
const DEEP_CORRIDOR_DEPTH: u16 = 40;

#[inline(always)]
fn reorder_at_key(list: &mut [MvItem], idx: usize, key: u64) {
    list[idx].sort_key = key;
    if idx > 0 && key < list[idx - 1].sort_key {
        let mut dst = idx - 1;
        while dst > 0 && key < list[dst - 1].sort_key {
            dst -= 1;
        }
        unsafe {
            let item = std::ptr::read(list.as_ptr().add(idx));
            std::ptr::copy(list.as_ptr().add(dst), list.as_mut_ptr().add(dst + 1), idx - dst);
            std::ptr::write(list.as_mut_ptr().add(dst), item);
        }
    } else if idx + 1 < list.len() && key > list[idx + 1].sort_key {
        let len = list.len();
        let mut dst = idx + 1;
        while dst + 1 < len && key > list[dst + 1].sort_key {
            dst += 1;
        }
        unsafe {
            let item = std::ptr::read(list.as_ptr().add(idx));
            std::ptr::copy(list.as_ptr().add(idx + 1), list.as_mut_ptr().add(idx), dst - idx);
            std::ptr::write(list.as_mut_ptr().add(dst), item);
        }
    }
}

#[inline]
fn reorder_at_or(list: &mut [MvItem], idx: usize) {
    if list.len() <= 1 {
        return;
    }
    let key = or_sort_key(&list[idx]);
    reorder_at_key(list, idx, key);
}

#[inline]
fn reorder_at_and(list: &mut [MvItem], idx: usize) {
    if list.len() <= 1 {
        return;
    }
    let key = and_sort_key(&list[idx]);
    reorder_at_key(list, idx, key);
}

/// Lock-free virtual in-flight counter table (`65536` logical slots strided 4x across `1 MB`
/// via `alloc_zeroed`, cutting cross-CCD cache-line false sharing by 4x while keeping O(1) 1-load reads).
struct InFlightTable {
    counts: Box<[AtomicU32]>,
}

impl InFlightTable {
    fn new() -> Self {
        // SAFETY: AtomicU32 is #[repr(transparent)] over u32; all-zero is 0u32.
        Self {
            counts: unsafe { alloc_zeroed_slice::<AtomicU32>(IF_SLOTS << IF_STRIDE_SHIFT) },
        }
    }

    #[cfg(debug_assertions)]
    fn assert_empty(&self) {
        for c in self.counts.iter() {
            debug_assert_eq!(c.load(Ordering::Relaxed), 0);
        }
    }

    #[inline(always)]
    fn slot_idx(zkey: u64, mkey: MKey) -> usize {
        let h = zkey ^ mkey.0.wrapping_mul(0x9e37_79b9_7f4a_7c15).rotate_left(21);
        ((h ^ (h >> 19)) as usize) & IF_MASK
    }

    #[inline(always)]
    fn get_slot(&self, sidx: usize) -> u32 {
        unsafe {
            self.counts
                .get_unchecked(sidx << IF_STRIDE_SHIFT)
                .load(Ordering::Relaxed)
        }
    }

    #[inline(always)]
    fn enter_slot(&self, sidx: usize, active: bool) -> InFlightGuard<'_> {
        if active {
            let slot = unsafe { self.counts.get_unchecked(sidx << IF_STRIDE_SHIFT) };
            slot.fetch_add(1, Ordering::Relaxed);
            InFlightGuard { slot: Some(slot) }
        } else {
            InFlightGuard { slot: None }
        }
    }
}

struct InFlightGuard<'a> {
    slot: Option<&'a AtomicU32>,
}

impl<'a> Drop for InFlightGuard<'a> {
    #[inline(always)]
    fn drop(&mut self) {
        if let Some(s) = self.slot {
            s.fetch_sub(1, Ordering::Relaxed);
        }
    }
}

#[derive(Copy, Clone)]
struct PathEntry {
    ks: KeyState,
    tn: Turn,
    cpn: u16,
    prev: u16,
    if_slot: u32,
}

/// Thread-local L1d-resident LIFO ancestor stack for cycle & hand-loss repetition detection,
/// O(1) self-in-flight subtraction (`slot_counts`), and O(1)-amortized solved-ancestor checking.
struct LocalPathTable {
    buckets: Box<[u16; PATH_BUCKETS]>,
    slot_counts: Box<[u8]>,
    entries: Vec<PathEntry>,
    anc_cursor: usize,
}

impl LocalPathTable {
    fn new() -> Self {
        Self {
            buckets: Box::new([u16::MAX; PATH_BUCKETS]),
            slot_counts: unsafe { alloc_zeroed_slice::<u8>(IF_SLOTS) },
            entries: Vec::with_capacity(1024),
            anc_cursor: 17,
        }
    }

    #[inline(always)]
    fn len(&self) -> usize {
        self.entries.len()
    }

    #[inline(always)]
    fn push(&mut self, ks: KeyState, tn: Turn, cpn: u16, if_slot: u32) {
        let b = (ks.zkey as usize) & PATH_MASK;
        let prev = self.buckets[b];
        let idx = self.entries.len() as u16;
        if if_slot != u32::MAX {
            let sc = unsafe { self.slot_counts.get_unchecked_mut(if_slot as usize) };
            *sc = sc.saturating_add(1);
        }
        self.entries.push(PathEntry {
            ks,
            tn,
            cpn,
            prev,
            if_slot,
        });
        self.buckets[b] = idx;
    }

    #[inline(always)]
    fn pop(&mut self) {
        if let Some(e) = self.entries.pop() {
            self.buckets[(e.ks.zkey as usize) & PATH_MASK] = e.prev;
            if e.if_slot != u32::MAX {
                let sc = unsafe { self.slot_counts.get_unchecked_mut(e.if_slot as usize) };
                *sc = sc.saturating_sub(1);
            }
        }
    }

    #[inline(always)]
    fn has_exact(&self, zkey: u64, mkey: MKey) -> bool {
        let mut idx = self.buckets[(zkey as usize) & PATH_MASK];
        while idx != u16::MAX {
            let e = unsafe { *self.entries.get_unchecked(idx as usize) };
            if e.ks.zkey == zkey && e.ks.mkey_for(e.tn).0 == mkey.0 {
                return true;
            }
            idx = e.prev;
        }
        false
    }

    #[inline(always)]
    fn self_slot_count(&self, sidx: usize) -> u32 {
        unsafe { *self.slot_counts.get_unchecked(sidx) as u32 }
    }

    /// Apply exact repetition (`MC_CURRENT`) and superior-hand repetition (`mcard.mkey.ge_all(mkey)`)
    /// bounds in L1 cache with zero shared-memory writes.
    #[inline(always)]
    fn apply_cycle_bounds(&self, cks: &KeyState, mv: &mut MvItem, tn: Turn) {
        if mv.tdata.pn == 0 || mv.tdata.dn == 0 {
            return;
        }
        let mut idx = self.buckets[(cks.zkey as usize) & PATH_MASK];
        if idx == u16::MAX {
            return;
        }
        let mkey = cks.mkey_for(tn);
        let mut exact_cpn: u16 = 0;
        let mut dom_cpn: u16 = 0;
        while idx != u16::MAX {
            let e = unsafe { *self.entries.get_unchecked(idx as usize) };
            if e.ks.zkey == cks.zkey {
                let anc_mk = e.ks.mkey_for(tn);
                if anc_mk.0 == mkey.0 {
                    if e.cpn.max(1) > exact_cpn {
                        exact_cpn = e.cpn.max(1);
                    }
                } else if anc_mk.ge_all(mkey) && e.cpn.max(1) > dom_cpn {
                    dom_cpn = e.cpn.max(1);
                }
            }
            idx = e.prev;
        }
        if exact_cpn > 0 {
            mv.tdata = TData {
                pn: exact_cpn.max(dom_cpn),
                dn: 1,
                sh: 0,
            };
            mv.set_search(true);
        } else if dom_cpn > mv.tdata.pn {
            mv.tdata.pn = dom_cpn;
        }
    }

    /// Find the shallowest ancestor on our stack (above Root `idx > 0`) that another thread
    /// has already proven or disproven in `SharedTBase`, using the `32 KB` L1 `solved_bits`
    /// filter plus bounded rolling cursor for deep spines (`Microcosmos` 1525 plies).
    #[inline]
    fn find_solved_ancestor(&mut self, tbase: &SharedTBase) -> Option<usize> {
        let len = self.entries.len();
        if len <= 1 {
            return None;
        }
        let head_end = len.min(17);
        for i in 1..head_end {
            let e = unsafe { *self.entries.get_unchecked(i) };
            if tbase.is_terminal(&e.ks, e.tn) {
                return Some(i);
            }
        }
        if len > 17 {
            let tail_len = len - 17;
            let step_cnt = tail_len.min(48);
            let mut cur = if self.anc_cursor >= 17 && self.anc_cursor < len {
                self.anc_cursor
            } else {
                17
            };
            for _ in 0..step_cnt {
                let e = unsafe { *self.entries.get_unchecked(cur) };
                if tbase.is_terminal(&e.ks, e.tn) {
                    self.anc_cursor = cur;
                    return Some(cur);
                }
                cur += 1;
                if cur >= len {
                    cur = 17;
                }
            }
            self.anc_cursor = cur;
        }
        None
    }
}

struct WorkerOracle<'a> {
    tbase: &'a SharedTBase,
    local_tt: Option<&'a mut [LocalTerminalEntry]>,
    hs_depth: u8,
}

impl<'a> HsOracle for WorkerOracle<'a> {
    fn hs_lookup<P: PositionBackend>(&mut self, pos: &P, tn: Turn) -> bool {
        if pos.n_oute() == 0 {
            return false;
        }
        let ks = pos.key_state();
        let res = if let Some(ltt) = self.local_tt.as_deref_mut() {
            self.tbase.hs_lookup_cached(&ks, tn, ltt)
        } else {
            self.tbase.hs_lookup_raw(&ks, tn)
        };
        if res >= 0 {
            return res != 0;
        }
        if !self.tbase.any_gc() || self.hs_depth >= 2 {
            return false;
        }
        self.hs_depth += 1;
        let mut list = Vec::with_capacity(16);
        let _ = pos.generate_evasion_with(self, &mut list);
        if list.is_empty() {
            self.hs_depth -= 1;
            return true;
        }
        let mut i = 0;
        if list.len() > 1 {
            self.tbase.prefetch(pos.key_forward_item(&list[0]).zkey);
        }
        while i < list.len() {
            if i + 1 < list.len() {
                self.tbase.prefetch(pos.key_forward_item(&list[i + 1]).zkey);
            }
            let child_ks = pos.key_forward_item(&list[i]);
            self.tbase.make_tree_lookup(&child_ks, &mut list[i], tn);
            if let Some(tail) = list[i].take_tail() {
                list.insert(i + 1, split_tail_item(pos, tail));
            }
            i += 1;
        }
        self.hs_depth -= 1;
        sort_and(&mut list);
        list[0].tdata.pn == 0
    }
}

#[inline]
fn proof_number(list: &[MvItem]) -> (u16, u16) {
    let mut wpn = list[0].tdata.pn;
    if wpn == INFINATE && list[0].tdata.dn == 0 {
        return (INFINATE, 0);
    }
    let mut pn: u16 = 0;
    for item in &list[1..] {
        if item.tdata.pn == 0 {
            break;
        }
        pn = pn.saturating_add(1);
        wpn = wpn.max(item.tdata.pn);
    }
    ((INFINATE - 1).min(wpn.saturating_add(pn)), pn)
}

#[inline]
fn disproof_number(list: &[MvItem]) -> (u16, u16) {
    let mut wdn = list[0].tdata.dn;
    if wdn == INFINATE && list[0].tdata.pn == 0 {
        return (INFINATE, 0);
    }
    let mut dn: u16 = 0;
    for item in &list[1..] {
        if item.tdata.dn == 0 {
            break;
        }
        dn = dn.saturating_add(1);
        wdn = wdn.max(item.tdata.dn);
    }
    ((INFINATE - 1).min(wdn.saturating_add(dn)), dn)
}

#[inline]
fn split_tail_item<P: PositionBackend>(pos: &P, tail: MoveChain) -> MvItem {
    let hm = tail[0];
    let src_koma = if hm.is_board_move() {
        pos.board_at(hm.prev_pos as usize)
    } else {
        0
    };
    let cap_koma = pos.board_at(hm.dest());
    MvItem::from_moves_with_hints(tail, src_koma, cap_koma)
}

fn tsumi_proof<P: PositionBackend>(pos: &P, mvlist: &mut MvItem, pflag: bool) {
    mvlist.tdata = TData::TSUMI;
    let enemy_mkey = pos.mkey(pos.turn().flip());
    mvlist.mkey = if pflag { enemy_mkey } else { proof_koma(pos) };
    if !enemy_mkey.is_empty() {
        mvlist.set_inc(true);
        mvlist.nouse = mvlist.mkey.total();
        mvlist.nouse2 = enemy_mkey.total();
    } else {
        mvlist.set_inc(false);
    }
    mvlist.set_hinc(!mvlist.mkey.is_empty());
}

#[repr(align(64))]
struct CachePaddedAtomicU64(AtomicU64);

impl std::ops::Deref for CachePaddedAtomicU64 {
    type Target = AtomicU64;
    #[inline(always)]
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[repr(C, align(64))]
struct SharedRunState<'a> {
    config: &'a SearchConfig,
    tbase: &'a SharedTBase,
    ni_cache: &'a SharedNiCache,
    inflight: &'a InFlightTable,
    corridor: &'a SharedCorridorTable,
    num_workers: usize,
    start: Instant,
    symmetry: bool,
    abort_bn: AtomicBool,
    suspended: AtomicBool,
    global_nodes: CachePaddedAtomicU64,
    max_thpn: AtomicU16,
    winner_root: Mutex<Option<MvItem>>,
    scout_stats: Mutex<Option<CycleStats>>,
    external_abort: Option<Arc<AtomicBool>>,
    progress_cb: Option<Arc<dyn Fn(u64) + Send + Sync>>,
    last_progress_nodes: AtomicU64,
}

impl<'a> SharedRunState<'a> {
    #[inline]
    #[allow(clippy::too_many_arguments)]
    fn new(
        config: &'a SearchConfig,
        tbase: &'a SharedTBase,
        ni_cache: &'a SharedNiCache,
        inflight: &'a InFlightTable,
        corridor: &'a SharedCorridorTable,
        num_workers: usize,
        start: Instant,
        symmetry: bool,
        init_nodes: u64,
    ) -> Self {
        Self {
            config,
            tbase,
            ni_cache,
            inflight,
            corridor,
            num_workers,
            start,
            symmetry,
            abort_bn: AtomicBool::new(false),
            suspended: AtomicBool::new(false),
            global_nodes: CachePaddedAtomicU64(AtomicU64::new(init_nodes)),
            max_thpn: AtomicU16::new(0),
            winner_root: Mutex::new(None),
            scout_stats: Mutex::new(None),
            external_abort: None,
            progress_cb: None,
            last_progress_nodes: AtomicU64::new(init_nodes),
        }
    }

    fn with_hooks(
        mut self,
        external_abort: Option<Arc<AtomicBool>>,
        progress_cb: Option<Arc<dyn Fn(u64) + Send + Sync>>,
    ) -> Self {
        self.external_abort = external_abort;
        self.progress_cb = progress_cb;
        self
    }
}

struct RawWorkerScratch {
    path: LocalPathTable,
    mv_pool: Vec<Vec<MvItem>>,
    local_tt: Box<[LocalTerminalEntry]>,
    local_meta: LocalMetaTable,
    cycle_tracker: Option<Box<CycleTracker>>,
    epoch: u64,
}

impl RawWorkerScratch {
    fn new(pool_depth: usize, epoch: u64) -> Self {
        Self {
            path: LocalPathTable::new(),
            mv_pool: Vec::with_capacity(pool_depth),
            local_tt: vec![LocalTerminalEntry::EMPTY; LOCAL_TT_SIZE].into_boxed_slice(),
            local_meta: LocalMetaTable::new(4096, epoch),
            cycle_tracker: None,
            epoch,
        }
    }

    #[inline]
    fn sync_epoch(&mut self, epoch: u64) {
        if self.epoch != epoch {
            self.local_tt.fill(LocalTerminalEntry::EMPTY);
            self.local_meta.sync_epoch(epoch);
            if let Some(ct) = self.cycle_tracker.as_mut() {
                ct.clear();
            }
            self.epoch = epoch;
        }
    }
}

struct ScratchPool {
    slots: Box<[Mutex<Option<RawWorkerScratch>>]>,
}

impl ScratchPool {
    fn new(num_slots: usize) -> Self {
        let mut v = Vec::with_capacity(num_slots);
        for _ in 0..num_slots {
            v.push(Mutex::new(None));
        }
        Self {
            slots: v.into_boxed_slice(),
        }
    }

    #[inline]
    fn checkout(&self, idx: usize, epoch: u64) -> RawWorkerScratch {
        if let Some(mut raw) = self.slots.get(idx).and_then(|m| m.lock().unwrap().take()) {
            raw.sync_epoch(epoch);
            raw
        } else {
            RawWorkerScratch::new(96, epoch)
        }
    }

    #[inline]
    fn checkin(&self, idx: usize, raw: RawWorkerScratch) {
        if let Some(m) = self.slots.get(idx) {
            *m.lock().unwrap() = Some(raw);
        }
    }
}

struct WorkerCtx<'a, H> {
    tid: usize,
    shared: &'a SharedRunState<'a>,
    heuristic: H,
    path: LocalPathTable,
    mv_pool: Vec<Vec<MvItem>>,
    local_tt: Box<[LocalTerminalEntry]>,
    local_meta: LocalMetaTable,
    cycle_tracker: Option<Box<CycleTracker>>,
    raw_epoch: u64,
    local_nodes: u64,
    flushed_nodes: u64,
    local_max_thpn: u16,
    abort: bool,
    unwind_target: Option<usize>,
    warmup_cap: Option<u64>,
    eps_pn_shift: u8,
    deep_eps_pn_shift: u8,
    eps_dn_shift: u8,
    use_1ply: bool,
    use_ni: bool,
    is_cycle_scout: bool,
    in_cycle_probe: bool,
    skip_root_ni: bool,
    in_make_tree: bool,
}

impl<'a, H> WorkerCtx<'a, H> {
    #[inline]
    fn from_raw<P: PositionBackend>(
        tid: usize,
        shared: &'a SharedRunState<'a>,
        heuristic: H,
        mut raw: RawWorkerScratch,
        warmup_cap: Option<u64>,
        in_make_tree: bool,
    ) -> Self {
        let base_edn = if shared.config.eps_dn_shift > 0 {
            shared.config.eps_dn_shift
        } else if shared.config.use_ml_ordering {
            P::DEFAULT_EPS_DN_SHIFT
        } else {
            0
        };
        let cfg_epn = shared.config.eps_pn_shift;
        let base_deep_epn = if cfg_epn > 0 && cfg_epn <= 3 {
            cfg_epn
        } else {
            3
        };
        let tight_deep_epn = if cfg_epn > 0 && cfg_epn <= 4 {
            cfg_epn
        } else {
            4
        };
        let (eps_pn_shift, deep_eps_pn_shift, eps_dn_shift) = if tid == 0 {
            (cfg_epn, base_deep_epn, base_edn)
        } else if tid < 8 {
            match tid {
                1 => (if cfg_epn > 0 { cfg_epn } else { 4 }, tight_deep_epn, 3),
                2 => (if cfg_epn > 0 { cfg_epn } else { 5 }, tight_deep_epn, base_edn),
                3 => (cfg_epn, base_deep_epn, 5),
                4 => (cfg_epn, base_deep_epn, 3),
                5 => (if cfg_epn > 0 { cfg_epn } else { 4 }, tight_deep_epn, 5),
                6 => (if cfg_epn > 0 { cfg_epn } else { 5 }, tight_deep_epn, 3),
                _ => (if cfg_epn > 0 { cfg_epn } else { 3 }, base_deep_epn, base_edn),
            }
        } else {
            // High-thread (`16T`..`48T`) portfolio diversification across `(tid & 7, band)`
            let band = (tid >> 3) as u8;
            let epn = if cfg_epn > 0 {
                cfg_epn
            } else {
                match (tid + band as usize) & 7 {
                    0 => 0,
                    1 => 3,
                    2 => 4,
                    3 => 3,
                    4 => 5,
                    5 => 4,
                    6 => 6,
                    _ => 5,
                }
            };
            let depn = if cfg_epn > 0 {
                cfg_epn.min(4)
            } else {
                match (tid & 7, band & 1) {
                    (0 | 3, 0) => 3,
                    (0 | 3, _) => 2,
                    (1 | 5, 0) => 4,
                    (1 | 5, _) => 3,
                    (2 | 6, 0) => 5,
                    _ => 4,
                }
            };
            let edn = if shared.config.eps_dn_shift > 0 {
                shared.config.eps_dn_shift
            } else {
                2 + (((tid + (band as usize) * 3) & 3) as u8)
            };
            (epn, depn, edn)
        };

        let use_ni = !shared.config.simple_mate && shared.config.use_ni_cache;
        let scout_tid0 = if shared.num_workers >= 8 {
            6
        } else {
            shared.num_workers - 1
        };
        let is_cycle_scout = P::SUPPORTS_NI
            && use_ni
            && !in_make_tree
            && warmup_cap.is_none()
            && ((shared.num_workers >= 16 && (tid == 11 || tid == 27))
                || (shared.config.use_cycle_skip && tid == scout_tid0));
        if is_cycle_scout && raw.cycle_tracker.is_none() {
            raw.cycle_tracker = Some(Box::new(CycleTracker::new()));
        }

        Self {
            tid,
            shared,
            heuristic,
            path: raw.path,
            mv_pool: raw.mv_pool,
            local_tt: raw.local_tt,
            local_meta: raw.local_meta,
            cycle_tracker: raw.cycle_tracker,
            raw_epoch: raw.epoch,
            local_nodes: 0,
            flushed_nodes: 0,
            local_max_thpn: 0,
            abort: false,
            unwind_target: None,
            warmup_cap,
            eps_pn_shift,
            deep_eps_pn_shift,
            eps_dn_shift,
            use_1ply: shared.config.use_1ply_mate,
            use_ni,
            is_cycle_scout,
            in_cycle_probe: false,
            skip_root_ni: false,
            in_make_tree,
        }
    }

    #[inline]
    fn into_raw(mut self) -> (RawWorkerScratch, H) {
        while !self.path.entries.is_empty() {
            self.path.pop();
        }
        if let Some(ct) = self.cycle_tracker.as_mut() {
            ct.truncate_to_ply(0, 0);
        }
        (
            RawWorkerScratch {
                path: self.path,
                mv_pool: self.mv_pool,
                local_tt: self.local_tt,
                local_meta: self.local_meta,
                cycle_tracker: self.cycle_tracker,
                epoch: self.raw_epoch,
            },
            self.heuristic,
        )
    }

    #[inline(always)]
    fn acquire_list(&mut self) -> Vec<MvItem> {
        let mut v = self.mv_pool.pop().unwrap_or_else(|| Vec::with_capacity(64));
        v.clear();
        v
    }

    #[inline(always)]
    fn release_list(&mut self, v: Vec<MvItem>) {
        self.mv_pool.push(v);
    }

    #[inline(always)]
    fn bump_node(&mut self) {
        self.local_nodes += 1;
        if let Some(cap) = self.warmup_cap {
            if self.local_nodes >= cap {
                self.abort = true;
                return;
            }
        }
        if (self.local_nodes & 63) == 0 {
            if self.shared.abort_bn.load(Ordering::Relaxed) {
                self.abort = true;
                return;
            }
            let flush_mask = match self.shared.config.node_limit {
                Some(nl) if nl < 16384 => 255,
                _ => 1023,
            };
            if (self.local_nodes & flush_mask) == 0 {
                let delta = self.local_nodes - self.flushed_nodes;
                self.flushed_nodes = self.local_nodes;
                let tot = self.shared.global_nodes.fetch_add(delta, Ordering::Relaxed) + delta;
                if let Some(limit) = self.shared.config.node_limit {
                    if tot >= limit {
                        self.shared.suspended.store(true, Ordering::Relaxed);
                        self.shared.abort_bn.store(true, Ordering::Relaxed);
                        self.abort = true;
                        return;
                    }
                }
                if let Some(secs) = self.shared.config.time_limit_secs {
                    if self.shared.start.elapsed().as_secs_f64() >= secs {
                        self.shared.suspended.store(true, Ordering::Relaxed);
                        self.shared.abort_bn.store(true, Ordering::Relaxed);
                        self.abort = true;
                        return;
                    }
                }
                if let Some(flag) = &self.shared.external_abort {
                    if flag.load(Ordering::Relaxed) {
                        self.shared.suspended.store(true, Ordering::Relaxed);
                        self.shared.abort_bn.store(true, Ordering::Relaxed);
                        self.abort = true;
                        return;
                    }
                }
                if let Some(cb) = &self.shared.progress_cb {
                    let prev = self.shared.last_progress_nodes.load(Ordering::Relaxed);
                    if tot.saturating_sub(prev) >= 65536
                        && self
                            .shared
                            .last_progress_nodes
                            .compare_exchange(prev, tot, Ordering::Relaxed, Ordering::Relaxed)
                            .is_ok()
                    {
                        cb(tot);
                    }
                }
                if self.unwind_target.is_none() && self.warmup_cap.is_none() {
                    if let Some(anc_idx) = self.path.find_solved_ancestor(self.shared.tbase) {
                        self.unwind_target = Some(anc_idx);
                    }
                }
            }
        }
    }

    fn flush_stats(&mut self) {
        let rem = self.local_nodes - self.flushed_nodes;
        if rem > 0 {
            self.shared.global_nodes.fetch_add(rem, Ordering::Relaxed);
            self.flushed_nodes = self.local_nodes;
        }
        if self.local_max_thpn > 0 {
            self.shared
                .max_thpn
                .fetch_max(self.local_max_thpn, Ordering::Relaxed);
        }
        if self.is_cycle_scout {
            if let Some(ct) = self.cycle_tracker.as_ref() {
                let s = ct.stats;
                let mut guard = self.shared.scout_stats.lock().unwrap();
                let dst = guard.get_or_insert_with(CycleStats::default);
                dst.detected += s.detected;
                dst.rollouts_ok += s.rollouts_ok;
                dst.max_rollout_cycles = dst.max_rollout_cycles.max(s.max_rollout_cycles);
                dst.max_rollout_plies = dst.max_rollout_plies.max(s.max_rollout_plies);
                dst.boundary_pn0 += s.boundary_pn0;
                dst.boundary_dn0 += s.boundary_dn0;
                dst.corridor_registered += s.corridor_registered;
                dst.corridor_hits += s.corridor_hits;
                dst.corridor_invalidated += s.corridor_invalidated;
                dst.soft_bak_nodes_proven += s.soft_bak_nodes_proven;
                dst.soft_bak_full_ok += s.soft_bak_full_ok;
                dst.soft_bak_deep_deferred += s.soft_bak_deep_deferred;
                dst.soft_bak_dn0_abort += s.soft_bak_dn0_abort;
                dst.certs_extracted += s.certs_extracted;
                dst.covariant_replays_ok += s.covariant_replays_ok;
                dst.covariant_replays_fail += s.covariant_replays_fail;
                dst.probe_nodes += s.probe_nodes;
                dst.backup_nodes += s.backup_nodes;
            }
        }
    }

    #[inline(always)]
    fn stopping(&self) -> bool {
        self.abort
            || match self.unwind_target {
                Some(idx) => self.path.len() > idx,
                None => false,
            }
    }

    #[inline]
    fn try_1ply_mate_or<P: PositionBackend, const SIMPLE: bool>(
        &mut self,
        pos: &P,
        mvlist: &mut MvItem,
        tn: Turn,
        ks: &KeyState,
    ) -> bool {
        let m1_opt = if SIMPLE {
            pos.find_1ply_mate_simple()
        } else {
            pos.find_1ply_mate()
        };
        let Some(m1) = m1_opt else {
            return false;
        };
        self.bump_node();
        let mut child_item = MvItem::single_with_hints(m1.mv, m1.src_koma, m1.cap_koma);
        child_item.tdata = TData::TSUMI;
        let mut atk_after = pos.mkey(tn);
        if m1.mv.is_drop() {
            atk_after.dec(m1.mv.hand_ktype());
        } else if m1.cap_koma != 0 {
            atk_after.inc(m1.cap_koma % PROMOTED);
        }
        let child_pmk = if m1.is_double_check || IS_SKOMA[m1.final_atk_koma as usize] {
            MKey::EMPTY
        } else {
            let mut m = atk_after;
            let def_m = pos.mkey(tn.flip());
            if def_m.hi() != 0 {
                m.set_hi(0);
            }
            if def_m.ka() != 0 {
                m.set_ka(0);
            }
            if def_m.ki() != 0 {
                m.set_ki(0);
            }
            if def_m.gi() != 0 {
                m.set_gi(0);
            }
            if def_m.ke() != 0 {
                m.set_ke(0);
            }
            if def_m.ky() != 0 {
                m.set_ky(0);
            }
            if def_m.fu() != 0 {
                m.set_fu(0);
            }
            m
        };
        child_item.mkey = child_pmk;
        if !SIMPLE {
            if !atk_after.is_empty() {
                child_item.set_inc(true);
                child_item.nouse = child_pmk.total();
                child_item.nouse2 = atk_after.total();
            } else {
                child_item.set_inc(false);
            }
            child_item.set_hinc(!child_pmk.is_empty());
        }
        let child_ks = pos.key_forward_item(&child_item);
        self.shared.tbase.update(&child_ks, &mut child_item, tn);
        mvlist.tdata = TData {
            pn: 0,
            dn: INFINATE,
            sh: 1,
        };
        if SIMPLE {
            proof_koma_or_simple(mvlist, &child_item);
        } else {
            proof_koma_or(pos, mvlist, &child_item);
        }
        self.shared
            .tbase
            .update_with_path(ks, mvlist, tn, |z, m| self.path.has_exact(z, m));
        if P::SUPPORTS_NI && self.use_ni && pos.count() > 0 {
            let child_pos = pos.move_forward(m1.mv);
            self.ni_record_and_leaf_local(&child_pos);
            self.ni_record_or_proof(pos, mvlist, &child_item, 0);
        }
        true
    }

    #[inline]
    fn ni_apply_hit<P: PositionBackend, const SIMPLE: bool>(
        &mut self,
        pos: &P,
        hit: NiHit,
        hit_work: u16,
        att_cur: MKey,
        def_cur: MKey,
        mvlist: &mut MvItem,
        tn: Turn,
    ) -> bool {
        let ks = pos.key_state();
        let tb_mkey = proof_tbase_mkey(hit.att_min, hit.def_max, att_cur, def_cur);
        if !SIMPLE {
            let extra_forced = tb_mkey.total().saturating_sub(hit.att_min.total());
            let nouse = hit.nouse.saturating_add(extra_forced);
            let hinc = hit.hinc || extra_forced > 0;
            let extra_hand = att_cur.total().saturating_sub(tb_mkey.total());
            let inc = hinc || extra_hand > 0;
            if inc && pos.count() < 30 {
                return false;
            }
            mvlist.nouse = nouse;
            mvlist.nouse2 = extra_hand.saturating_add(nouse);
            mvlist.set_hinc(hinc);
            mvlist.set_inc(inc);
        }
        mvlist.tdata = hit.tdata;
        mvlist.mkey = tb_mkey;
        let fp = NiFootprint {
            valid: true,
            mask: hit.mask,
            att_min: hit.att_min,
            def_max: hit.def_max,
            sh: hit.tdata.sh,
            hinc: hit.hinc,
            nouse: hit.nouse,
        };
        let w16 = hit_work.max(MIN_NI_INSERT_WORK as u16);
        self.local_meta.record(ks.zkey, &fp, w16);
        self.shared.ni_cache.record_meta(ks.zkey, &fp, w16);
        self.shared.tbase.update(&ks, mvlist, tn);
        true
    }

    #[inline(always)]
    fn lookup_ni_meta(&self, zkey: u64, att_cur: MKey, def_cur: MKey) -> Option<(NiFootprint, u16)> {
        self.local_meta
            .lookup(zkey, att_cur, def_cur)
            .or_else(|| self.shared.ni_cache.lookup_meta(zkey, att_cur, def_cur))
    }

    #[inline(always)]
    fn record_ni_meta(&mut self, zkey: u64, fp: &NiFootprint, work: u16) {
        self.local_meta.record(zkey, fp, work);
        self.shared.ni_cache.record_meta(zkey, fp, work);
    }

    #[inline]
    fn ni_try_lookup_with_planes<P: PositionBackend, const SIMPLE: bool>(
        &mut self,
        pos: &P,
        planes: &BoardPlanes,
        rhash: u64,
        th_tdata: TData,
        mvlist: &mut MvItem,
        tn: Turn,
    ) -> bool {
        if tn != Turn::Sente || pos.n_oute() != 0 || !self.shared.ni_cache.has_tag_match(rhash) {
            return false;
        }
        let att_cur = pos.mkey(Turn::Sente);
        let def_cur = pos.mkey(Turn::Gote);
        let forbid_inc = !SIMPLE && pos.count() < 30;
        let Some((hit, hit_work)) = self.shared.ni_cache.lookup_hit(
            rhash,
            planes,
            att_cur,
            def_cur,
            th_tdata.sh,
            forbid_inc,
        ) else {
            return false;
        };
        self.ni_apply_hit::<P, SIMPLE>(pos, hit, hit_work, att_cur, def_cur, mvlist, tn)
    }

    #[inline]
    fn ni_try_lookup<P: PositionBackend, const SIMPLE: bool>(
        &mut self,
        pos: &P,
        th_tdata: TData,
        mvlist: &mut MvItem,
        tn: Turn,
    ) -> bool {
        if tn != Turn::Sente || pos.n_oute() != 0 {
            return false;
        }
        let rhash = pos.ni_ring_hash();
        if !self.shared.ni_cache.has_tag_match(rhash) {
            return false;
        }
        let planes = pos.ni_planes();
        self.ni_try_lookup_with_planes::<P, SIMPLE>(pos, &planes, rhash, th_tdata, mvlist, tn)
    }

    #[inline]
    fn ni_record_or_proof<P: PositionBackend>(
        &mut self,
        pos: &P,
        mvlist: &MvItem,
        best_child: &MvItem,
        local_work: u64,
    ) {
        let child_ks = pos.key_forward_item(best_child);
        let Some((child_fp, child_work)) =
            self.lookup_ni_meta(child_ks.zkey, child_ks.mkey[0], child_ks.mkey[1])
        else {
            return;
        };
        let mv = best_child.head_move();
        let Some(mv_mask) = pos.ni_proof_or_move_mask(mv, best_child.tdata.sh) else {
            return;
        };
        let mut att_min = child_fp.att_min;
        let mut hinc = child_fp.hinc;
        let mut nouse = child_fp.nouse;
        if mv.is_drop() {
            att_min.inc(mv.hand_ktype());
        } else if best_child.cap_koma != 0 {
            let kt = best_child.cap_koma % PROMOTED;
            if att_min.get(kt) == 0 {
                hinc = true;
                nouse = nouse.saturating_add(1);
            } else {
                att_min = mkey_min_dec_sat(att_min, kt);
            }
        }
        let fp = NiFootprint {
            valid: true,
            mask: child_fp.mask | mv_mask,
            att_min,
            def_max: child_fp.def_max,
            sh: mvlist.tdata.sh,
            hinc,
            nouse,
        };
        let tot_work = (child_work as u64).saturating_add(1).max(local_work);
        self.record_ni_meta(pos.zkey(), &fp, tot_work.min(u16::MAX as u64) as u16);
        if tot_work >= MIN_NI_INSERT_WORK && fp.sh >= MIN_NI_INSERT_SH {
            let (planes, rhash) = pos.ni_planes_and_ring_hash();
            self.shared.ni_cache.insert(rhash, &planes, &fp, tot_work);
        }
    }

    #[inline]
    fn ni_record_and_leaf_local<P: PositionBackend>(&mut self, pos: &P) {
        let Some((mask, def_max)) = pos.ni_proof_leaf() else {
            return;
        };
        let fp = NiFootprint {
            valid: true,
            mask,
            att_min: MKey::EMPTY,
            def_max,
            sh: 0,
            hinc: false,
            nouse: 0,
        };
        self.local_meta.record(pos.zkey(), &fp, 1);
    }

    #[inline]
    fn ni_record_and_leaf<P: PositionBackend>(&mut self, pos: &P) {
        let Some((mask, def_max)) = pos.ni_proof_leaf() else {
            return;
        };
        let fp = NiFootprint {
            valid: true,
            mask,
            att_min: MKey::EMPTY,
            def_max,
            sh: 0,
            hinc: false,
            nouse: 0,
        };
        self.record_ni_meta(pos.zkey(), &fp, 1);
    }

    #[inline]
    fn ni_record_and_proof<P: PositionBackend>(
        &mut self,
        pos: &P,
        mvlist: &MvItem,
        evasions: &[MvItem],
        local_work: u64,
    ) {
        let Some((mut mask, mut def_max)) = pos.ni_proof_and_local(evasions) else {
            return;
        };
        let mut att_min = MKey::EMPTY;
        let mut head_nouse = 0u8;
        let mut head_att_tot = 0u8;
        let mut head_hinc = false;
        let mut sum_child_work = 0u64;
        for (idx, ev_item) in evasions.iter().enumerate() {
            let child_ks = pos.key_forward_item(ev_item);
            let Some((child_fp, cw)) =
                self.lookup_ni_meta(child_ks.zkey, child_ks.mkey[0], child_ks.mkey[1])
            else {
                return;
            };
            sum_child_work = sum_child_work.saturating_add(cw as u64);
            if idx == 0 {
                head_nouse = child_fp.nouse;
                head_att_tot = child_fp.att_min.total();
                head_hinc = child_fp.hinc;
            }
            let ev = ev_item.head_move();
            mask |= if ev.is_drop() {
                crate::futsuu::ni_cache::upgrade_ni_mask_for_white_drop(
                    child_fp.mask,
                    ev.hand_ktype(),
                    sq_file(ev.dest()),
                )
            } else {
                child_fp.mask
            };
            att_min = att_min.max(child_fp.att_min);
            let c_def = if ev.is_drop() {
                mkey_bound_inc(child_fp.def_max, ev.hand_ktype())
            } else if ev_item.cap_koma != 0 {
                mkey_bound_dec_if_bounded(child_fp.def_max, ev_item.cap_koma % PROMOTED)
            } else {
                child_fp.def_max
            };
            def_max = def_max.min(c_def);
        }
        let nouse = head_nouse.saturating_add(att_min.total().saturating_sub(head_att_tot));
        let hinc = head_hinc || nouse != 0;
        let fp = NiFootprint {
            valid: true,
            mask,
            att_min,
            def_max,
            sh: mvlist.tdata.sh,
            hinc,
            nouse,
        };
        let tot_work = sum_child_work.saturating_add(1).max(local_work);
        self.record_ni_meta(pos.zkey(), &fp, tot_work.min(u16::MAX as u64) as u16);
        if tot_work >= MIN_NI_INSERT_WORK && fp.sh >= MIN_NI_INSERT_SH {
            let (planes, rhash) = pos.ni_planes_and_ring_hash();
            self.shared.ni_cache.insert(rhash, &planes, &fp, tot_work);
        }
    }

    fn probe_or_bounded_local<P: PositionBackend, const SIMPLE: bool>(
        &mut self,
        or_pos: &P,
        thdata: TData,
        item: &mut MvItem,
        max_nodes: u64,
    ) where
        H: SearchHeuristic<P>,
    {
        if self.stopping() || self.shared.abort_bn.load(Ordering::Relaxed) {
            return;
        }
        let saved_cap = self.warmup_cap;
        let saved_abort = self.abort;
        let saved_probe = self.in_cycle_probe;
        let saved_unwind = self.unwind_target;
        let target = self.local_nodes.saturating_add(max_nodes);
        self.warmup_cap = Some(match saved_cap {
            Some(c) => c.min(target),
            None => target,
        });
        self.in_cycle_probe = true;
        self.unwind_target = None;
        self.bn_search_or::<P, SIMPLE>(or_pos, thdata, item);
        self.in_cycle_probe = saved_probe;
        self.warmup_cap = saved_cap;
        self.unwind_target = saved_unwind;
        let global_stop = self.shared.abort_bn.load(Ordering::Relaxed)
            || self.shared.suspended.load(Ordering::Relaxed)
            || saved_cap.is_some_and(|c| self.local_nodes >= c);
        if !global_stop {
            self.abort = saved_abort;
        }
    }

    fn extract_covariant_cert_parallel<P: PositionBackend, const SIMPLE: bool>(
        &mut self,
        root_or: &P,
        root_ev_mv: Move,
        root_captured_tracked: bool,
        root_moved_tracked: bool,
        init_tracked: Option<u8>,
    ) -> Option<CovariantCert> {
        let mut nodes: Vec<CertOrNode> = Vec::with_capacity(32);
        let mut memo: std::collections::HashMap<(u64, MKey, Option<u8>), u16> =
            std::collections::HashMap::with_capacity(32);
        let mut visiting: HashSet<(u64, MKey)> = HashSet::with_capacity(32);
        let tn = root_or.turn();
        self.extract_cert_or_rec_parallel::<P, SIMPLE>(
            root_or,
            init_tracked,
            tn,
            &mut nodes,
            &mut memo,
            &mut visiting,
        )?;
        Some(CovariantCert {
            root_ev_mv,
            root_captured_tracked,
            root_moved_tracked,
            nodes,
        })
    }

    fn extract_cert_or_rec_parallel<P: PositionBackend, const SIMPLE: bool>(
        &mut self,
        pos: &P,
        cur_tracked: Option<u8>,
        tn: Turn,
        nodes: &mut Vec<CertOrNode>,
        memo: &mut std::collections::HashMap<(u64, MKey, Option<u8>), u16>,
        visiting: &mut HashSet<(u64, MKey)>,
    ) -> Option<u16> {
        if nodes.len() >= MAX_CERT_NODES {
            return None;
        }
        let zkey = pos.zkey();
        let att_mk = pos.mkey(tn);
        let memo_key = (zkey, att_mk, cur_tracked);
        if let Some(&idx) = memo.get(&memo_key) {
            return Some(idx);
        }
        if !visiting.insert((zkey, att_mk)) {
            return None;
        }

        let mut list = self.acquire_list();
        pos.generate_check_fast(&mut list);
        for item in list.iter_mut() {
            let cks = pos.key_forward_item(item);
            self.shared.tbase.lookup(&cks, item, tn);
        }
        if SIMPLE {
            sort_or_simple(&mut list);
        } else {
            sort_or(&mut list);
        }

        let chosen_opt: Option<(Move, bool)> = if !list.is_empty() && list[0].tdata.pn == 0 {
            Some((list[0].head_move(), list[0].tdata.sh == 0))
        } else {
            let m1 = if SIMPLE {
                pos.find_1ply_mate_simple()
            } else {
                pos.find_1ply_mate()
            };
            m1.map(|w| (w.mv, true))
        };
        self.release_list(list);

        let my_idx = nodes.len() as u16;
        nodes.push(CertOrNode::default());

        let Some((chosen_mv, is_imm_leaf)) = chosen_opt else {
            visiting.remove(&(zkey, att_mk));
            nodes[my_idx as usize] = CertOrNode {
                best_mv: Move::default(),
                orig_tracked: cur_tracked,
                moved_tracked: false,
                edges: smallvec::SmallVec::new(),
            };
            memo.insert(memo_key, my_idx);
            return Some(my_idx);
        };

        let moved_tracked =
            cur_tracked.is_some_and(|s| chosen_mv.is_board_move() && chosen_mv.prev_pos == s);
        let and_tracked = if moved_tracked {
            Some(chosen_mv.dest() as u8)
        } else {
            cur_tracked
        };

        let mut edges: smallvec::SmallVec<[CertEdge; 4]> = smallvec::SmallVec::new();
        if !is_imm_leaf {
            let and_pos = pos.move_forward(chosen_mv);
            let mut ev_list = self.acquire_list();
            if SIMPLE {
                and_pos.generate_evasion_simple_fast(&mut ev_list);
            } else {
                let mut oracle = WorkerOracle {
                    tbase: self.shared.tbase,
                    local_tt: Some(&mut self.local_tt),
                    hs_depth: 0,
                };
                let _ = and_pos.generate_evasion_with(&mut oracle, &mut ev_list);
            }
            let mut ei = 0;
            while ei < ev_list.len() {
                if let Some(tail) = ev_list[ei].take_tail() {
                    ev_list.insert(ei + 1, split_tail_item(&and_pos, tail));
                }
                ei += 1;
            }
            let mut ok = true;
            for ev_item in &ev_list {
                let hm = ev_item.head_move();
                let cap_tr = and_tracked.is_some_and(|s| hm.dest() == s as usize);
                let mov_tr = and_tracked.is_some_and(|s| hm.is_board_move() && hm.prev_pos == s);
                let next_tr = if cap_tr {
                    None
                } else if mov_tr {
                    Some(hm.dest() as u8)
                } else {
                    and_tracked
                };
                let child_or = and_pos.move_forward(hm);
                if let Some(c_idx) = self.extract_cert_or_rec_parallel::<P, SIMPLE>(
                    &child_or,
                    next_tr,
                    tn,
                    nodes,
                    memo,
                    visiting,
                ) {
                    edges.push(CertEdge {
                        ev_mv: hm,
                        captured_tracked: cap_tr,
                        moved_tracked: mov_tr,
                        child_idx: c_idx,
                    });
                } else {
                    ok = false;
                    break;
                }
            }
            self.release_list(ev_list);
            if !ok {
                visiting.remove(&(zkey, att_mk));
                return None;
            }
        }

        visiting.remove(&(zkey, att_mk));
        nodes[my_idx as usize] = CertOrNode {
            best_mv: chosen_mv,
            orig_tracked: cur_tracked,
            moved_tracked,
            edges,
        };
        memo.insert(memo_key, my_idx);
        Some(my_idx)
    }

    #[allow(clippy::too_many_arguments)]
    fn replay_covariant_or_parallel<P: PositionBackend, const SIMPLE: bool>(
        &mut self,
        pos: &P,
        cert: &CovariantCert,
        node_idx: u16,
        cur_tracked: Option<u8>,
        th_tdata: TData,
        out_item: &mut MvItem,
        rem_depth: u16,
        rem_fallback_nodes: &mut u64,
    ) -> bool
    where
        H: SearchHeuristic<P>,
    {
        let max_d = if SIMPLE {
            SIMPLE_MAX_DEPTH
        } else {
            TSUME_MAX_DEPTH
        };
        if self.stopping()
            || self.shared.abort_bn.load(Ordering::Relaxed)
            || rem_depth == 0
            || pos.count() >= max_d - 1
            || th_tdata.sh < 2
        {
            return false;
        }
        let tn = pos.turn();
        let or_ks = pos.key_state();
        self.shared.tbase.lookup(&or_ks, out_item, tn);
        if out_item.tdata.pn == 0 && out_item.tdata.sh < th_tdata.sh {
            return true;
        }
        if out_item.tdata.dn == 0 {
            return false;
        }
        if P::SUPPORTS_NI
            && self.use_ni
            && self.ni_try_lookup::<P, SIMPLE>(pos, th_tdata, out_item, tn)
        {
            return true;
        }
        if self.try_1ply_mate_or::<P, SIMPLE>(pos, out_item, tn, &or_ks) {
            return true;
        }

        let Some(node) = cert.nodes.get(node_idx as usize) else {
            return false;
        };
        if node.best_mv == Move::default() {
            if *rem_fallback_nodes >= 32 {
                let b = (*rem_fallback_nodes).min(512);
                let n0 = self.local_nodes;
                self.probe_or_bounded_local::<P, SIMPLE>(pos, th_tdata, out_item, b);
                *rem_fallback_nodes =
                    rem_fallback_nodes.saturating_sub(self.local_nodes.saturating_sub(n0));
                return out_item.tdata.pn == 0 && out_item.tdata.sh < th_tdata.sh;
            }
            return false;
        }

        let best_mv = node.best_mv;
        let moved_tracked = node.moved_tracked;
        let orig_tracked = node.orig_tracked;
        let edges = node.edges.clone();

        let mut check_list = self.acquire_list();
        pos.generate_check_fast(&mut check_list);
        self.bump_node();
        if check_list.is_empty() {
            self.release_list(check_list);
            return false;
        }

        let mut cand_indices: smallvec::SmallVec<[usize; 4]> = smallvec::SmallVec::new();
        let push_unique = |idx: usize, v: &mut smallvec::SmallVec<[usize; 4]>| {
            if v.len() < 4 && !v.contains(&idx) {
                v.push(idx);
            }
        };

        if !moved_tracked {
            if let Some(idx) = check_list.iter().position(|it| it.head_move() == best_mv) {
                push_unique(idx, &mut cand_indices);
            }
        } else if let (Some(orig_src), Some(cur_src)) = (orig_tracked, cur_tracked) {
            let orig_dest = best_mv.dest();
            let orig_prom = best_mv.is_promote();
            for want_prom in [orig_prom, !orig_prom] {
                if let Some(idx) = check_list.iter().position(|it| {
                    let m = it.head_move();
                    m.is_board_move()
                        && m.prev_pos == cur_src
                        && m.dest() == orig_dest
                        && m.is_promote() == want_prom
                }) {
                    push_unique(idx, &mut cand_indices);
                }
            }
            let df = sq_file(cur_src as usize) as i32 - sq_file(orig_src as usize) as i32;
            let dr = sq_rank(cur_src as usize) as i32 - sq_rank(orig_src as usize) as i32;
            if let Some(smv) = shift_move(best_mv, df, dr) {
                let sdest = smv.dest();
                for want_prom in [orig_prom, !orig_prom] {
                    if let Some(idx) = check_list.iter().position(|it| {
                        let m = it.head_move();
                        m.is_board_move()
                            && m.prev_pos == cur_src
                            && m.dest() == sdest
                            && m.is_promote() == want_prom
                    }) {
                        push_unique(idx, &mut cand_indices);
                    }
                }
            }
            for (idx, it) in check_list.iter().enumerate() {
                let m = it.head_move();
                if m.is_board_move() && m.prev_pos == cur_src {
                    push_unique(idx, &mut cand_indices);
                }
            }
        }

        for &c_idx in &cand_indices {
            if self.stopping() || self.shared.abort_bn.load(Ordering::Relaxed) {
                break;
            }
            let mut cand_item = check_list[c_idx].clone();
            let cand_mv = cand_item.head_move();
            let and_ks = pos.key_forward_item(&cand_item);
            self.shared.tbase.lookup(&and_ks, &mut cand_item, tn);
            if cand_item.tdata.dn == 0 {
                continue;
            }
            let and_th = TData {
                pn: PROOF_MAX - 1,
                dn: INFINATE - 1,
                sh: th_tdata.sh.saturating_sub(1),
            };
            let and_pos = pos.move_forward(cand_mv);
            if cand_item.tdata.pn != 0
                && P::SUPPORTS_NI
                && self.use_ni
                && self.ni_try_lookup::<P, SIMPLE>(&and_pos, and_th, &mut cand_item, tn)
            {
                // Proven via SharedNiCache
            }

            if cand_item.tdata.pn != 0 {
                let and_tracked =
                    if cur_tracked.is_some_and(|s| cand_mv.is_board_move() && cand_mv.prev_pos == s)
                    {
                        Some(cand_mv.dest() as u8)
                    } else {
                        cur_tracked
                    };
                let mut ev_list = self.acquire_list();
                let (proof_flag, invalid_flag) = if SIMPLE {
                    and_pos.generate_evasion_simple_fast(&mut ev_list);
                    (false, false)
                } else {
                    let mut oracle = WorkerOracle {
                        tbase: self.shared.tbase,
                        local_tt: Some(&mut self.local_tt),
                        hs_depth: 0,
                    };
                    and_pos.generate_evasion_with(&mut oracle, &mut ev_list)
                };
                self.bump_node();

                if ev_list.is_empty() {
                    if SIMPLE {
                        cand_item.tdata = TData::TSUMI;
                        cand_item.mkey = proof_koma(&and_pos);
                    } else {
                        tsumi_proof(&and_pos, &mut cand_item, proof_flag);
                    }
                    self.shared.tbase.update(&and_ks, &mut cand_item, tn);
                    if P::SUPPORTS_NI && self.use_ni && !proof_flag && !invalid_flag {
                        self.ni_record_and_leaf(&and_pos);
                    }
                } else {
                    let mut ei = 0;
                    while ei < ev_list.len() {
                        if let Some(tail) = ev_list[ei].take_tail() {
                            ev_list.insert(ei + 1, split_tail_item(&and_pos, tail));
                        }
                        ei += 1;
                    }
                    let mut all_ok = true;
                    for ev_item in ev_list.iter_mut() {
                        let hm = ev_item.head_move();
                        let cks = and_pos.key_forward_item(ev_item);
                        self.shared.tbase.lookup(&cks, ev_item, tn);
                        if ev_item.tdata.pn == 0 && ev_item.tdata.sh < and_th.sh {
                            continue;
                        }
                        if ev_item.tdata.dn == 0 {
                            all_ok = false;
                            break;
                        }
                        let cap_tr = and_tracked.is_some_and(|s| hm.dest() == s as usize);
                        let mov_tr =
                            and_tracked.is_some_and(|s| hm.is_board_move() && hm.prev_pos == s);
                        let next_tr = if cap_tr {
                            None
                        } else if mov_tr {
                            Some(hm.dest() as u8)
                        } else {
                            and_tracked
                        };
                        let cpos = and_pos.move_forward(hm);
                        let cth = TData {
                            pn: PROOF_MAX - 1,
                            dn: INFINATE - 1,
                            sh: and_th.sh.saturating_sub(1),
                        };
                        if P::SUPPORTS_NI
                            && self.use_ni
                            && self.ni_try_lookup::<P, SIMPLE>(&cpos, cth, ev_item, tn)
                        {
                            continue;
                        }
                        if cth.sh >= 2
                            && self.try_1ply_mate_or::<P, SIMPLE>(&cpos, ev_item, tn, &cks)
                        {
                            continue;
                        }
                        let matched_edge = edges
                            .iter()
                            .find(|e| !e.captured_tracked && !e.moved_tracked && e.ev_mv == hm)
                            .or_else(|| {
                                if cap_tr {
                                    edges.iter().find(|e| {
                                        e.captured_tracked
                                            && e.ev_mv.is_drop() == hm.is_drop()
                                            && (!hm.is_board_move()
                                                || e.ev_mv.prev_pos == hm.prev_pos)
                                    })
                                } else if mov_tr {
                                    edges.iter().find(|e| e.moved_tracked)
                                } else {
                                    edges.iter().find(|e| e.ev_mv == hm)
                                }
                            });
                        if let Some(edge) = matched_edge {
                            let _ = self.replay_covariant_or_parallel::<P, SIMPLE>(
                                &cpos,
                                cert,
                                edge.child_idx,
                                next_tr,
                                cth,
                                ev_item,
                                rem_depth - 1,
                                rem_fallback_nodes,
                            );
                        }
                        if ev_item.tdata.pn != 0
                            && ev_item.tdata.dn != 0
                            && *rem_fallback_nodes >= 16
                        {
                            let b = (*rem_fallback_nodes).min(512);
                            let n0 = self.local_nodes;
                            self.probe_or_bounded_local::<P, SIMPLE>(&cpos, cth, ev_item, b);
                            *rem_fallback_nodes = rem_fallback_nodes
                                .saturating_sub(self.local_nodes.saturating_sub(n0));
                        }
                        if ev_item.tdata.pn != 0 || ev_item.tdata.sh >= and_th.sh {
                            all_ok = false;
                            break;
                        }
                    }

                    if all_ok {
                        if SIMPLE {
                            sort_and_simple(&mut ev_list);
                        } else {
                            sort_and(&mut ev_list);
                        }
                        if !SIMPLE && invalid_flag && ev_list[0].tdata.pn == 0 && ev_list[0].inc() {
                            tsumi_proof(&and_pos, &mut cand_item, proof_flag);
                        } else {
                            cand_item.tdata = TData {
                                pn: 0,
                                dn: INFINATE,
                                sh: ev_list[0].tdata.sh.saturating_add(1),
                            };
                            if SIMPLE {
                                proof_koma_and_simple(&and_pos, &mut cand_item, &ev_list);
                            } else if proof_flag {
                                let enemy_mkey = and_pos.mkey(and_pos.turn().flip());
                                cand_item.mkey = enemy_mkey;
                                cand_item.set_inc(ev_list[0].inc());
                                cand_item.set_hinc(ev_list[0].inc());
                                cand_item.nouse = ev_list[0].nouse.saturating_add(
                                    enemy_mkey.total().saturating_sub(ev_list[0].mkey.total()),
                                );
                                cand_item.nouse2 = cand_item.nouse;
                            } else {
                                proof_koma_and(&and_pos, &mut cand_item, &ev_list);
                            }
                        }
                        self.shared.tbase.update(&and_ks, &mut cand_item, tn);
                        if P::SUPPORTS_NI && self.use_ni && !proof_flag && !invalid_flag {
                            self.ni_record_and_proof(
                                &and_pos,
                                &cand_item,
                                &ev_list,
                                MIN_NI_INSERT_WORK,
                            );
                        }
                    }
                }
                self.release_list(ev_list);
            }

            if cand_item.tdata.pn == 0 && cand_item.tdata.sh < th_tdata.sh.saturating_sub(1) {
                out_item.tdata = TData {
                    pn: 0,
                    dn: INFINATE,
                    sh: cand_item.tdata.sh.saturating_add(1),
                };
                if SIMPLE {
                    proof_koma_or_simple(out_item, &cand_item);
                } else {
                    proof_koma_or(pos, out_item, &cand_item);
                }
                self.shared.tbase.update(&or_ks, out_item, tn);
                if P::SUPPORTS_NI && self.use_ni && pos.count() > 0 {
                    self.ni_record_or_proof(pos, out_item, &cand_item, MIN_NI_INSERT_WORK);
                }
                self.release_list(check_list);
                return true;
            }
        }

        self.release_list(check_list);
        false
    }

    fn try_cycle_skip_parallel<P: PositionBackend, const SIMPLE: bool>(
        &mut self,
        pos: &P,
        planes: &BoardPlanes,
        rhash: u64,
        th_tdata: TData,
        mvlist: &mut MvItem,
    ) -> bool
    where
        H: SearchHeuristic<P>,
    {
        let cur_nodes = self.local_nodes;
        let max_d = if SIMPLE {
            SIMPLE_MAX_DEPTH
        } else {
            TSUME_MAX_DEPTH
        };

        let mut chosen_rollout: Option<(CycleDetection, Vec<(P, Vec<Move>)>, P)> = None;
        for _attempt in 0..2 {
            let det = {
                let Some(ct) = self.cycle_tracker.as_mut() else {
                    return false;
                };
                // Dedicated scout worker spends <= 20% of its own nodes (~2.5% of 8T CPU)
                let spent = ct.stats.probe_nodes + ct.stats.backup_nodes;
                if cur_nodes > 30_000 && spent * 5 > cur_nodes {
                    return false;
                }
                let Some(d) = ct.detect_cycle(pos, planes, rhash, cur_nodes) else {
                    return false;
                };
                if d.sigma.len() < 16 && cur_nodes > 20_000 && spent * 10 > cur_nodes {
                    return false;
                }
                // In default parallel mode (`!use_cycle_skip`), let `SharedNiCache` handle early
                // Type II board translations (`shift != (0, 0)`, e.g. Microcosmos M1525) so scout
                // fast-forwarding does not lock in a non-minimal cycle length before `2.5M` local nodes.
                if !self.shared.config.use_cycle_skip && d.shift != (0, 0) && cur_nodes < 2_500_000 {
                    return false;
                }
                d
            };
            let l_try = det.sigma.len();
            let (cycles, boundary_pos) = rollout_corridor(pos, &det, max_d);
            let total_plies = cycles.len() * l_try;
            if cycles.is_empty() || total_plies < 16 {
                if let Some(ct) = self.cycle_tracker.as_mut() {
                    let cd = if l_try < 16 { 40_000 } else { 18_000 };
                    ct.set_pattern_cooldown(
                        l_try,
                        det.kind,
                        det.shift,
                        det.cur_ply,
                        self.local_nodes + cd,
                    );
                }
                continue;
            }
            chosen_rollout = Some((det, cycles, boundary_pos));
            break;
        }

        let Some((det, cycles, boundary_pos)) = chosen_rollout else {
            return false;
        };
        let l = det.sigma.len();
        let shift = det.shift;
        let tracked_start_sq = det.tracked_start_sq;
        let kind = det.kind;
        let cur_ply = det.cur_ply;
        let n_cycles = cycles.len();
        let total_plies = n_cycles * l;

        if let Some(ct) = self.cycle_tracker.as_mut() {
            ct.stats.rollouts_ok += 1;
            ct.stats.max_rollout_cycles = ct.stats.max_rollout_cycles.max(n_cycles);
            ct.stats.max_rollout_plies = ct.stats.max_rollout_plies.max(total_plies);
        }

        let tn = pos.turn();
        let base_budget = if l >= 40 {
            36_000u64
        } else if l >= 16 {
            28_000u64
        } else {
            6_000u64
        };

        let mut proven_boundary: Option<(usize, MvItem)> = None;
        let mut candidates: Vec<(usize, P)> = Vec::with_capacity(n_cycles);
        candidates.push((n_cycles - 1, boundary_pos));
        if shift == (0, 0) {
            for k in (1..n_cycles).rev() {
                candidates.push((k - 1, cycles[k].0.clone()));
            }
        } else if n_cycles >= 2 {
            candidates.push((n_cycles - 2, cycles[n_cycles - 1].0.clone()));
        }

        let mut unknown_stepbacks: usize = 0;
        for (c_idx, bpos) in candidates {
            if self.stopping() || self.shared.abort_bn.load(Ordering::Relaxed) {
                break;
            }
            let bks = bpos.key_state();
            let mut bitem = MvItem::empty_root();
            self.shared.tbase.lookup(&bks, &mut bitem, tn);
            if bitem.tdata.pn != 0 && bitem.tdata.dn != 0 {
                let bth = TData {
                    pn: PROOF_MAX - 1,
                    dn: INFINATE - 1,
                    sh: max_d.saturating_sub(bpos.count() + 1),
                };
                if !(P::SUPPORTS_NI
                    && self.use_ni
                    && self.ni_try_lookup::<P, SIMPLE>(&bpos, bth, &mut bitem, tn))
                {
                    let bud = if unknown_stepbacks == 0 {
                        base_budget
                    } else {
                        base_budget / 2
                    };
                    let n0 = self.local_nodes;
                    self.probe_or_bounded_local::<P, SIMPLE>(&bpos, bth, &mut bitem, bud);
                    let used = self.local_nodes.saturating_sub(n0);
                    if let Some(ct) = self.cycle_tracker.as_mut() {
                        ct.stats.probe_nodes += used;
                    }
                }
            }
            if bitem.tdata.pn == 0 {
                if !SIMPLE && shift == (0, 0) && bitem.inc() {
                    continue;
                }
                if let Some(ct) = self.cycle_tracker.as_mut() {
                    ct.stats.boundary_pn0 += 1;
                }
                proven_boundary = Some((c_idx, bitem));
                break;
            } else if bitem.tdata.dn == 0 {
                if let Some(ct) = self.cycle_tracker.as_mut() {
                    ct.stats.boundary_dn0 += 1;
                }
                continue;
            } else if kind.is_hand_monotone() || unknown_stepbacks >= 1 {
                break;
            } else {
                unknown_stepbacks += 1;
            }
        }

        let Some((proven_idx, mut cur_or_item)) = proven_boundary else {
            if let Some(ct) = self.cycle_tracker.as_mut() {
                let cd = if l < 16 { 100_000 } else { 55_000 };
                ct.set_pattern_cooldown(l, kind, shift, cur_ply, self.local_nodes + cd);
            }
            return false;
        };

        let n_pairs = l / 2;
        let mut step_data: Vec<Vec<(P, Move, P, Move, Option<u8>)>> =
            Vec::with_capacity(proven_idx + 1);
        for c in 0..=proven_idx {
            let (ref c_start, ref mvs) = cycles[c];
            let mut cur = c_start.clone();
            let mut tr_cur = tracked_start_sq
                .and_then(|s0| shift_sq(s0 as usize, shift.0 * (c as i32), shift.1 * (c as i32)));
            let mut pairs = Vec::with_capacity(n_pairs);
            let mut j = 0;
            while j + 1 < mvs.len() {
                let or_mv = mvs[j];
                let ev_mv = mvs[j + 1];
                let tr_and =
                    if tr_cur.is_some_and(|s| or_mv.is_board_move() && or_mv.prev_pos == s) {
                        Some(or_mv.dest() as u8)
                    } else {
                        tr_cur
                    };
                let and_pos = cur.move_forward(or_mv);
                let next_or = and_pos.move_forward(ev_mv);
                tr_cur = if tr_and.is_some_and(|s| ev_mv.dest() == s as usize) {
                    None
                } else if tr_and.is_some_and(|s| ev_mv.is_board_move() && ev_mv.prev_pos == s) {
                    Some(ev_mv.dest() as u8)
                } else {
                    tr_and
                };
                pairs.push((cur, or_mv, and_pos, ev_mv, tr_and));
                cur = next_or;
                j += 2;
            }
            step_data.push(pairs);
        }

        let backup_n0 = self.local_nodes;
        let is_multi_long = proven_idx >= 1 && l >= 40;
        let template_side_budget: u64 = if is_multi_long {
            32_000
        } else if proven_idx >= 1 {
            16_000
        } else {
            6_000
        };
        let max_backup_nodes: u64 = if is_multi_long {
            450_000
        } else if proven_idx >= 1 {
            180_000
        } else {
            80_000
        };
        let replay_miss_side_budget: u64 = if is_multi_long { 8_000 } else { 4_000 };

        let mut step_certs: Vec<Vec<CovariantCert>> = vec![Vec::new(); n_pairs];
        let mut active_top = proven_idx;
        let mut chain_intact = true;
        let mut refuted_cycle = false;
        let mut likely_escape = false;

        'outer_bak: for c in (0..=proven_idx).rev() {
            if self.stopping()
                || self.shared.abort_bn.load(Ordering::Relaxed)
                || (!chain_intact && c < active_top)
                || (refuted_cycle && c < active_top)
            {
                break;
            }
            if self.local_nodes.saturating_sub(backup_n0) >= max_backup_nodes {
                chain_intact = false;
                break;
            }
            let pairs = &step_data[c];
            for p_idx in (0..pairs.len()).rev() {
                if self.stopping()
                    || self.shared.abort_bn.load(Ordering::Relaxed)
                    || self.local_nodes.saturating_sub(backup_n0) >= max_backup_nodes
                {
                    chain_intact = false;
                    break 'outer_bak;
                }
                let (ref or_pos, or_mv, ref and_pos, ev_mv, tr_and) = pairs[p_idx];
                let and_ks = and_pos.key_state();

                let mut and_item = MvItem::single_with_hints(
                    or_mv,
                    if or_mv.is_board_move() {
                        or_pos.board_at(or_mv.prev_pos as usize)
                    } else {
                        0
                    },
                    or_pos.board_at(or_mv.dest()),
                );
                self.shared.tbase.lookup(&and_ks, &mut and_item, tn);
                if and_item.tdata.pn != 0
                    && P::SUPPORTS_NI
                    && self.use_ni
                    && self.ni_try_lookup::<P, SIMPLE>(
                        and_pos,
                        TData {
                            pn: PROOF_MAX - 1,
                            dn: INFINATE - 1,
                            sh: max_d.saturating_sub(and_pos.count() + 1),
                        },
                        &mut and_item,
                        tn,
                    )
                {
                    // Proven via SharedNiCache
                }

                if and_item.tdata.pn != 0 {
                    let mut ev_list = self.acquire_list();
                    let (proof_flag, invalid_flag) = if SIMPLE {
                        and_pos.generate_evasion_simple_fast(&mut ev_list);
                        (false, false)
                    } else {
                        let mut oracle = WorkerOracle {
                            tbase: self.shared.tbase,
                            local_tt: Some(&mut self.local_tt),
                            hs_depth: 0,
                        };
                        and_pos.generate_evasion_with(&mut oracle, &mut ev_list)
                    };
                    self.bump_node();
                    let mut ei = 0;
                    while ei < ev_list.len() {
                        if let Some(tail) = ev_list[ei].take_tail() {
                            ev_list.insert(ei + 1, split_tail_item(and_pos, tail));
                        }
                        ei += 1;
                    }

                    let mut all_ev_proven = true;

                    for ev_item in ev_list.iter_mut() {
                        let hm = ev_item.head_move();
                        let cks = and_pos.key_forward_item(ev_item);
                        self.shared.tbase.lookup(&cks, ev_item, tn);
                        if ev_item.tdata.pn == 0 {
                            continue;
                        }
                        if ev_item.tdata.dn == 0 {
                            refuted_cycle = true;
                            all_ev_proven = false;
                            break;
                        }
                        if hm == ev_mv {
                            if cur_or_item.tdata.pn == 0 {
                                copy_proof_fields(ev_item, &cur_or_item);
                                continue;
                            } else {
                                all_ev_proven = false;
                                break;
                            }
                        }
                        let cpos = and_pos.move_forward(hm);
                        let cth = TData {
                            pn: PROOF_MAX - 1,
                            dn: INFINATE - 1,
                            sh: max_d.saturating_sub(cpos.count() + 1),
                        };
                        if P::SUPPORTS_NI
                            && self.use_ni
                            && self.ni_try_lookup::<P, SIMPLE>(&cpos, cth, ev_item, tn)
                        {
                            continue;
                        }
                        if cth.sh >= 2
                            && self.try_1ply_mate_or::<P, SIMPLE>(&cpos, ev_item, tn, &cks)
                        {
                            continue;
                        }

                        let cap_tr = tr_and.is_some_and(|s| hm.dest() == s as usize);
                        let mov_tr =
                            tr_and.is_some_and(|s| hm.is_board_move() && hm.prev_pos == s);
                        let next_tr = if cap_tr {
                            None
                        } else if mov_tr {
                            Some(hm.dest() as u8)
                        } else {
                            tr_and
                        };

                        if c < active_top && !step_certs[p_idx].is_empty() {
                            let matched_cert_idx = step_certs[p_idx]
                                .iter()
                                .position(|cert| {
                                    !cap_tr
                                        && !mov_tr
                                        && !cert.root_captured_tracked
                                        && !cert.root_moved_tracked
                                        && cert.root_ev_mv == hm
                                })
                                .or_else(|| {
                                    if cap_tr {
                                        step_certs[p_idx].iter().position(|cert| {
                                            cert.root_captured_tracked
                                                && cert.root_ev_mv.is_drop() == hm.is_drop()
                                                && (!hm.is_board_move()
                                                    || cert.root_ev_mv.prev_pos == hm.prev_pos)
                                        })
                                    } else if mov_tr {
                                        step_certs[p_idx]
                                            .iter()
                                            .position(|cert| cert.root_moved_tracked)
                                    } else if shift != (0, 0) {
                                        step_certs[p_idx].iter().position(|cert| {
                                            shift_move(cert.root_ev_mv, -shift.0, -shift.1)
                                                == Some(hm)
                                        })
                                    } else {
                                        None
                                    }
                                });

                            if let Some(ci) = matched_cert_idx {
                                let cert_clone = step_certs[p_idx][ci].clone();
                                let mut rem_fb = 2_000u64;
                                if self.replay_covariant_or_parallel::<P, SIMPLE>(
                                    &cpos,
                                    &cert_clone,
                                    0,
                                    next_tr,
                                    cth,
                                    ev_item,
                                    160,
                                    &mut rem_fb,
                                ) {
                                    if let Some(ct) = self.cycle_tracker.as_mut() {
                                        ct.stats.covariant_replays_ok += 1;
                                    }
                                    if c > 0 {
                                        if let Some(updated_cert) = self
                                            .extract_covariant_cert_parallel::<P, SIMPLE>(
                                                &cpos, hm, cap_tr, mov_tr, next_tr,
                                            )
                                        {
                                            step_certs[p_idx][ci] = updated_cert;
                                        }
                                    }
                                    continue;
                                } else if let Some(ct) = self.cycle_tracker.as_mut() {
                                    ct.stats.covariant_replays_fail += 1;
                                }
                            }
                        }

                        let stage1_budget = if c == active_top {
                            4_000u64.min(template_side_budget)
                        } else {
                            replay_miss_side_budget
                        };
                        self.probe_or_bounded_local::<P, SIMPLE>(&cpos, cth, ev_item, stage1_budget);
                        if c == active_top
                            && ev_item.tdata.pn > 0
                            && ev_item.tdata.dn > 0
                            && template_side_budget > stage1_budget
                            && !(ev_item.tdata.pn >= 9 && ev_item.tdata.dn >= 45)
                        {
                            self.probe_or_bounded_local::<P, SIMPLE>(
                                &cpos,
                                cth,
                                ev_item,
                                template_side_budget - stage1_budget,
                            );
                        }
                        if ev_item.tdata.dn == 0 {
                            refuted_cycle = true;
                            all_ev_proven = false;
                            break;
                        }
                        if ev_item.tdata.pn != 0 {
                            if ev_item.tdata.pn >= 9 && ev_item.tdata.dn >= 45 {
                                likely_escape = true;
                            }
                            all_ev_proven = false;
                            if let Some(ct) = self.cycle_tracker.as_mut() {
                                ct.stats.soft_bak_deep_deferred += 1;
                            }
                            break;
                        }
                        if c > 0 {
                            if let Some(cert) = self.extract_covariant_cert_parallel::<P, SIMPLE>(
                                &cpos, hm, cap_tr, mov_tr, next_tr,
                            ) {
                                step_certs[p_idx].push(cert);
                                if let Some(ct) = self.cycle_tracker.as_mut() {
                                    ct.stats.certs_extracted += 1;
                                }
                            }
                        }
                    }

                    if !all_ev_proven {
                        self.release_list(ev_list);
                        if refuted_cycle {
                            if let Some(ct) = self.cycle_tracker.as_mut() {
                                ct.stats.soft_bak_dn0_abort += 1;
                            }
                        }
                        if c == active_top
                            && c > 0
                            && !kind.is_hand_monotone()
                            && !self.stopping()
                            && !self.shared.abort_bn.load(Ordering::Relaxed)
                            && self.local_nodes.saturating_sub(backup_n0) < max_backup_nodes
                        {
                            let prev_bpos = &cycles[c].0;
                            let prev_ks = prev_bpos.key_state();
                            let mut prev_bitem = MvItem::empty_root();
                            self.shared.tbase.lookup(&prev_ks, &mut prev_bitem, tn);
                            if prev_bitem.tdata.pn != 0 && prev_bitem.tdata.dn != 0 {
                                let pbth = TData {
                                    pn: PROOF_MAX - 1,
                                    dn: INFINATE - 1,
                                    sh: max_d.saturating_sub(prev_bpos.count() + 1),
                                };
                                if !(P::SUPPORTS_NI
                                    && self.use_ni
                                    && self.ni_try_lookup::<P, SIMPLE>(
                                        prev_bpos,
                                        pbth,
                                        &mut prev_bitem,
                                        tn,
                                    ))
                                {
                                    let n0 = self.local_nodes;
                                    self.probe_or_bounded_local::<P, SIMPLE>(
                                        prev_bpos,
                                        pbth,
                                        &mut prev_bitem,
                                        8_000,
                                    );
                                    let used = self.local_nodes.saturating_sub(n0);
                                    if let Some(ct) = self.cycle_tracker.as_mut() {
                                        ct.stats.probe_nodes += used;
                                    }
                                }
                            }
                            if prev_bitem.tdata.pn == 0 {
                                active_top = c - 1;
                                cur_or_item = prev_bitem;
                                for v in &mut step_certs {
                                    v.clear();
                                }
                                chain_intact = true;
                                refuted_cycle = false;
                                likely_escape = false;
                                continue 'outer_bak;
                            }
                        }
                        chain_intact = false;
                        break 'outer_bak;
                    }

                    if !ev_list.is_empty() {
                        if SIMPLE {
                            sort_and_simple(&mut ev_list);
                        } else {
                            sort_and(&mut ev_list);
                        }
                        if !SIMPLE && invalid_flag && ev_list[0].tdata.pn == 0 && ev_list[0].inc() {
                            tsumi_proof(and_pos, &mut and_item, proof_flag);
                        } else {
                            and_item.tdata = TData {
                                pn: 0,
                                dn: INFINATE,
                                sh: ev_list[0].tdata.sh.saturating_add(1),
                            };
                            if SIMPLE {
                                proof_koma_and_simple(and_pos, &mut and_item, &ev_list);
                            } else if proof_flag {
                                let enemy_mkey = and_pos.mkey(and_pos.turn().flip());
                                and_item.mkey = enemy_mkey;
                                and_item.set_inc(ev_list[0].inc());
                                and_item.set_hinc(ev_list[0].inc());
                                and_item.nouse = ev_list[0].nouse.saturating_add(
                                    enemy_mkey.total().saturating_sub(ev_list[0].mkey.total()),
                                );
                                and_item.nouse2 = and_item.nouse;
                            } else {
                                proof_koma_and(and_pos, &mut and_item, &ev_list);
                            }
                        }
                        self.shared.tbase.update(&and_ks, &mut and_item, tn);
                        if P::SUPPORTS_NI && self.use_ni && !proof_flag && !invalid_flag {
                            self.ni_record_and_proof(
                                and_pos,
                                &and_item,
                                &ev_list,
                                MIN_NI_INSERT_WORK,
                            );
                        }
                    }
                    self.release_list(ev_list);
                }

                if and_item.tdata.pn == 0 {
                    let or_ks = or_pos.key_state();
                    let mut or_item = MvItem::empty_root();
                    or_item.tdata = TData {
                        pn: 0,
                        dn: INFINATE,
                        sh: and_item.tdata.sh.saturating_add(1),
                    };
                    if SIMPLE {
                        proof_koma_or_simple(&mut or_item, &and_item);
                    } else {
                        proof_koma_or(or_pos, &mut or_item, &and_item);
                    }
                    self.shared.tbase.update(&or_ks, &mut or_item, tn);
                    if P::SUPPORTS_NI && self.use_ni && or_pos.count() > 0 {
                        self.ni_record_or_proof(or_pos, &or_item, &and_item, MIN_NI_INSERT_WORK);
                    }
                    cur_or_item = or_item;
                    if let Some(ct) = self.cycle_tracker.as_mut() {
                        ct.stats.soft_bak_nodes_proven += 1;
                    }
                } else {
                    chain_intact = false;
                    break 'outer_bak;
                }
            }
        }

        if !chain_intact && !refuted_cycle && !likely_escape && (active_top + 1) * l >= 16 {
            let mut reg_cnt = 0u64;
            for pairs in &step_data[..=active_top] {
                for &(ref op, omv, _, _, _) in pairs {
                    self.shared.corridor.register(op.zkey(), op.mkey(tn), omv);
                    reg_cnt += 1;
                }
            }
            if let Some(ct) = self.cycle_tracker.as_mut() {
                ct.stats.corridor_registered += reg_cnt;
            }
        }

        let used_bak = self.local_nodes.saturating_sub(backup_n0);
        if let Some(ct) = self.cycle_tracker.as_mut() {
            ct.stats.backup_nodes += used_bak;
            let cd = if refuted_cycle || likely_escape {
                300_000
            } else {
                140_000
            };
            ct.set_pattern_cooldown(l, kind, shift, cur_ply, self.local_nodes + cd);
        }

        if chain_intact && cur_or_item.tdata.pn == 0 && cur_or_item.tdata.sh < th_tdata.sh {
            copy_proof_fields(mvlist, &cur_or_item);
            if let Some(ct) = self.cycle_tracker.as_mut() {
                ct.stats.soft_bak_full_ok += 1;
            }
            return true;
        }
        false
    }

    #[inline]
    fn bn_search_or<P: PositionBackend, const SIMPLE: bool>(
        &mut self,
        pos: &P,
        th_tdata: TData,
        mvlist: &mut MvItem,
    ) where
        H: SearchHeuristic<P>,
    {
        if self.stopping() {
            return;
        }
        let tn = pos.turn();
        let max_d = if SIMPLE {
            SIMPLE_MAX_DEPTH
        } else {
            TSUME_MAX_DEPTH
        };
        let want_ni = P::SUPPORTS_NI
            && self.use_ni
            && (!self.skip_root_ni || !self.path.entries.is_empty())
            && pos.count() > 0
            && th_tdata.pn >= 2
            && th_tdata.sh >= 2
            && pos.count() < max_d - 1;
        let want_cyc = self.is_cycle_scout
            && !self.in_cycle_probe
            && pos.count() >= 32
            && th_tdata.pn >= 2
            && th_tdata.sh >= 2
            && pos.count() < max_d - 1;

        let mut pushed_cycle_frame = false;
        if want_cyc {
            let (planes, rhash) = pos.ni_planes_and_ring_hash();
            if want_ni
                && self.ni_try_lookup_with_planes::<P, SIMPLE>(
                    pos, &planes, rhash, th_tdata, mvlist, tn,
                )
            {
                return;
            }
            if pos.count() >= DEEP_CORRIDOR_DEPTH
                && self
                    .cycle_tracker
                    .as_ref()
                    .is_some_and(|ct| ct.has_ring_ancestor(rhash))
                && self.try_cycle_skip_parallel::<P, SIMPLE>(pos, &planes, rhash, th_tdata, mvlist)
            {
                return;
            }
            if let Some(ct) = self.cycle_tracker.as_mut() {
                ct.push_or_frame(
                    pos.count(),
                    pos.zkey(),
                    rhash,
                    planes,
                    pos.mkey(Turn::Sente),
                    pos.mkey(Turn::Gote),
                );
                pushed_cycle_frame = true;
            }
        } else if want_ni && self.ni_try_lookup::<P, SIMPLE>(pos, th_tdata, mvlist, tn) {
            return;
        }

        let mut list = self.acquire_list();
        self.bn_search_or_inner::<P, SIMPLE>(pos, th_tdata, mvlist, &mut list);
        self.release_list(list);

        if pushed_cycle_frame {
            if let Some(ct) = self.cycle_tracker.as_mut() {
                ct.pop_or_frame();
            }
        }
    }

    fn bn_search_or_inner<P: PositionBackend, const SIMPLE: bool>(
        &mut self,
        pos: &P,
        th_tdata: TData,
        mvlist: &mut MvItem,
        list: &mut Vec<MvItem>,
    ) where
        H: SearchHeuristic<P>,
    {
        if self.stopping() {
            return;
        }
        let max_d = if SIMPLE {
            SIMPLE_MAX_DEPTH
        } else {
            TSUME_MAX_DEPTH
        };
        if pos.count() >= max_d - 1 {
            mvlist.tdata = TData {
                pn: INFINATE - 1,
                dn: 1,
                sh: 0,
            };
            return;
        }
        let tn = pos.turn();
        let ks = pos.key_state();
        let entry_nodes = self.local_nodes;

        if self.use_1ply
            && pos.count() > 0
            && th_tdata.pn <= 2
            && th_tdata.sh >= 2
            && self.try_1ply_mate_or::<P, SIMPLE>(pos, mvlist, tn, &ks)
        {
            return;
        }

        pos.generate_check_fast(list);
        self.bump_node();

        if list.is_empty() {
            mvlist.tdata = TData::FUDUMI;
            mvlist.mkey = disproof_koma(pos);
            self.shared.tbase.update(&ks, mvlist, tn);
            return;
        }

        if pos.count() == 0 && self.shared.symmetry {
            list.retain(|item| sq_file(item.head_move().dest()) < 5);
            if list.is_empty() {
                mvlist.tdata = TData::FUDUMI;
                mvlist.mkey = disproof_koma(pos);
                self.shared
                    .tbase
                    .update_with_path(&ks, mvlist, tn, |z, m| self.path.has_exact(z, m));
                return;
            }
        }

        let enemy_ou = pos.enemy_ou();

        let n = list.len();
        let mut ks0 = pos.key_forward_item(&list[0]);
        self.shared.tbase.prefetch(ks0.zkey);
        let mut ks1 = if n > 1 {
            let k = pos.key_forward_item(&list[1]);
            self.shared.tbase.prefetch(k.zkey);
            k
        } else {
            ks0
        };
        let mut ks2 = if n > 2 {
            let k = pos.key_forward_item(&list[2]);
            self.shared.tbase.prefetch(k.zkey);
            k
        } else {
            ks1
        };

        let mut has_mate = false;
        let mut has_fudumi = false;
        let mut all_unsearched = true;
        let mut min_pn: u16 = INFINATE;
        let mut max_dn: u16 = 0;

        for i in 0..n {
            let cur_cks = ks0;
            if i + 3 < n {
                let ks3 = pos.key_forward_item(&list[i + 3]);
                self.shared.tbase.prefetch(ks3.zkey);
                ks0 = ks1;
                ks1 = ks2;
                ks2 = ks3;
            } else {
                ks0 = ks1;
                ks1 = ks2;
            }
            if self.in_make_tree {
                self.shared.tbase.make_tree_lookup(&cur_cks, &mut list[i], tn);
            } else {
                self.shared
                    .tbase
                    .lookup_cached(&cur_cks, &mut list[i], tn, &mut self.local_tt);
            }
            self.path.apply_cycle_bounds(&cur_cks, &mut list[i], tn);

            let item = &mut list[i];
            if item.tdata.pn == 0 {
                if SIMPLE {
                    mvlist.tdata = TData {
                        pn: 0,
                        dn: INFINATE,
                        sh: item.tdata.sh.saturating_add(1),
                    };
                    proof_koma_or_simple(mvlist, item);
                    self.shared.tbase.update(&ks, mvlist, tn);
                    if P::SUPPORTS_NI && self.use_ni && pos.count() > 0 {
                        let work = self.local_nodes.saturating_sub(entry_nodes);
                        self.ni_record_or_proof(pos, mvlist, item, work);
                    }
                    return;
                }
                has_mate = true;
            }
            if item.tdata.dn == 0 {
                has_fudumi = true;
            }
            item.length = DISTANCE[enemy_ou][item.head_move().dest()];
            let base_pn0 = initial_or_pn0(enemy_ou, item);
            item.sort_key = base_pn0 as u64;
            if !item.search() {
                item.tdata.pn = item.tdata.pn.max(base_pn0);
            } else {
                all_unsearched = false;
            }
            if item.tdata.pn < min_pn {
                min_pn = item.tdata.pn;
            }
            if item.tdata.dn > max_dn {
                max_dn = item.tdata.dn;
            }
        }

        if pos.count() > 0
            && all_unsearched
            && !has_mate
            && !has_fudumi
            && !H::HAS_PN_FLOOR
            && !H::HAS_EDGE_DELTA
        {
            let dn_val = (INFINATE - 1).min(max_dn.saturating_add((n - 1) as u16));
            if dn_val >= th_tdata.dn || min_pn >= th_tdata.pn || 1 >= th_tdata.sh {
                if th_tdata.sh >= 2 && self.try_1ply_mate_or::<P, SIMPLE>(pos, mvlist, tn, &ks) {
                    return;
                }
                mvlist.tdata = TData {
                    pn: min_pn,
                    dn: dn_val,
                    sh: 1,
                };
                mvlist.set_inc(false);
                self.shared
                    .tbase
                    .update_leaf_hint(&ks, mvlist, tn, |z, m| self.path.has_exact(z, m), self.tid > 0);
                return;
            }
        }

        let gc_num = self.shared.tbase.gc_num();
        if self.shared.config.use_ml_ordering
            && (list.len() > 1 || H::HAS_EDGE_DELTA || H::HAS_PN_FLOOR)
            && !(has_mate && pos.count() >= 30)
        {
            self.heuristic
                .score_or_moves_ctx(pos, mvlist.head_move(), gc_num, list);
        }
        if SIMPLE {
            sort_or_simple(list);
        } else {
            sort_or(list);
        }

        let is_multi = self.warmup_cap.is_none();
        let mut expanded_any = false;
        let mut corridor_mv = if self.tid > 0 && !self.in_cycle_probe && !self.in_make_tree {
            self.shared.corridor.lookup(ks.zkey, ks.mkey_for(tn))
        } else {
            None
        };

        loop {
            if !SIMPLE
                && list[0].tdata.pn == 0
                && list[0].tdata.sh > 1
                && list[0].inc()
                && pos.count() < 30
            {
                let c_th = TData {
                    pn: INFINATE - 1,
                    dn: INFINATE - 1,
                    sh: 2,
                };
                for item in &mut list[1..] {
                    if !item.search() && !self.stopping() {
                        let hm = item.head_move();
                        let pushed_mv = if self.is_cycle_scout && !self.in_cycle_probe {
                            if let Some(ct) = self.cycle_tracker.as_mut() {
                                ct.push_move(hm);
                                true
                            } else {
                                false
                            }
                        } else {
                            false
                        };
                        let child_pos = pos.move_forward(hm);
                        self.bn_search_and::<P, false>(&child_pos, c_th, item);
                        if pushed_mv {
                            if let Some(ct) = self.cycle_tracker.as_mut() {
                                ct.pop_move();
                            }
                        }
                        item.set_search(true);
                    }
                }
                sort_or(list);
            }

            mvlist.tdata.pn = list[0].tdata.pn;
            if pos.count() == 0
                && self.tid == 0
                && self.shared.tbase.root_pn.load(Ordering::Relaxed) != mvlist.tdata.pn
            {
                self.shared.tbase.root_pn.store(mvlist.tdata.pn, Ordering::Relaxed);
            }
            let (dn_val, dcnt) = if pos.count() > 0 {
                disproof_number(list)
            } else {
                (list[0].tdata.dn, 0)
            };
            mvlist.tdata.dn = dn_val;
            mvlist.tdata.sh = list[0].tdata.sh.saturating_add(1);
            if !SIMPLE {
                mvlist.set_inc(list[0].inc());
            }

            if self.stopping() {
                return;
            }

            if mvlist.tdata.pn >= th_tdata.pn
                || mvlist.tdata.dn >= th_tdata.dn
                || mvlist.tdata.sh >= th_tdata.sh
            {
                if !expanded_any
                    && pos.count() > 0
                    && mvlist.tdata.pn > 0
                    && mvlist.tdata.dn > 0
                    && th_tdata.sh >= 2
                    && self.try_1ply_mate_or::<P, SIMPLE>(pos, mvlist, tn, &ks)
                {
                    return;
                }
                if mvlist.tdata.pn == 0 {
                    if SIMPLE {
                        proof_koma_or_simple(mvlist, &list[0]);
                    } else {
                        proof_koma_or(pos, mvlist, &list[0]);
                    }
                } else if mvlist.tdata.dn == 0 {
                    disproof_koma_or(pos, mvlist, list);
                }
                self.shared
                    .tbase
                    .update_with_path(&ks, mvlist, tn, |z, m| self.path.has_exact(z, m));
                if mvlist.tdata.pn == 0 && P::SUPPORTS_NI && self.use_ni && pos.count() > 0 {
                    let work = self.local_nodes.saturating_sub(entry_nodes);
                    self.ni_record_or_proof(pos, mvlist, &list[0], work);
                }
                return;
            }

            // Soft-Corridor Guidance from `SharedCorridorTable` (registered when a periodic
            // cycle's boundary `P_r` was proven `pn == 0` by a Cycle-Scout worker).
            let mut on_corridor_boost = false;
            if let Some(cmv) = corridor_mv {
                if let Some(cidx) = list.iter().position(|it| it.head_move() == cmv) {
                    if list[cidx].tdata.dn == 0 || list[cidx].cu() {
                        self.shared.corridor.invalidate(ks.zkey, ks.mkey_for(tn));
                        if let Some(ct) = self.cycle_tracker.as_mut() {
                            ct.stats.corridor_invalidated += 1;
                        }
                        corridor_mv = None;
                    } else if list[0].tdata.pn > 0
                        && list[cidx].tdata.pn < th_tdata.pn
                        && list[cidx].tdata.pn <= list[0].tdata.pn.saturating_mul(2)
                    {
                        if cidx != 0 {
                            let item = list.remove(cidx);
                            list.insert(0, item);
                        }
                        if list[0].head_move() == cmv {
                            on_corridor_boost = true;
                            if let Some(ct) = self.cycle_tracker.as_mut() {
                                ct.stats.corridor_hits += 1;
                            }
                        }
                    }
                } else {
                    corridor_mv = None;
                }
            }

            // Select best eligible child (`true_pn < th_tdata.pn && true_dn > 0`).
            // `tid == 0` acts as a pure canonical Anchor Thread (never diverted by `w`, while absorbing
            // all proofs/disproofs from helper threads `1..T` and broadcasting its own path in `InFlightTable`).
            let len = list.len();
            let deep_corridor = pos.count() >= DEEP_CORRIDOR_DEPTH;
            let (best_idx, c_threshold, child_ks) = if len == 1 {
                let cks = pos.key_forward_item(&list[0]);
                (
                    0usize,
                    TData {
                        pn: th_tdata.pn,
                        dn: th_tdata.dn,
                        sh: th_tdata.sh.saturating_sub(1),
                    },
                    cks,
                )
            } else if !is_multi || self.tid == 0 {
                let next_pn = if on_corridor_boost {
                    let cur_pn = list[0].tdata.pn;
                    let s1 = list[1].tdata.pn;
                    let boosted = s1.saturating_mul(2).max(s1.saturating_add(4));
                    boosted.max(cur_pn.saturating_add(1)).min(INFINATE)
                } else if list[0].search() {
                    let s1 = list[1].tdata.pn;
                    let eps_shift = if deep_corridor {
                        self.deep_eps_pn_shift
                    } else {
                        self.eps_pn_shift
                    };
                    let step = if eps_shift > 0 && s1 < INFINATE {
                        1u16.saturating_add(s1 >> eps_shift)
                    } else {
                        1
                    };
                    s1.saturating_add(step).min(INFINATE)
                } else if deep_corridor {
                    2
                } else {
                    1
                };
                let c_pn = th_tdata.pn.min(next_pn);
                let c_dn = if th_tdata.dn == INFINATE - 1 {
                    INFINATE - 1
                } else if pos.count() > 0 {
                    th_tdata.dn.saturating_sub(dcnt)
                } else {
                    th_tdata.dn
                };
                (
                    0usize,
                    TData {
                        pn: c_pn,
                        dn: c_dn,
                        sh: th_tdata.sh.saturating_sub(1),
                    },
                    pos.key_forward_item(&list[0]),
                )
            } else {
                let mut best_i = 0usize;
                let mut best_eff = u32::MAX;
                let mut best_cks = ks;
                let role = self.tid & 7;
                let band = (self.tid >> 3) as u32;
                let (w_cap, shift_v, deep_slack) = if band == 0 {
                    match role {
                        1 => (1u32, 4u32, 1u32),
                        2 => (1u32, 4u32, 0u32),
                        3 => (2u32, 3u32, 0u32),
                        4 => (1u32, 3u32, 0u32),
                        5 => (1u32, 4u32, 2u32),
                        6 => (1u32, 3u32, 3u32),
                        _ => (1u32, 4u32, 4u32),
                    }
                } else {
                    // `tid >= 8` (`16T`..`48T`): span `(w_cap: 1..4, shift_v: 2..4, deep_slack: 0..12)`
                    match role {
                        0 => (1 + (band & 1), 4, (5 + 2 * band).min(12)),
                        1 => (2, 4, (1 + band).min(6)),
                        2 => (2 + (band & 1), 3, 0),
                        3 => (1, 4, (4 + 2 * band).min(12)),
                        4 => (3 + (band & 1), 2 + (band & 1), 0),
                        5 => (2, 4, (2 + 2 * band).min(10)),
                        6 => (2 + (band & 1), 3, (3 + band).min(8)),
                        _ => (3 + (band & 1), 3, 1),
                    }
                };

                for (i, item) in list.iter().enumerate() {
                    if item.tdata.dn == 0 {
                        break;
                    }
                    let tpn = item.tdata.pn as u32;
                    if tpn >= th_tdata.pn as u32 {
                        break;
                    }
                    if i > 0 && tpn > (best_eff >> 5) {
                        break;
                    }
                    let cks = pos.key_forward_item(item);
                    let sidx = InFlightTable::slot_idx(cks.zkey, cks.mkey_for(tn));
                    let raw_w = self.shared.inflight.get_slot(sidx);
                    let w = if raw_w == 0 {
                        0
                    } else {
                        raw_w.saturating_sub(self.path.self_slot_count(sidx))
                    };
                    let w_eff = if deep_corridor {
                        w.saturating_sub(deep_slack)
                    } else {
                        w
                    };
                    let v_unit = 1 + (tpn >> shift_v);
                    let eff = ((tpn + w_eff.min(w_cap) * v_unit) << 5) | (i.min(31) as u32);

                    if eff < best_eff {
                        best_eff = eff;
                        best_i = i;
                        best_cks = cks;
                        if i == 0 && w_eff == 0 {
                            break;
                        }
                    }
                }
                if best_eff == u32::MAX {
                    best_cks = pos.key_forward_item(&list[0]);
                }

                let tpn_best = list[best_i].tdata.pn;
                let next_pn = if best_i == 0 && on_corridor_boost {
                    let s1 = list[1].tdata.pn;
                    let boosted = s1.saturating_mul(2).max(s1.saturating_add(4));
                    boosted.max(tpn_best.saturating_add(1)).min(INFINATE)
                } else if !list[best_i].search() {
                    if deep_corridor { 2 } else { 1 }
                } else {
                    let raw_next = if best_i + 1 < list.len() {
                        list[best_i + 1].tdata.pn
                    } else {
                        INFINATE
                    };
                    let s1 = if best_i == 0 {
                        raw_next
                    } else {
                        let p0 = list[0].tdata.pn;
                        raw_next
                            .min(tpn_best.saturating_add((tpn_best >> 3).max(2)))
                            .min(p0.saturating_add((p0 >> 1).max(6)))
                    };
                    let eps_shift = if deep_corridor {
                        self.deep_eps_pn_shift
                    } else {
                        self.eps_pn_shift
                    };
                    let step = if eps_shift > 0 && s1 < INFINATE {
                        1u16.saturating_add(s1 >> eps_shift)
                    } else {
                        1
                    };
                    s1.saturating_add(step)
                        .max(tpn_best.saturating_add(1))
                        .min(INFINATE)
                };
                let c_pn = th_tdata.pn.min(next_pn);
                let raw_c_dn = if th_tdata.dn == INFINATE - 1 {
                    INFINATE - 1
                } else if pos.count() > 0 {
                    th_tdata.dn.saturating_sub(dcnt)
                } else {
                    th_tdata.dn
                };
                let c_dn = if best_i > 0 {
                    let tdn_best = list[best_i].tdata.dn;
                    raw_c_dn.min(tdn_best.saturating_add((tdn_best >> 1).max(8)))
                } else {
                    raw_c_dn
                };
                (
                    best_i,
                    TData {
                        pn: c_pn,
                        dn: c_dn,
                        sh: th_tdata.sh.saturating_sub(1),
                    },
                    best_cks,
                )
            };

            mvlist.tdata = c_threshold;
            if c_threshold.pn < PROOF_MAX - 1 && c_threshold.pn > self.local_max_thpn {
                self.local_max_thpn = c_threshold.pn;
            }
            list[best_idx].set_search(true);
            expanded_any = true;

            let old_pn0 = list[best_idx].tdata.pn;
            let old_dn0 = list[best_idx].tdata.dn;
            let nodes_before = self.local_nodes;
            let track_if = is_multi && c_threshold.pn >= 4 && c_threshold.dn >= 4;
            let (child_sidx, path_slot) = if track_if {
                let s = InFlightTable::slot_idx(child_ks.zkey, child_ks.mkey_for(tn));
                (s, s as u32)
            } else {
                (0, u32::MAX)
            };
            let _if_guard = self.shared.inflight.enter_slot(child_sidx, track_if);

            let chosen_mv = list[best_idx].head_move();
            let pushed_mv = if self.is_cycle_scout && !self.in_cycle_probe {
                if let Some(ct) = self.cycle_tracker.as_mut() {
                    ct.push_move(chosen_mv);
                    true
                } else {
                    false
                }
            } else {
                false
            };
            self.path.push(ks, tn, mvlist.tdata.pn, path_slot);
            let child_pos = pos.move_forward(chosen_mv);
            self.bn_search_and::<P, SIMPLE>(&child_pos, c_threshold, &mut list[best_idx]);
            self.path.pop();
            if pushed_mv {
                if let Some(ct) = self.cycle_tracker.as_mut() {
                    ct.pop_move();
                }
            }
            drop(_if_guard);

            // Check if an ancestor-solved unwind landed right at this node!
            if self.unwind_target == Some(self.path.len()) {
                self.unwind_target = None;
                let mut tmp = mvlist.clone();
                self.shared.tbase.lookup(&ks, &mut tmp, tn);
                if tmp.tdata.pn == 0 || tmp.tdata.dn == 0 {
                    *mvlist = tmp;
                    return;
                }
            }
            if self.stopping() {
                return;
            }

            let sub_nodes = self.local_nodes.saturating_sub(nodes_before);
            if H::HAS_ONLINE_LEARNING && best_idx == 0 {
                self.heuristic
                    .on_or_step(pos, mvlist.head_move(), list, old_pn0, old_dn0, sub_nodes);
            }

            if SIMPLE && list[best_idx].tdata.pn == 0 {
                mvlist.tdata = TData {
                    pn: 0,
                    dn: INFINATE,
                    sh: list[best_idx].tdata.sh.saturating_add(1),
                };
                proof_koma_or_simple(mvlist, &list[best_idx]);
                self.shared.tbase.update(&ks, mvlist, tn);
                if P::SUPPORTS_NI && self.use_ni && pos.count() > 0 {
                    let work = self.local_nodes.saturating_sub(entry_nodes);
                    self.ni_record_or_proof(pos, mvlist, &list[best_idx], work);
                }
                return;
            }

            // If another thread already solved this node while we were in `best_idx`, take it safely!
            if is_multi && sub_nodes >= 16 && pos.count() > 0 && self.shared.tbase.is_terminal(&ks, tn)
            {
                let mut tmp = mvlist.clone();
                self.shared.tbase.lookup(&ks, &mut tmp, tn);
                if tmp.tdata.pn == 0 || tmp.tdata.dn == 0 {
                    *mvlist = tmp;
                    return;
                }
            }

            if is_multi && sub_nodes >= 24 && list.len() > 1 {
                for (j, sib) in list.iter_mut().enumerate() {
                    if j != best_idx && sib.tdata.pn != 0 && sib.tdata.dn != 0 {
                        let cks = pos.key_forward_item(sib);
                        let base_pn0 = initial_or_pn0(enemy_ou, sib);
                        self.shared.tbase.sync_sibling_cached(
                            &cks,
                            sib,
                            tn,
                            self.in_make_tree,
                            &mut self.local_tt,
                        );
                        self.path.apply_cycle_bounds(&cks, sib, tn);
                        if !sib.search() {
                            sib.tdata.pn = sib.tdata.pn.max(base_pn0);
                        }
                    }
                }
                if SIMPLE {
                    sort_or_simple(list);
                } else {
                    sort_or(list);
                }
            } else if best_idx == 0 {
                if SIMPLE {
                    reorder_first_or_simple(list);
                } else {
                    reorder_first_or(list);
                }
            } else if SIMPLE {
                sort_or_simple(list);
            } else if on_corridor_boost {
                sort_or(list);
            } else {
                reorder_at_or(list, best_idx);
            }
        }
    }

    #[inline]
    fn bn_search_and<P: PositionBackend, const SIMPLE: bool>(
        &mut self,
        pos: &P,
        th_tdata: TData,
        mvlist: &mut MvItem,
    ) where
        H: SearchHeuristic<P>,
    {
        let mut list = self.acquire_list();
        self.bn_search_and_inner::<P, SIMPLE>(pos, th_tdata, mvlist, &mut list);
        self.release_list(list);
    }

    fn bn_search_and_inner<P: PositionBackend, const SIMPLE: bool>(
        &mut self,
        pos: &P,
        th_tdata: TData,
        mvlist: &mut MvItem,
        list: &mut Vec<MvItem>,
    ) where
        H: SearchHeuristic<P>,
    {
        if self.stopping() {
            return;
        }
        let ryuma_flag = mvlist.length > 2;
        let tn = pos.turn().flip();
        let ks = pos.key_state();
        let entry_nodes = self.local_nodes;

        if P::SUPPORTS_NI
            && self.use_ni
            && th_tdata.pn >= 2
            && th_tdata.sh >= 2
            && self.ni_try_lookup::<P, SIMPLE>(pos, th_tdata, mvlist, tn)
        {
            return;
        }

        let (proof_flag, invalid_flag) = if SIMPLE {
            pos.generate_evasion_simple_fast(list);
            (false, false)
        } else {
            let mut oracle = WorkerOracle {
                tbase: self.shared.tbase,
                local_tt: Some(&mut self.local_tt),
                hs_depth: 0,
            };
            pos.generate_evasion_with(&mut oracle, list)
        };
        self.bump_node();

        if list.is_empty() {
            if SIMPLE {
                mvlist.tdata = TData::TSUMI;
                mvlist.mkey = proof_koma(pos);
            } else {
                tsumi_proof(pos, mvlist, proof_flag);
            }
            self.shared.tbase.update(&ks, mvlist, tn);
            if P::SUPPORTS_NI && self.use_ni && !proof_flag && !invalid_flag {
                self.ni_record_and_leaf(pos);
            }
            return;
        }

        let mut i = 0;
        let mut all_fresh = !ryuma_flag;
        let mut best_fudumi: Option<(u64, usize)> = None;
        if list.len() > 1 {
            let cks0 = pos.key_forward_item(&list[0]);
            self.shared.tbase.prefetch(cks0.zkey);
        }
        while i < list.len() {
            if i + 1 < list.len() {
                let cks_next = pos.key_forward_item(&list[i + 1]);
                self.shared.tbase.prefetch(cks_next.zkey);
            }
            let child_ks = pos.key_forward_item(&list[i]);
            if self.in_make_tree {
                self.shared.tbase.make_tree_lookup(&child_ks, &mut list[i], tn);
            } else {
                self.shared
                    .tbase
                    .lookup_cached(&child_ks, &mut list[i], tn, &mut self.local_tt);
            }
            self.path.apply_cycle_bounds(&child_ks, &mut list[i], tn);

            if list[i].tdata.pn == 0 {
                all_fresh = false;
                if let Some(tail) = list[i].take_tail() {
                    list.insert(i + 1, split_tail_item(pos, tail));
                }
            } else if list[i].search() || list[i].tdata.pn != 1 || list[i].tdata.dn != 1 {
                all_fresh = false;
            }
            if list[i].tdata.dn == 0 {
                if SIMPLE {
                    mvlist.tdata = TData {
                        pn: INFINATE,
                        dn: 0,
                        sh: list[i].tdata.sh.saturating_add(1),
                    };
                    disproof_koma_and(pos, mvlist, &list[i]);
                    self.shared.tbase.update(&ks, mvlist, tn);
                    return;
                }
                let fk = and_sort_key(&list[i]);
                if best_fudumi.is_none_or(|(bk, _)| fk < bk) {
                    best_fudumi = Some((fk, i));
                }
                all_fresh = false;
            }
            list[i].sort_key = (1u64 << 16) | 1u64;
            i += 1;
        }

        if let Some((_, f_idx)) = best_fudumi {
            mvlist.tdata = TData {
                pn: INFINATE,
                dn: 0,
                sh: list[f_idx].tdata.sh.saturating_add(1),
            };
            disproof_koma_and(pos, mvlist, &list[f_idx]);
            self.shared
                .tbase
                .update_with_path(&ks, mvlist, tn, |z, m| self.path.has_exact(z, m));
            return;
        }

        if all_fresh && !H::HAS_EDGE_DELTA && list.len() <= (PRE_PROOF_MAX as usize) {
            let pn_val = list.len() as u16;
            if pn_val >= th_tdata.pn || 1 >= th_tdata.dn || 1 >= th_tdata.sh {
                mvlist.tdata = TData {
                    pn: pn_val,
                    dn: 1,
                    sh: 1,
                };
                self.shared
                    .tbase
                    .update_leaf_hint(&ks, mvlist, tn, |z, m| self.path.has_exact(z, m), self.tid > 0);
                return;
            }
        }

        let gc_num = self.shared.tbase.gc_num();
        if self.shared.config.use_ml_ordering && (list.len() > 1 || H::HAS_EDGE_DELTA) {
            self.heuristic
                .score_and_moves_ctx(pos, mvlist.head_move(), gc_num, list);
        }
        if SIMPLE {
            sort_and_simple(list);
        } else {
            sort_and(list);
        }

        let is_multi = self.warmup_cap.is_none();

        loop {
            if ryuma_flag && list.len() > 1 {
                if list[0].head_move().is_drop() {
                    if list[0].tdata.pn != 0
                        && list[0].tdata.dn != 0
                        && list[1].tdata.pn != 0
                        && list[1].tdata.dn != 0
                        && !pos.self_effect_contains(list[0].head_move().dest())
                    {
                        let first = list.remove(0);
                        list[0].fold_moves_from(first);
                    }
                } else {
                    let mut fold_idx = None;
                    for idx in 1..list.len() {
                        if list[idx].tdata.pn == 0 {
                            break;
                        }
                        if list[idx].head_move().is_drop()
                            && !pos.self_effect_contains(list[idx].head_move().dest())
                        {
                            fold_idx = Some(idx);
                            break;
                        }
                    }
                    if let Some(idx) = fold_idx {
                        let item = list.remove(idx);
                        list[0].fold_moves_from(item);
                    }
                }
            }

            if !SIMPLE && invalid_flag && list[0].tdata.pn == 0 && list[0].inc() {
                tsumi_proof(pos, mvlist, proof_flag);
                self.shared.tbase.update(&ks, mvlist, tn);
                return;
            }

            let pcnt;
            loop {
                mvlist.tdata.dn = list[0].tdata.dn;
                let (pn_val, p) = proof_number(list);
                mvlist.tdata.pn = pn_val;
                mvlist.tdata.sh = list[0].tdata.sh.saturating_add(1);

                if list[0].tdata.pn >= PRE_PROOF_MAX - 1 && list[0].tdata.dn == 1 {
                    mvlist.tdata.dn = 0;
                    mvlist.tdata.pn = INFINATE;
                    pcnt = p;
                    break;
                } else if mvlist.tdata.pn > PRE_PROOF_MAX
                    && mvlist.tdata.dn != 0
                    && list.len() > 1
                    && list[1].tdata.pn != 0
                    && list[0].cap_koma == 0
                {
                    let mut max_idx = 0;
                    for idx in 1..list.len() {
                        if list[idx].tdata.pn > list[max_idx].tdata.pn {
                            max_idx = idx;
                        }
                    }
                    let max_item = list.remove(max_idx);
                    if SIMPLE {
                        sort_and_simple(list);
                    } else {
                        sort_and(list);
                    }
                    list[0].fold_moves_from(max_item);
                } else if list[0].tdata.pn != 0
                    && list[0].tdata.dn > PRE_DISPROOF_MAX
                    && list.len() > 1
                    && list[1].tdata.pn != 0
                    && list[1].tdata.dn != 0
                {
                    let second = list.remove(1);
                    list[0].fold_moves_from(second);
                } else {
                    pcnt = p;
                    break;
                }
            }

            if self.stopping() {
                return;
            }

            if mvlist.tdata.pn >= th_tdata.pn
                || mvlist.tdata.dn >= th_tdata.dn
                || mvlist.tdata.sh >= th_tdata.sh
            {
                if mvlist.tdata.dn == 0 {
                    disproof_koma_and(pos, mvlist, &list[0]);
                } else if mvlist.tdata.pn == 0 {
                    if SIMPLE {
                        proof_koma_and_simple(pos, mvlist, list);
                    } else if proof_flag {
                        let enemy_mkey = pos.mkey(pos.turn().flip());
                        mvlist.mkey = enemy_mkey;
                        mvlist.set_inc(list[0].inc());
                        mvlist.set_hinc(list[0].inc());
                        mvlist.nouse = list[0]
                            .nouse
                            .saturating_add(enemy_mkey.total().saturating_sub(list[0].mkey.total()));
                        mvlist.nouse2 = mvlist.nouse;
                    } else {
                        proof_koma_and(pos, mvlist, list);
                    }
                }
                self.shared
                    .tbase
                    .update_with_path(&ks, mvlist, tn, |z, m| self.path.has_exact(z, m));
                if mvlist.tdata.pn == 0
                    && P::SUPPORTS_NI
                    && self.use_ni
                    && !proof_flag
                    && !invalid_flag
                {
                    let work = self.local_nodes.saturating_sub(entry_nodes);
                    self.ni_record_and_proof(pos, mvlist, list, work);
                }
                return;
            }

            let len = list.len();
            let edn = if gc_num < 2 { self.eps_dn_shift } else { 0 };
            let (best_idx, c_threshold, child_ks) = if len == 1 {
                let cks = pos.key_forward_item(&list[0]);
                (
                    0usize,
                    TData {
                        pn: th_tdata.pn,
                        dn: th_tdata.dn,
                        sh: th_tdata.sh.saturating_sub(1),
                    },
                    cks,
                )
            } else if !is_multi || self.tid == 0 {
                let s1_dn = list[1].tdata.dn;
                let dn_step = if edn > 0 {
                    1u16.saturating_add(s1_dn >> edn)
                } else {
                    1
                };
                (
                    0usize,
                    TData {
                        dn: th_tdata.dn.min(s1_dn.saturating_add(dn_step)),
                        pn: th_tdata.pn.saturating_sub(pcnt),
                        sh: th_tdata.sh.saturating_sub(1),
                    },
                    pos.key_forward_item(&list[0]),
                )
            } else {
                let mut best_i = 0usize;
                let mut best_eff = u32::MAX;
                let mut best_cks = ks;
                let role = self.tid & 7;
                let band = (self.tid >> 3) as u32;
                let w_and_cap = if band == 0 { 4 } else { (4 + band).min(8) };

                for (i, item) in list.iter().enumerate() {
                    if item.tdata.pn == 0 {
                        break;
                    }
                    let tdn = item.tdata.dn as u32;
                    if tdn >= th_tdata.dn as u32 {
                        break;
                    }
                    if i > 0 && tdn > (best_eff >> 5) {
                        break;
                    }
                    let cks = pos.key_forward_item(item);
                    let sidx = InFlightTable::slot_idx(cks.zkey, cks.mkey_for(tn));
                    let raw_w = self.shared.inflight.get_slot(sidx);
                    let w = if raw_w == 0 {
                        0
                    } else {
                        raw_w.saturating_sub(self.path.self_slot_count(sidx))
                    };
                    let v_unit = if band == 0 {
                        match role {
                            1 | 4 => 2 + (tdn >> 2),
                            2 | 5 => 2 + (tdn >> 1),
                            _ => 3 + (tdn >> 1),
                        }
                    } else {
                        match role {
                            0 | 3 => 1 + (tdn >> 2),
                            1 | 5 => 2 + (tdn >> 2),
                            2 | 6 => 2 + (tdn >> 1),
                            _ => 3 + (tdn >> 1) + band.min(2),
                        }
                    };
                    let eff = ((tdn + w.min(w_and_cap) * v_unit) << 5) | (i.min(31) as u32);

                    if eff < best_eff {
                        best_eff = eff;
                        best_i = i;
                        best_cks = cks;
                        if i == 0 && w == 0 {
                            break;
                        }
                    }
                }
                if best_eff == u32::MAX {
                    best_cks = pos.key_forward_item(&list[0]);
                }

                let tdn_best = list[best_i].tdata.dn;
                let raw_next = if best_i + 1 < list.len() {
                    list[best_i + 1].tdata.dn
                } else {
                    INFINATE
                };
                let s1_dn = if best_i == 0 {
                    raw_next
                } else {
                    raw_next.min(tdn_best.saturating_add((tdn_best >> 1).max(3)))
                };
                let dn_step = if edn > 0 && s1_dn < INFINATE {
                    1u16.saturating_add(s1_dn >> edn)
                } else {
                    1
                };
                let target_dn = s1_dn
                    .saturating_add(dn_step)
                    .max(tdn_best.saturating_add(1));
                (
                    best_i,
                    TData {
                        dn: th_tdata.dn.min(target_dn),
                        pn: th_tdata.pn.saturating_sub(pcnt),
                        sh: th_tdata.sh.saturating_sub(1),
                    },
                    best_cks,
                )
            };

            mvlist.tdata = c_threshold;
            let old_pn0 = list[best_idx].tdata.pn;
            let old_dn0 = list[best_idx].tdata.dn;
            let nodes_before = self.local_nodes;
            let track_if = is_multi && c_threshold.pn >= 4 && c_threshold.dn >= 4;
            let (child_sidx, path_slot) = if track_if {
                let s = InFlightTable::slot_idx(child_ks.zkey, child_ks.mkey_for(tn));
                (s, s as u32)
            } else {
                (0, u32::MAX)
            };
            let _if_guard = self.shared.inflight.enter_slot(child_sidx, track_if);

            let chosen_mv = list[best_idx].head_move();
            let pushed_mv = if self.is_cycle_scout && !self.in_cycle_probe {
                if let Some(ct) = self.cycle_tracker.as_mut() {
                    ct.push_move(chosen_mv);
                    true
                } else {
                    false
                }
            } else {
                false
            };
            self.path.push(ks, tn, mvlist.tdata.pn, path_slot);
            let child_pos = pos.move_forward(chosen_mv);
            self.bn_search_or::<P, SIMPLE>(&child_pos, c_threshold, &mut list[best_idx]);
            self.path.pop();
            if pushed_mv {
                if let Some(ct) = self.cycle_tracker.as_mut() {
                    ct.pop_move();
                }
            }
            drop(_if_guard);

            if self.unwind_target == Some(self.path.len()) {
                self.unwind_target = None;
                let mut tmp = mvlist.clone();
                self.shared.tbase.lookup(&ks, &mut tmp, tn);
                if tmp.tdata.pn == 0 || tmp.tdata.dn == 0 {
                    *mvlist = tmp;
                    return;
                }
            }
            if self.stopping() {
                return;
            }

            let sub_nodes = self.local_nodes.saturating_sub(nodes_before);
            if H::HAS_ONLINE_LEARNING && best_idx == 0 {
                self.heuristic
                    .on_and_step(pos, mvlist.head_move(), list, old_pn0, old_dn0, sub_nodes);
            }

            if SIMPLE && list[best_idx].tdata.dn == 0 {
                mvlist.tdata = TData {
                    pn: INFINATE,
                    dn: 0,
                    sh: list[best_idx].tdata.sh.saturating_add(1),
                };
                disproof_koma_and(pos, mvlist, &list[best_idx]);
                self.shared.tbase.update(&ks, mvlist, tn);
                return;
            }

            if is_multi && sub_nodes >= 16 && self.shared.tbase.is_terminal(&ks, tn) {
                let mut tmp = mvlist.clone();
                self.shared.tbase.lookup(&ks, &mut tmp, tn);
                if tmp.tdata.pn == 0 || tmp.tdata.dn == 0 {
                    *mvlist = tmp;
                    return;
                }
            }

            // Expand any newly proven folded interpositions on `best_idx`
            let mut n_inserted = 0usize;
            while list[best_idx].tdata.pn == 0 && list[best_idx].has_tail() {
                let tail = list[best_idx].take_tail().unwrap();
                let mut next_item = split_tail_item(pos, tail);
                let cks = pos.key_forward_item(&next_item);
                if self.in_make_tree {
                    self.shared.tbase.make_tree_lookup(&cks, &mut next_item, tn);
                } else {
                    self.shared
                        .tbase
                        .lookup_cached(&cks, &mut next_item, tn, &mut self.local_tt);
                }
                self.path.apply_cycle_bounds(&cks, &mut next_item, tn);
                list.insert(best_idx, next_item);
                n_inserted += 1;
            }

            if is_multi && sub_nodes >= 24 && list.len() > 1 {
                let mut j = 0usize;
                while j < list.len() {
                    if (j < best_idx || j > best_idx + n_inserted)
                        && list[j].tdata.pn != 0
                        && list[j].tdata.dn != 0
                    {
                        let cks = pos.key_forward_item(&list[j]);
                        self.shared.tbase.sync_sibling_cached(
                            &cks,
                            &mut list[j],
                            tn,
                            self.in_make_tree,
                            &mut self.local_tt,
                        );
                        self.path.apply_cycle_bounds(&cks, &mut list[j], tn);
                        if list[j].tdata.pn == 0 {
                            if let Some(tail) = list[j].take_tail() {
                                list.insert(j + 1, split_tail_item(pos, tail));
                            }
                        }
                    }
                    j += 1;
                }
                if self.shared.config.use_ml_ordering && n_inserted > 0 {
                    let end = (best_idx + n_inserted).min(list.len());
                    self.heuristic.score_and_moves_ctx(
                        pos,
                        mvlist.head_move(),
                        gc_num,
                        &mut list[best_idx..end],
                    );
                }
                if SIMPLE {
                    sort_and_simple(list);
                } else {
                    sort_and(list);
                }
            } else if n_inserted > 0 {
                if self.shared.config.use_ml_ordering {
                    let end = (best_idx + n_inserted).min(list.len());
                    self.heuristic.score_and_moves_ctx(
                        pos,
                        mvlist.head_move(),
                        gc_num,
                        &mut list[best_idx..end],
                    );
                }
                if SIMPLE {
                    sort_and_simple(list);
                } else {
                    sort_and(list);
                }
            } else if best_idx == 0 {
                if SIMPLE {
                    reorder_first_and_simple(list);
                } else {
                    reorder_first_and(list);
                }
            } else if SIMPLE {
                sort_and_simple(list);
            } else {
                reorder_at_and(list, best_idx);
            }
        }
    }
}

struct PostTreeBuilder<'a, H, F> {
    config: &'a SearchConfig,
    tbase: &'a SharedTBase,
    ni_cache: &'a SharedNiCache,
    inflight: &'a InFlightTable,
    corridor: &'a SharedCorridorTable,
    pool: &'a rayon::ThreadPool,
    scratches: &'a ScratchPool,
    solve_epoch: u64,
    make_heuristic: &'a F,
    start: Instant,
    symmetry: bool,
    nodes: u64,
    suspend: bool,
    error: bool,
    redundant: bool,
    skip_sh_capped_probes: bool,
    add_thpn: u16,
    mt_stack: &'a mut std::collections::HashMap<(u64, MKey), u16>,
    simple_dag: &'a mut SimpleDag,
    mv_pool: &'a mut Vec<Vec<MvItem>>,
    probe_raw: Option<RawWorkerScratch>,
    probe_heuristic: Option<H>,
    external_abort: Option<Arc<AtomicBool>>,
    progress_cb: Option<Arc<dyn Fn(u64) + Send + Sync>>,
}

impl<'a, H, F> PostTreeBuilder<'a, H, F> {
    #[inline(always)]
    fn acquire_list(&mut self) -> Vec<MvItem> {
        let mut v = self.mv_pool.pop().unwrap_or_else(|| Vec::with_capacity(64));
        v.clear();
        v
    }

    #[inline(always)]
    fn release_list(&mut self, v: Vec<MvItem>) {
        self.mv_pool.push(v);
    }

    #[inline]
    fn check_limits(&mut self) {
        if self.suspend {
            return;
        }
        if let Some(limit) = self.config.node_limit {
            if self.nodes >= limit {
                self.suspend = true;
                return;
            }
        }
        if let Some(secs) = self.config.time_limit_secs {
            if (self.nodes & 4095) == 0 && self.start.elapsed().as_secs_f64() >= secs {
                self.suspend = true;
                return;
            }
        }
        if (self.nodes & 4095) == 0 {
            if let Some(flag) = &self.external_abort {
                if flag.load(Ordering::Relaxed) {
                    self.suspend = true;
                    return;
                }
            }
            if (self.nodes & 65535) == 0 {
                if let Some(cb) = &self.progress_cb {
                    cb(self.nodes);
                }
            }
        }
    }

    fn probe_and_bounded<P: PositionBackend, const SIMPLE: bool>(
        &mut self,
        child_pos: &P,
        thdata: TData,
        item: &mut MvItem,
        max_nodes: u64,
    ) where
        H: SearchHeuristic<P> + Clone + Send + Sync,
        F: Fn(usize) -> H + Sync,
    {
        if self.suspend {
            return;
        }
        let orig_item = item.clone();
        let cap = match self.config.node_limit {
            Some(nl) => max_nodes.min(nl.saturating_sub(self.nodes).max(1)),
            None => max_nodes,
        };
        let shared = SharedRunState::new(
            self.config,
            self.tbase,
            self.ni_cache,
            self.inflight,
            self.corridor,
            self.pool.current_num_threads(),
            self.start,
            false,
            self.nodes,
        );
        let raw = self.probe_raw.take().unwrap();
        let h = self.probe_heuristic.take().unwrap();
        let mut w = WorkerCtx::from_raw::<P>(0, &shared, h, raw, Some(cap), true);
        w.bn_search_and::<P, SIMPLE>(child_pos, thdata, item);
        let n = w.local_nodes;
        let ab = w.abort;
        let (raw_back, h_back) = w.into_raw();
        self.probe_raw = Some(raw_back);
        self.probe_heuristic = Some(h_back);
        self.nodes += n;
        if ab {
            *item = orig_item;
            let cks = child_pos.key_state();
            self.tbase
                .make_tree_lookup(&cks, item, child_pos.turn().flip());
        }
        if shared.suspended.load(Ordering::Relaxed) {
            self.suspend = true;
        }
        if let Some(nl) = self.config.node_limit {
            if self.nodes >= nl {
                self.suspend = true;
            }
        }
    }

    fn probe_and_stage2<P: PositionBackend, const SIMPLE: bool>(
        &mut self,
        child_pos: &P,
        thdata: TData,
        item: &mut MvItem,
    ) where
        H: SearchHeuristic<P> + Clone + Send + Sync,
        F: Fn(usize) -> H + Sync,
    {
        let shared = SharedRunState::new(
            self.config,
            self.tbase,
            self.ni_cache,
            self.inflight,
            self.corridor,
            self.pool.current_num_threads(),
            self.start,
            false,
            self.nodes,
        );
        let fallback_t0: Mutex<Option<MvItem>> = Mutex::new(None);
        let base_item = item.clone();
        let make_h = self.make_heuristic;
        let scratches = self.scratches;
        let epoch = self.solve_epoch;
        self.pool.broadcast(|bctx| {
            let tid = bctx.index();
            let raw = scratches.checkout(tid, epoch);
            let mut h = make_h(tid);
            if H::HAS_ONLINE_LEARNING {
                h.reset_for_root(child_pos);
            }
            let mut w = WorkerCtx::from_raw::<P>(tid, &shared, h, raw, None, true);
            let mut local_item = base_item.clone();
            w.bn_search_and::<P, SIMPLE>(child_pos, thdata, &mut local_item);
            w.flush_stats();

            let sh_hit = local_item.tdata.sh >= thdata.sh && thdata.pn < INFINATE - 1;
            if !w.abort && w.unwind_target.is_none() {
                if local_item.tdata.pn == 0
                    || local_item.tdata.dn == 0
                    || local_item.tdata.pn >= thdata.pn
                    || local_item.tdata.dn >= thdata.dn
                    || sh_hit
                {
                    let mut guard = shared.winner_root.lock().unwrap();
                    if guard.is_none() {
                        *guard = Some(local_item);
                    }
                    shared.abort_bn.store(true, Ordering::Release);
                } else if tid == 0 {
                    *fallback_t0.lock().unwrap() = Some(local_item);
                }
            }
            let (raw_back, _) = w.into_raw();
            scratches.checkin(tid, raw_back);
        });
        self.nodes = shared.global_nodes.load(Ordering::Relaxed);
        if shared.suspended.load(Ordering::Relaxed) {
            self.suspend = true;
        }
        if let Some(won) = shared
            .winner_root
            .into_inner()
            .unwrap()
            .or_else(|| fallback_t0.into_inner().unwrap())
        {
            *item = won;
        }
    }

    /// Two-stage `make_tree` / `bns_plus` AND probe:
    /// 1. Zero-allocation single-thread fast path (`<= 256` nodes) reusing `probe_raw`.
    /// 2. Automatic `pool.broadcast` escalation across all `N` threads if the subtree exceeds `256` nodes.
    fn probe_and<P: PositionBackend, const SIMPLE: bool>(
        &mut self,
        child_pos: &P,
        thdata: TData,
        item: &mut MvItem,
    ) where
        H: SearchHeuristic<P> + Clone + Send + Sync,
        F: Fn(usize) -> H + Sync,
    {
        if self.suspend {
            return;
        }
        let orig_item = item.clone();
        let warmup_cap = match self.config.node_limit {
            Some(nl) => PROBE_WARMUP_NODES.min(nl.saturating_sub(self.nodes).max(1)),
            None => PROBE_WARMUP_NODES,
        };
        let stage1_aborted = {
            let shared = SharedRunState::new(
                self.config,
                self.tbase,
                self.ni_cache,
                self.inflight,
                self.corridor,
                self.pool.current_num_threads(),
                self.start,
                false,
                self.nodes,
            );
            let raw = self.probe_raw.take().unwrap();
            let h = self.probe_heuristic.take().unwrap();
            let mut w = WorkerCtx::from_raw::<P>(0, &shared, h, raw, Some(warmup_cap), true);
            w.bn_search_and::<P, SIMPLE>(child_pos, thdata, item);
            let n = w.local_nodes;
            let ab = w.abort;
            let (raw_back, h_back) = w.into_raw();
            self.probe_raw = Some(raw_back);
            self.probe_heuristic = Some(h_back);
            self.nodes += n;
            if shared.suspended.load(Ordering::Relaxed) {
                self.suspend = true;
            }
            if let Some(nl) = self.config.node_limit {
                if self.nodes >= nl {
                    self.suspend = true;
                }
            }
            ab
        };

        if !stage1_aborted {
            return;
        }
        *item = orig_item;
        if self.suspend {
            return;
        }

        self.probe_and_stage2::<P, SIMPLE>(child_pos, thdata, item);
    }

    #[inline]
    fn probe_or<P: PositionBackend, const SIMPLE: bool>(
        &mut self,
        child_pos: &P,
        thdata: TData,
        item: &mut MvItem,
    ) where
        H: SearchHeuristic<P> + Clone + Send + Sync,
        F: Fn(usize) -> H + Sync,
    {
        self.probe_or_inner::<P, SIMPLE>(child_pos, thdata, item, false);
    }

    /// Two-stage `make_tree` / `bns_plus` OR probe:
    /// 1. Zero-allocation single-thread fast path (`<= PROBE_WARMUP_NODES`) reusing `probe_raw`.
    /// 2. Automatic `pool.broadcast` escalation across all `N` threads if the subtree exceeds `PROBE_WARMUP_NODES`.
    fn probe_or_inner<P: PositionBackend, const SIMPLE: bool>(
        &mut self,
        child_pos: &P,
        thdata: TData,
        item: &mut MvItem,
        skip_root_ni: bool,
    ) where
        H: SearchHeuristic<P> + Clone + Send + Sync,
        F: Fn(usize) -> H + Sync,
    {
        if self.suspend {
            return;
        }
        let orig_item = item.clone();
        let warmup_cap = match self.config.node_limit {
            Some(nl) => PROBE_WARMUP_NODES.min(nl.saturating_sub(self.nodes).max(1)),
            None => PROBE_WARMUP_NODES,
        };
        let stage1_aborted = {
            let shared = SharedRunState::new(
                self.config,
                self.tbase,
                self.ni_cache,
                self.inflight,
                self.corridor,
                self.pool.current_num_threads(),
                self.start,
                false,
                self.nodes,
            );
            let raw = self.probe_raw.take().unwrap();
            let h = self.probe_heuristic.take().unwrap();
            let mut w = WorkerCtx::from_raw::<P>(0, &shared, h, raw, Some(warmup_cap), true);
            w.skip_root_ni = skip_root_ni;
            w.bn_search_or::<P, SIMPLE>(child_pos, thdata, item);
            let n = w.local_nodes;
            let ab = w.abort;
            let (raw_back, h_back) = w.into_raw();
            self.probe_raw = Some(raw_back);
            self.probe_heuristic = Some(h_back);
            self.nodes += n;
            if shared.suspended.load(Ordering::Relaxed) {
                self.suspend = true;
            }
            if let Some(nl) = self.config.node_limit {
                if self.nodes >= nl {
                    self.suspend = true;
                }
            }
            ab
        };

        if !stage1_aborted {
            return;
        }
        *item = orig_item;
        if self.suspend {
            return;
        }

        // Stage 2: Heavy subtree (>PROBE_WARMUP_NODES) — broadcast across all worker threads!
        let shared = SharedRunState::new(
            self.config,
            self.tbase,
            self.ni_cache,
            self.inflight,
            self.corridor,
            self.pool.current_num_threads(),
            self.start,
            false,
            self.nodes,
        );
        let fallback_t0: Mutex<Option<MvItem>> = Mutex::new(None);
        let base_item = item.clone();
        let make_h = self.make_heuristic;
        let scratches = self.scratches;
        let epoch = self.solve_epoch;
        self.pool.broadcast(|bctx| {
            let tid = bctx.index();
            let raw = scratches.checkout(tid, epoch);
            let mut h = make_h(tid);
            if H::HAS_ONLINE_LEARNING {
                h.reset_for_root(child_pos);
            }
            let mut w = WorkerCtx::from_raw::<P>(tid, &shared, h, raw, None, true);
            w.skip_root_ni = skip_root_ni;
            let mut local_item = base_item.clone();
            w.bn_search_or::<P, SIMPLE>(child_pos, thdata, &mut local_item);
            w.flush_stats();

            let sh_hit = local_item.tdata.sh >= thdata.sh && thdata.pn < INFINATE - 1;
            if !w.abort && w.unwind_target.is_none() {
                if local_item.tdata.pn == 0
                    || local_item.tdata.dn == 0
                    || local_item.tdata.pn >= thdata.pn
                    || local_item.tdata.dn >= thdata.dn
                    || sh_hit
                {
                    let mut guard = shared.winner_root.lock().unwrap();
                    if guard.is_none() {
                        *guard = Some(local_item);
                    }
                    shared.abort_bn.store(true, Ordering::Release);
                } else if tid == 0 {
                    *fallback_t0.lock().unwrap() = Some(local_item);
                }
            }
            let (raw_back, _) = w.into_raw();
            scratches.checkin(tid, raw_back);
        });
        self.nodes = shared.global_nodes.load(Ordering::Relaxed);
        if shared.suspended.load(Ordering::Relaxed) {
            self.suspend = true;
        }
        if let Some(won) = shared
            .winner_root
            .into_inner()
            .unwrap()
            .or_else(|| fallback_t0.into_inner().unwrap())
        {
            *item = won;
        }
    }

    /// Single-broadcast Spine-First Work-Stealing pre-warmer for `make_tree`:
    /// Unrolls the proof DAG in `< 2 ms` onto a LIFO stack and distributes all `to_probe`
    /// sibling-check searches and `SharedNiCache` subtree materializations across all `N` cores
    /// in a single `pool.broadcast`, turning Pass 0 of `make_tree` into an in-cache walk.
    fn parallel_warm_proof_dag<P: PositionBackend>(&mut self, root: &P)
    where
        H: SearchHeuristic<P> + Clone + Send + Sync,
        F: Fn(usize) -> H + Sync,
    {
        struct WarmQueue<P> {
            stack: Vec<P>,
            visited: HashSet<(u64, MKey)>,
            active: usize,
        }

        let root_tn = root.turn();
        let mut init_vis = HashSet::with_capacity(2048);
        init_vis.insert((root.zkey(), root.mkey(root_tn)));
        let mut init_stack = Vec::with_capacity(256);
        init_stack.push(root.clone());

        let warm_mu = Mutex::new(WarmQueue {
            stack: init_stack,
            visited: init_vis,
            active: 0,
        });
        let warm_cv = Condvar::new();

        let shared = SharedRunState::new(
            self.config,
            self.tbase,
            self.ni_cache,
            self.inflight,
            self.corridor,
            self.pool.current_num_threads(),
            self.start,
            false,
            self.nodes,
        );
        let th_probe_pn =
            (self.tbase.gc_max_level() + MAKE_TREE_PN_PLUS).max(self.config.mt_min_pn);
        let mt_min_pn = self.config.mt_min_pn;
        let tbase = self.tbase;
        let make_h = self.make_heuristic;
        let scratches = self.scratches;
        let epoch = self.solve_epoch;

        self.pool.broadcast(|bctx| {
            let slot_id = bctx.index();
            let raw = scratches.checkout(slot_id, epoch);
            let mut h = make_h(slot_id);
            if H::HAS_ONLINE_LEARNING {
                h.reset_for_root(root);
            }
            // Use `tid = 0` canonical df-pn per popped OR node (`warmup_cap = Some(u64::MAX)` => `is_multi == false`).
            let mut w = WorkerCtx::from_raw::<P>(0, &shared, h, raw, Some(u64::MAX), true);
            let mut to_probe: smallvec::SmallVec<[usize; 32]> = smallvec::SmallVec::new();
            let mut new_children: smallvec::SmallVec<[P; 8]> = smallvec::SmallVec::new();

            loop {
                let pos = {
                    let mut st = warm_mu.lock().unwrap();
                    loop {
                        if shared.abort_bn.load(Ordering::Relaxed) {
                            drop(st);
                            w.flush_stats();
                            let (raw_back, _) = w.into_raw();
                            scratches.checkin(slot_id, raw_back);
                            return;
                        }
                        if let Some(p) = st.stack.pop() {
                            st.active += 1;
                            break p;
                        }
                        if st.active == 0 {
                            warm_cv.notify_all();
                            drop(st);
                            w.flush_stats();
                            let (raw_back, _) = w.into_raw();
                            scratches.checkin(slot_id, raw_back);
                            return;
                        }
                        st = warm_cv.wait(st).unwrap();
                    }
                };

                if pos.count() < TSUME_MAX_DEPTH - 1
                    && !w.abort
                    && !shared.abort_bn.load(Ordering::Relaxed)
                {
                    let tn = pos.turn();
                    let mut list = w.acquire_list();
                    pos.generate_check_fast(&mut list);
                    w.bump_node();

                    if !list.is_empty() && !w.abort {
                        let mut min_proven_sh = u16::MAX;
                        let mut best_proven_idx: Option<usize> = None;
                        let mut any_proven = false;
                        to_probe.clear();

                        for (i, item) in list.iter_mut().enumerate() {
                            let child_ks = pos.key_forward_item(item);
                            tbase.make_tree_lookup(&child_ks, item, tn);
                            if item.tdata.pn == 0 {
                                any_proven = true;
                                if !item.inc() && item.tdata.sh < min_proven_sh {
                                    min_proven_sh = item.tdata.sh;
                                    best_proven_idx = Some(i);
                                }
                            }
                            if !item.search() || (item.tdata.pn != 0 && item.tdata.pn < mt_min_pn) {
                                to_probe.push(i);
                            }
                        }

                        // Materialize winning check if `pos` was closed via `SharedNiCache`.
                        if !any_proven && !w.abort {
                            let mut or_item = MvItem::empty_root();
                            let or_th = TData {
                                pn: INFINATE - 1,
                                dn: INFINATE - 1,
                                sh: TSUME_MAX_DEPTH.saturating_sub(pos.count()),
                            };
                            w.skip_root_ni = true;
                            w.bn_search_or::<P, false>(&pos, or_th, &mut or_item);
                            w.skip_root_ni = false;

                            min_proven_sh = u16::MAX;
                            best_proven_idx = None;
                            to_probe.clear();
                            for (i, item) in list.iter_mut().enumerate() {
                                let child_ks = pos.key_forward_item(item);
                                tbase.make_tree_lookup(&child_ks, item, tn);
                                if item.tdata.pn == 0 && !item.inc() && item.tdata.sh < min_proven_sh
                                {
                                    min_proven_sh = item.tdata.sh;
                                    best_proven_idx = Some(i);
                                }
                                if !item.search()
                                    || (item.tdata.pn != 0 && item.tdata.pn < mt_min_pn)
                                {
                                    to_probe.push(i);
                                }
                            }
                        }

                        // Spine-first unroll: push child evasion OR nodes BEFORE running `to_probe`
                        // so all worker threads immediately saturate across the entire corridor!
                        if let Some(b_idx) = best_proven_idx {
                            if pos.count() + 2 < TSUME_MAX_DEPTH && !w.abort {
                                let sh_cap = min_proven_sh.saturating_add(8);
                                let mut cand_checks: smallvec::SmallVec<[usize; 4]> =
                                    smallvec::SmallVec::new();
                                for (ci, citem) in list.iter().enumerate() {
                                    if ci != b_idx
                                        && citem.tdata.pn == 0
                                        && !citem.inc()
                                        && citem.tdata.sh <= sh_cap
                                    {
                                        cand_checks.push(ci);
                                    }
                                }
                                // Push `b_idx` last so its longest spine evasion sits at top of stack!
                                cand_checks.push(b_idx);

                                new_children.clear();
                                for &c_idx in &cand_checks {
                                    if w.abort || shared.abort_bn.load(Ordering::Relaxed) {
                                        break;
                                    }
                                    let and_pos = pos.move_forward(list[c_idx].head_move());
                                    let mut ev_list = w.acquire_list();
                                    {
                                        let mut oracle = WorkerOracle {
                                            tbase,
                                            local_tt: Some(&mut w.local_tt),
                                            hs_depth: 0,
                                        };
                                        let _ =
                                            and_pos.generate_evasion_with(&mut oracle, &mut ev_list);
                                    }
                                    w.bump_node();

                                    let mut ei = 0;
                                    while ei < ev_list.len() {
                                        let cks = and_pos.key_forward_item(&ev_list[ei]);
                                        tbase.make_tree_lookup(&cks, &mut ev_list[ei], tn);
                                        if let Some(tail) = ev_list[ei].take_tail() {
                                            ev_list.insert(ei + 1, split_tail_item(&and_pos, tail));
                                        }
                                        ei += 1;
                                    }
                                    sort_and(&mut ev_list);

                                    let ev_th = TData {
                                        pn: INFINATE - 1,
                                        dn: INFINATE - 1,
                                        sh: TSUME_MAX_DEPTH.saturating_sub(and_pos.count() + 1),
                                    };
                                    while !ev_list.is_empty()
                                        && ev_list[0].tdata.pn != 0
                                        && !w.abort
                                        && !shared.abort_bn.load(Ordering::Relaxed)
                                    {
                                        let cpos = and_pos.move_forward(ev_list[0].head_move());
                                        w.bn_search_or::<P, false>(&cpos, ev_th, &mut ev_list[0]);
                                        if ev_list[0].tdata.dn == 0
                                            || ev_list[0].tdata.pn >= INFINATE - 1
                                        {
                                            break;
                                        }
                                        reorder_first_and(&mut ev_list);
                                    }

                                    for ev_item in ev_list.iter().rev() {
                                        if ev_item.tdata.pn == 0 && !ev_item.inc() {
                                            new_children
                                                .push(and_pos.move_forward(ev_item.head_move()));
                                        }
                                    }
                                    w.release_list(ev_list);
                                }

                                if !new_children.is_empty() {
                                    let mut st = warm_mu.lock().unwrap();
                                    let mut pushed = 0usize;
                                    for cpos in new_children.drain(..) {
                                        let key = (cpos.zkey(), cpos.mkey(root_tn));
                                        if st.visited.insert(key) {
                                            st.stack.push(cpos);
                                            pushed += 1;
                                        }
                                    }
                                    if pushed > 0 {
                                        if pushed == 1 {
                                            warm_cv.notify_one();
                                        } else {
                                            warm_cv.notify_all();
                                        }
                                    }
                                }
                            }
                        }

                        // Now execute `to_probe` on `pos` in parallel with peers!
                        let mut th_probe_sh = TSUME_MAX_DEPTH.saturating_sub(pos.count() + 1);
                        if min_proven_sh < th_probe_sh {
                            th_probe_sh = min_proven_sh;
                        }
                        if min_proven_sh > 0 && !to_probe.is_empty() && !w.abort {
                            let thdata = TData {
                                pn: th_probe_pn,
                                dn: INFINATE - 1,
                                sh: th_probe_sh,
                            };
                            for &i in &to_probe {
                                if w.abort || shared.abort_bn.load(Ordering::Relaxed) {
                                    break;
                                }
                                let child_pos = pos.move_forward(list[i].head_move());
                                w.bn_search_and::<P, false>(&child_pos, thdata, &mut list[i]);
                            }
                        }
                    }
                    w.release_list(list);
                }

                {
                    let mut st = warm_mu.lock().unwrap();
                    st.active -= 1;
                    if w.abort || shared.suspended.load(Ordering::Relaxed) {
                        shared.abort_bn.store(true, Ordering::Relaxed);
                        warm_cv.notify_all();
                    } else if st.stack.is_empty() && st.active == 0 {
                        warm_cv.notify_all();
                    }
                }
            }
        });

        self.nodes = shared.global_nodes.load(Ordering::Relaxed);
        if shared.suspended.load(Ordering::Relaxed) {
            self.suspend = true;
        }
    }

    fn make_tree<P: PositionBackend>(&mut self, root: &P, mvlist: &mut MvItem)
    where
        H: SearchHeuristic<P> + Clone + Send + Sync,
        F: Fn(usize) -> H + Sync,
    {
        let mut warmed = false;
        if !mvlist.inc()
            && mvlist.tdata.sh >= 32
            && self.pool.current_num_threads() >= 2
            && !self.error
            && !self.suspend
        {
            self.parallel_warm_proof_dag(root);
            warmed = true;
        }
        for pass in 0..self.config.n_make_tree {
            if self.error || self.suspend {
                break;
            }
            self.skip_sh_capped_probes = warmed && pass == 0 && !self.tbase.any_gc();
            self.tbase.clear_protect();
            if !mvlist.inc() {
                let mut list = self.acquire_list();
                self.make_tree_or_inner(root, mvlist, &mut list);
                self.release_list(list);
            } else {
                self.redundant = true;
            }
            self.skip_sh_capped_probes = false;
        }
    }

    fn make_tree_or_inner<P: PositionBackend>(
        &mut self,
        pos: &P,
        mvlist: &mut MvItem,
        list: &mut Vec<MvItem>,
    ) where
        H: SearchHeuristic<P> + Clone + Send + Sync,
        F: Fn(usize) -> H + Sync,
    {
        self.check_limits();
        if pos.count() >= TSUME_MAX_DEPTH - 1 || self.suspend {
            return;
        }
        let tn = pos.turn();
        pos.generate_check_fast(list);
        self.nodes += 1;
        if list.is_empty() {
            self.error = true;
            return;
        }

        let enemy_ou = pos.enemy_ou();
        let n = list.len();
        let th_probe_pn = (self.tbase.gc_max_level() + MAKE_TREE_PN_PLUS).max(self.config.mt_min_pn);
        let mut th_probe_sh = TSUME_MAX_DEPTH.saturating_sub(pos.count() + 1);

        let mut min_proven_sh = u16::MAX;
        let mut to_probe: smallvec::SmallVec<[usize; 32]> = smallvec::SmallVec::new();
        if n > 1 {
            self.tbase.prefetch(pos.key_forward_item(&list[0]).zkey);
            if n > 2 {
                self.tbase.prefetch(pos.key_forward_item(&list[1]).zkey);
            }
        }
        for i in 0..n {
            if i + 2 < n {
                self.tbase.prefetch(pos.key_forward_item(&list[i + 2]).zkey);
            }
            let child_ks = pos.key_forward_item(&list[i]);
            self.tbase.make_tree_lookup(&child_ks, &mut list[i], tn);
            if list[i].tdata.pn == 0 && !list[i].inc() && list[i].tdata.sh < min_proven_sh {
                min_proven_sh = list[i].tdata.sh;
            }
            if !list[i].search()
                || (list[i].tdata.pn != 0 && list[i].tdata.pn < self.config.mt_min_pn)
            {
                to_probe.push(i);
            }
        }
        if min_proven_sh < th_probe_sh {
            th_probe_sh = min_proven_sh;
        }
        if min_proven_sh > 0 && !to_probe.is_empty() && !self.suspend {
            let thdata = TData {
                pn: th_probe_pn,
                dn: INFINATE - 1,
                sh: th_probe_sh,
            };
            for &i in &to_probe {
                if self.suspend {
                    break;
                }
                if self.skip_sh_capped_probes
                    && list[i].search()
                    && list[i].tdata.pn != 0
                    && list[i].tdata.dn != 0
                    && list[i].tdata.sh >= th_probe_sh
                    && !self.tbase.any_gc()
                {
                    continue;
                }
                let child_pos = pos.move_forward(list[i].head_move());
                self.probe_and::<P, false>(&child_pos, thdata, &mut list[i]);
            }
        }

        for item in list.iter_mut() {
            item.length = DISTANCE[enemy_ou][item.head_move().dest()];
            if item.length > 1
                && item.tdata.pn == 1
                && item.tdata.dn == 1
                && item.tdata.sh == 0
                && item.head_move().is_drop()
            {
                item.tdata.pn = item.length as u16;
            }
        }

        sort_or(list);

        if list[0].tdata.pn != 0 && !self.suspend && list[0].tdata.pn < INFINATE - 1 {
            let mut or_item = MvItem::empty_root();
            let or_th = TData {
                pn: INFINATE - 1,
                dn: INFINATE - 1,
                sh: TSUME_MAX_DEPTH.saturating_sub(pos.count()),
            };
            self.probe_or_inner::<P, false>(pos, or_th, &mut or_item, true);
            for item in list.iter_mut() {
                let child_ks = pos.key_forward_item(item);
                self.tbase.make_tree_lookup(&child_ks, item, tn);
            }
            sort_or(list);
        }

        while list[0].tdata.pn != 0 && !self.suspend {
            if list[0].tdata.pn >= INFINATE - 1 {
                self.error = true;
                return;
            }
            let th_pn = if list.len() > 1 {
                (INFINATE - 1).min(list[1].tdata.pn.saturating_add(1))
            } else {
                INFINATE - 1
            };
            let thdata = TData {
                pn: th_pn,
                dn: INFINATE - 1,
                sh: TSUME_MAX_DEPTH.saturating_sub(pos.count() + 1),
            };
            let child_pos = pos.move_forward(list[0].head_move());
            self.probe_and::<P, false>(&child_pos, thdata, &mut list[0]);
            if self.suspend {
                return;
            }
            reorder_first_or(list);
        }
        if self.suspend {
            return;
        }

        if self.tbase.any_gc() && mvlist.tdata.sh < list[0].tdata.sh && !list[0].inc() {
            let ptsh = mvlist.tdata.sh;
            for item in list.iter_mut() {
                if item.tdata.pn != 0 || self.suspend {
                    break;
                }
                let child_pos = pos.move_forward(item.head_move());
                let mut sub = self.acquire_list();
                self.bns_plus_and_inner(&child_pos, item, ptsh, &mut sub);
                self.release_list(sub);
            }
            if self.suspend {
                return;
            }
            sort_or(list);
        }

        if list[0].cu() {
            return;
        }

        while !list[0].inc() && !list[0].pr() && !self.suspend && !self.error {
            let child_pos = pos.move_forward(list[0].head_move());
            let mut sub = self.acquire_list();
            self.make_tree_and_inner(&child_pos, &mut list[0], &mut sub);
            self.release_list(sub);
            if self.suspend || self.error {
                return;
            }
            list[0].set_pr(true);
            reorder_first_or(list);
        }
        if self.suspend || self.error {
            return;
        }

        mvlist.tdata.pn = list[0].tdata.pn;
        mvlist.tdata.dn = list[0].tdata.dn;
        mvlist.tdata.sh = list[0].tdata.sh.saturating_add(1);

        if mvlist.tdata.pn == 0 {
            proof_koma_or(pos, mvlist, &list[0]);
        } else if mvlist.tdata.dn == 0 {
            disproof_koma_or(pos, mvlist, list);
        }
        let ks = pos.key_state();
        self.tbase.make_tree_update(&ks, mvlist, tn);
    }

    fn make_tree_and_inner<P: PositionBackend>(
        &mut self,
        pos: &P,
        mvlist: &mut MvItem,
        list: &mut Vec<MvItem>,
    ) where
        H: SearchHeuristic<P> + Clone + Send + Sync,
        F: Fn(usize) -> H + Sync,
    {
        if self.suspend {
            return;
        }
        let tn = pos.turn().flip();
        let mut oracle = WorkerOracle {
            tbase: self.tbase,
            local_tt: self.probe_raw.as_mut().map(|r| &mut *r.local_tt),
            hs_depth: 0,
        };
        let (proof_flag, invalid_flag) = pos.generate_evasion_with(&mut oracle, list);
        self.nodes += 1;

        if list.is_empty() {
            tsumi_proof(pos, mvlist, proof_flag);
            let ks = pos.key_state();
            self.tbase.make_tree_update(&ks, mvlist, tn);
            return;
        }

        let mut i = 0;
        if list.len() > 1 {
            self.tbase.prefetch(pos.key_forward_item(&list[0]).zkey);
        }
        while i < list.len() {
            if i + 1 < list.len() {
                self.tbase.prefetch(pos.key_forward_item(&list[i + 1]).zkey);
            }
            let child_ks = pos.key_forward_item(&list[i]);
            self.tbase.make_tree_lookup(&child_ks, &mut list[i], tn);
            if let Some(tail) = list[i].take_tail() {
                list.insert(i + 1, split_tail_item(pos, tail));
            }
            i += 1;
        }

        sort_and(list);

        let thdata = TData {
            pn: INFINATE - 1,
            dn: INFINATE - 1,
            sh: TSUME_MAX_DEPTH.saturating_sub(pos.count() + 1),
        };
        while list[0].tdata.pn != 0 && !self.suspend {
            let child_pos = pos.move_forward(list[0].head_move());
            self.probe_or::<P, false>(&child_pos, thdata, &mut list[0]);
            if self.suspend {
                return;
            }
            if list[0].tdata.dn == 0 || list[0].tdata.pn >= INFINATE - 1 {
                self.error = true;
                return;
            }
            reorder_first_and(list);
        }
        if self.suspend {
            return;
        }

        let ks = pos.key_state();
        let current = self.tbase.make_tree_set_item(&ks, tn, false);
        for item in list.iter_mut() {
            if !item.inc() && !item.cu() && !item.pr() && !self.suspend && !self.error {
                let child_pos = pos.move_forward(item.head_move());
                let mut sub = self.acquire_list();
                self.make_tree_or_inner(&child_pos, item, &mut sub);
                self.release_list(sub);
                if !self.suspend && !self.error {
                    item.set_pr(true);
                }
            }
        }
        sort_and(list);
        self.tbase.clear_current_token(current);
        if self.suspend || self.error {
            return;
        }

        if invalid_flag && list[0].tdata.pn == 0 && list[0].inc() {
            tsumi_proof(pos, mvlist, proof_flag);
            let ks = pos.key_state();
            self.tbase.make_tree_update(&ks, mvlist, tn);
            return;
        }

        mvlist.tdata.pn = list[0].tdata.pn;
        mvlist.tdata.dn = list[0].tdata.dn;
        mvlist.tdata.sh = list[0].tdata.sh.saturating_add(1);

        if mvlist.tdata.dn == 0 {
            disproof_koma_and(pos, mvlist, &list[0]);
        } else if mvlist.tdata.pn == 0 {
            if proof_flag {
                let enemy_mkey = pos.mkey(pos.turn().flip());
                mvlist.mkey = enemy_mkey;
                mvlist.set_inc(list[0].inc());
                mvlist.set_hinc(list[0].inc());
                mvlist.nouse = list[0]
                    .nouse
                    .saturating_add(enemy_mkey.total().saturating_sub(list[0].mkey.total()));
                mvlist.nouse2 = mvlist.nouse;
            } else {
                proof_koma_and(pos, mvlist, list);
            }
        }
        let ks = pos.key_state();
        self.tbase.make_tree_update(&ks, mvlist, tn);
    }

    fn bns_plus_or_inner<P: PositionBackend>(
        &mut self,
        pos: &P,
        mvlist: &mut MvItem,
        ptsh: u16,
        list: &mut Vec<MvItem>,
    ) where
        H: SearchHeuristic<P> + Clone + Send + Sync,
        F: Fn(usize) -> H + Sync,
    {
        self.check_limits();
        if self.suspend {
            return;
        }
        let tn = pos.turn();
        pos.generate_check_fast(list);
        self.nodes += 1;
        if list.is_empty() {
            self.error = true;
            return;
        }

        let enemy_ou = pos.enemy_ou();
        for i in 0..list.len() {
            let child_ks = pos.key_forward_item(&list[i]);
            self.tbase.make_tree_lookup(&child_ks, &mut list[i], tn);
            if !list[i].search() {
                let child_pos = pos.move_forward(list[i].head_move());
                let thdata = TData {
                    pn: 1,
                    dn: INFINATE - 1,
                    sh: 3u16.max(self.tbase.gc_max_level() + 1),
                };
                self.probe_and::<P, false>(&child_pos, thdata, &mut list[i]);
            }
            let item = &mut list[i];
            item.length = DISTANCE[enemy_ou][item.head_move().dest()];
            if item.length > 1
                && item.tdata.pn == 1
                && item.tdata.dn == 1
                && item.tdata.sh == 0
                && item.head_move().is_drop()
            {
                item.tdata.pn = item.length as u16;
            }
        }

        sort_or(list);

        if list[0].tdata.pn == 0 && (list[0].inc() || list[0].tdata.sh == 0) {
            mvlist.tdata.pn = list[0].tdata.pn;
            mvlist.tdata.dn = if pos.count() > 0 {
                disproof_number(list).0
            } else {
                list[0].tdata.dn
            };
            mvlist.tdata.sh = list[0].tdata.sh.saturating_add(1);
            proof_koma_or(pos, mvlist, &list[0]);
            let ks = pos.key_state();
            self.tbase.make_tree_update(&ks, mvlist, tn);
            return;
        }

        let mut tsh = ptsh.saturating_sub(1);
        for item in list.iter_mut() {
            if self.suspend {
                break;
            }
            if item.tdata.pn == 0 && !item.cu() {
                tsh = tsh.min(item.tdata.sh);
                if !item.pr() {
                    let ks = pos.key_state();
                    let _cur_guard = self.tbase.enter_current(&ks, mvlist.tdata.pn, tn);
                    let child_pos = pos.move_forward(item.head_move());
                    let mut sub = self.acquire_list();
                    self.bns_plus_and_inner(&child_pos, item, tsh, &mut sub);
                    self.release_list(sub);
                    drop(_cur_guard);
                    item.set_pr(true);
                    if item.tdata.pn == 0 && item.inc() {
                        break;
                    }
                }
            }
            if item.tdata.pn != 0
                && item.tdata.pn < INFINATE - 1
                && !item.cu()
                && !item.pr()
                && mvlist.tdata.sh > 1
            {
                let thdata = TData {
                    pn: self
                        .add_thpn
                        .min(item.tdata.pn.max(self.tbase.gc_max_level()))
                        .saturating_add(1),
                    dn: INFINATE - 1,
                    sh: TSUME_MAX_DEPTH
                        .saturating_sub(pos.count() + 1)
                        .min(mvlist.tdata.sh.saturating_add(ADD_SEARCH_SH)),
                };
                let child_pos = pos.move_forward(item.head_move());
                self.probe_and::<P, false>(&child_pos, thdata, item);
                item.set_pr(true);
                if item.tdata.pn == 0 && item.inc() {
                    break;
                }
            }
        }

        sort_or(list);
        if mvlist.tdata.sh > list[0].tdata.sh || list[0].inc() {
            mvlist.tdata.pn = list[0].tdata.pn;
            mvlist.tdata.dn = if pos.count() > 0 {
                disproof_number(list).0
            } else {
                list[0].tdata.dn
            };
            mvlist.tdata.sh = list[0].tdata.sh.saturating_add(1);
            mvlist.set_inc(list[0].inc());
            if mvlist.tdata.pn == 0 {
                proof_koma_or(pos, mvlist, &list[0]);
            } else if mvlist.tdata.dn == 0 {
                disproof_koma_or(pos, mvlist, list);
            }
            let ks = pos.key_state();
            self.tbase.make_tree_update(&ks, mvlist, tn);
        }
    }

    fn bns_plus_and_inner<P: PositionBackend>(
        &mut self,
        pos: &P,
        mvlist: &mut MvItem,
        ptsh: u16,
        list: &mut Vec<MvItem>,
    ) where
        H: SearchHeuristic<P> + Clone + Send + Sync,
        F: Fn(usize) -> H + Sync,
    {
        if self.suspend {
            return;
        }
        let tn = pos.turn().flip();
        let mut oracle = WorkerOracle {
            tbase: self.tbase,
            local_tt: self.probe_raw.as_mut().map(|r| &mut *r.local_tt),
            hs_depth: 0,
        };
        let (proof_flag, invalid_flag) = pos.generate_evasion_with(&mut oracle, list);
        self.nodes += 1;

        if list.is_empty() {
            tsumi_proof(pos, mvlist, proof_flag);
            let ks = pos.key_state();
            self.tbase.make_tree_update(&ks, mvlist, tn);
            return;
        }

        if ptsh == 0 {
            let ks = pos.key_state();
            self.tbase.make_tree_update(&ks, mvlist, tn);
            return;
        }

        let mut i = 0;
        if list.len() > 1 {
            self.tbase.prefetch(pos.key_forward_item(&list[0]).zkey);
        }
        while i < list.len() {
            if i + 1 < list.len() {
                self.tbase.prefetch(pos.key_forward_item(&list[i + 1]).zkey);
            }
            let child_ks = pos.key_forward_item(&list[i]);
            self.tbase.lookup(&child_ks, &mut list[i], tn);
            if list[i].tdata.pn == 0 {
                if let Some(tail) = list[i].take_tail() {
                    list.insert(i + 1, split_tail_item(pos, tail));
                }
            }
            i += 1;
        }

        sort_and(list);

        let thdata = TData {
            pn: INFINATE - 1,
            dn: INFINATE - 1,
            sh: TSUME_MAX_DEPTH.saturating_sub(pos.count() + 1),
        };
        while list[0].tdata.pn != 0 && !self.suspend {
            let child_pos = pos.move_forward(list[0].head_move());
            self.probe_or::<P, false>(&child_pos, thdata, &mut list[0]);
            if list[0].tdata.dn == 0 || list[0].tdata.pn >= INFINATE - 1 {
                break;
            }
            reorder_first_and(list);
        }

        let mut tsh = ptsh.saturating_sub(1);
        let ks = pos.key_state();
        let current = self.tbase.make_tree_set_item(&ks, tn, false);
        while !list[0].inc() && !list[0].pr() && !self.suspend {
            tsh = tsh.min(list[0].tdata.sh);
            let child_pos = pos.move_forward(list[0].head_move());
            let mut sub = self.acquire_list();
            self.bns_plus_or_inner(&child_pos, &mut list[0], tsh, &mut sub);
            self.release_list(sub);
            list[0].set_pr(true);
            reorder_first_and(list);
        }
        self.tbase.clear_current_token(current);

        if invalid_flag && list[0].tdata.pn == 0 && list[0].inc() {
            tsumi_proof(pos, mvlist, proof_flag);
            let ks = pos.key_state();
            self.tbase.make_tree_update(&ks, mvlist, tn);
            return;
        }

        mvlist.tdata.pn = proof_number(list).0;
        mvlist.tdata.dn = list[0].tdata.dn;
        mvlist.tdata.sh = list[0].tdata.sh.saturating_add(1);
        if mvlist.tdata.dn == 0 {
            disproof_koma_and(pos, mvlist, &list[0]);
        } else if mvlist.tdata.pn == 0 {
            if proof_flag {
                let enemy_mkey = pos.mkey(pos.turn().flip());
                mvlist.mkey = enemy_mkey;
                mvlist.set_inc(list[0].inc());
                mvlist.set_hinc(list[0].inc());
                mvlist.nouse = list[0]
                    .nouse
                    .saturating_add(enemy_mkey.total().saturating_sub(list[0].mkey.total()));
            } else {
                proof_koma_and(pos, mvlist, list);
            }
        }
        let ks = pos.key_state();
        self.tbase.make_tree_update(&ks, mvlist, tn);
    }

    fn make_tree_simple_or_inner<P: PositionBackend>(
        &mut self,
        pos: &P,
        mvlist: &mut MvItem,
        list: &mut Vec<MvItem>,
        lo: u16,
    ) -> (u16, bool, Option<(Move, MKey, u16)>)
    where
        H: SearchHeuristic<P> + Clone + Send + Sync,
        F: Fn(usize) -> H + Sync,
    {
        self.check_limits();
        if pos.count() >= SIMPLE_MAX_DEPTH - 1 || self.suspend {
            return (NO_CU, false, None);
        }
        let tn = pos.turn();
        let ks = pos.key_state();
        let att_mkey = ks.mkey_for(tn);
        let def_mkey = ks.mkey_for(tn.flip());
        let state_key = (ks.zkey, att_mkey);

        if lo > 0 {
            if let Some(e) = self.simple_dag.lookup_ub(ks.zkey, att_mkey, lo) {
                e.apply_to(mvlist, att_mkey, def_mkey);
                return (NO_CU, false, None);
            }
        }
        if let Some(e) = self.simple_dag.lookup(ks.zkey, att_mkey, def_mkey) {
            e.apply_to(mvlist, att_mkey, def_mkey);
            return (NO_CU, true, None);
        }
        if let Some(&anc_dep) = self.mt_stack.get(&state_key) {
            mvlist.set_cu(true);
            return (anc_dep, false, None);
        }

        let mut list_populated = false;
        if let Some((cand_idx, cand_e, new_bits)) =
            self.simple_dag.find_candidate(ks.zkey, att_mkey, def_mkey)
        {
            if cand_e.sh <= 2 || new_bits == 0 {
                self.simple_dag.mark_safe(ks.zkey, cand_idx, new_bits);
                cand_e.apply_to(mvlist, att_mkey, def_mkey);
                return (NO_CU, true, None);
            }
            let mut rem_bits: u8 = new_bits;
            let mut forbid_now: u8 = 0;

            let mut any_drop_here = false;
            for k in 1u8..=7 {
                if (rem_bits & (1u8 << (k - 1))) != 0 && pos.drop_check(k) {
                    any_drop_here = true;
                    break;
                }
            }

            if any_drop_here {
                pos.generate_check_fast(list);
                self.nodes += 1;
                if pos.count() == 0 && self.symmetry {
                    list.retain(|item| sq_file(item.head_move().dest()) < 5);
                }
                list_populated = true;

                let short_sh_cap = 12u16.min(cand_e.sh.saturating_sub(2));
                if let Some(m1) = pos.find_1ply_mate_simple() {
                    if m1.mv.is_drop() {
                        let bit = 1u8 << (m1.mv.hand_ktype().saturating_sub(1) & 7);
                        if (rem_bits & bit) != 0 {
                            forbid_now |= bit;
                            rem_bits &= !bit;
                            if let Some(it) = list.iter_mut().find(|x| x.head_move() == m1.mv) {
                                it.tdata = TData::TSUMI;
                            }
                        }
                    }
                }
                if rem_bits != 0 && short_sh_cap >= 1 {
                    for item in list.iter_mut() {
                        if self.suspend {
                            break;
                        }
                        let hm = item.head_move();
                        if !hm.is_drop() {
                            continue;
                        }
                        let bit = 1u8 << (hm.hand_ktype().saturating_sub(1) & 7);
                        if (rem_bits & bit) == 0 {
                            continue;
                        }
                        let child_ks = pos.key_forward_item(item);
                        if self.mt_stack.contains_key(&(child_ks.zkey, child_ks.mkey_for(tn))) {
                            continue;
                        }
                        self.tbase.make_tree_lookup(&child_ks, item, tn);
                        if item.tdata.dn == 0 {
                            continue;
                        }
                        if item.tdata.pn != 0 && item.tdata.pn < 24 {
                            let child_pos = pos.move_forward(hm);
                            let thdata = TData {
                                pn: 24,
                                dn: INFINATE - 1,
                                sh: short_sh_cap,
                            };
                            self.probe_and_bounded::<P, true>(&child_pos, thdata, item, 96);
                        }
                        if item.tdata.pn == 0 && item.tdata.sh.saturating_add(1) < cand_e.sh {
                            forbid_now |= bit;
                            rem_bits &= !bit;
                        }
                    }
                }
            }

            if rem_bits != 0 && forbid_now == 0 && !self.suspend {
                let mut walk_pos = pos.clone();
                let mut walk_sh = cand_e.sh;
                let mut cur_or_mv = cand_e.best_mv;
                for step in 1..=8usize {
                    if rem_bits == 0
                        || walk_sh <= 3
                        || cur_or_mv == Move::default()
                        || self.suspend
                    {
                        break;
                    }
                    let and_pos = walk_pos.move_forward(cur_or_mv);
                    let Some((_, and_e, _)) = self.simple_dag.find_candidate(
                        and_pos.zkey(),
                        and_pos.mkey(tn),
                        and_pos.mkey(tn.flip()),
                    ) else {
                        break;
                    };
                    if and_e.sh <= 2 || and_e.best_mv == Move::default() {
                        break;
                    }
                    walk_pos = and_pos.move_forward(and_e.best_mv);
                    walk_sh = and_e.sh.saturating_sub(1);

                    let mut can_drop = false;
                    for k in 1u8..=7 {
                        if (rem_bits & (1u8 << (k - 1))) != 0 && walk_pos.drop_check(k) {
                            can_drop = true;
                            break;
                        }
                    }
                    if can_drop {
                        if let Some(m1) = walk_pos.find_1ply_mate_simple() {
                            if m1.mv.is_drop() {
                                let bit = 1u8 << (m1.mv.hand_ktype().saturating_sub(1) & 7);
                                if (rem_bits & bit) != 0 {
                                    forbid_now |= bit;
                                    rem_bits &= !bit;
                                }
                            }
                        }
                        if rem_bits != 0 && step <= 3 && walk_sh >= 4 {
                            let mut sub_checks = self.acquire_list();
                            walk_pos.generate_check_fast(&mut sub_checks);
                            self.nodes += 1;
                            let cap_sh = 8u16.min(walk_sh.saturating_sub(2));
                            for sc in sub_checks.iter_mut() {
                                if self.suspend {
                                    break;
                                }
                                let hm = sc.head_move();
                                if !hm.is_drop() {
                                    continue;
                                }
                                let bit = 1u8 << (hm.hand_ktype().saturating_sub(1) & 7);
                                if (rem_bits & bit) == 0 {
                                    continue;
                                }
                                let cks = walk_pos.key_forward_item(sc);
                                self.tbase.make_tree_lookup(&cks, sc, tn);
                                if sc.tdata.dn == 0 {
                                    continue;
                                }
                                if sc.tdata.pn != 0 && sc.tdata.pn < 16 {
                                    let cpos = walk_pos.move_forward(hm);
                                    let th = TData {
                                        pn: 16,
                                        dn: INFINATE - 1,
                                        sh: cap_sh,
                                    };
                                    self.probe_and_bounded::<P, true>(&cpos, th, sc, 64);
                                }
                                if sc.tdata.pn == 0 && sc.tdata.sh.saturating_add(1) < walk_sh {
                                    forbid_now |= bit;
                                    rem_bits &= !bit;
                                }
                            }
                            self.release_list(sub_checks);
                        }
                    }
                    if forbid_now != 0 {
                        break;
                    }
                    let Some((_, next_or_e, _)) = self.simple_dag.find_candidate(
                        walk_pos.zkey(),
                        walk_pos.mkey(tn),
                        walk_pos.mkey(tn.flip()),
                    ) else {
                        break;
                    };
                    cur_or_mv = next_or_e.best_mv;
                }
            }

            let safe_now = new_bits & !forbid_now;
            if safe_now != 0 && !self.suspend {
                self.simple_dag.mark_safe(ks.zkey, cand_idx, safe_now);
            }
            if forbid_now == 0 && !self.suspend {
                cand_e.apply_to(mvlist, att_mkey, def_mkey);
                return (NO_CU, true, None);
            }
            if forbid_now != 0 {
                self.simple_dag.mark_forbidden(ks.zkey, cand_idx, forbid_now);
            }
        }

        if !list_populated {
            pos.generate_check_fast(list);
            self.nodes += 1;
            if pos.count() == 0 && self.symmetry {
                list.retain(|item| sq_file(item.head_move().dest()) < 5);
            }
        }
        if list.is_empty() {
            self.error = true;
            return (NO_CU, false, None);
        }

        let my_depth = self.mt_stack.len() as u16;
        self.mt_stack.insert(state_key, my_depth);

        let enemy_ou = pos.enemy_ou();
        let mut skipped_cu_depth = NO_CU;
        let mut min_anc_dep = NO_CU;

        for item in list.iter_mut() {
            let child_ks = pos.key_forward_item(item);
            let c_att = child_ks.mkey_for(tn);
            let c_def = child_ks.mkey_for(tn.flip());
            let child_key = (child_ks.zkey, c_att);
            if let Some(&anc_dep) = self.mt_stack.get(&child_key) {
                if anc_dep < my_depth {
                    skipped_cu_depth = skipped_cu_depth.min(anc_dep);
                }
                self.tbase.make_tree_lookup(&child_ks, item, tn);
                item.set_cu(true);
                item.set_pr(false);
                if anc_dep >= my_depth || item.tdata.pn != 0 {
                    item.tdata.pn = INFINATE - 1;
                }
            } else if lo > 0 && self.simple_dag.lookup_ub(child_ks.zkey, c_att, lo - 1).is_some() {
                let ub_e = self
                    .simple_dag
                    .lookup_ub(child_ks.zkey, c_att, lo - 1)
                    .unwrap();
                ub_e.apply_to(item, c_att, c_def);
                self.mt_stack.remove(&state_key);
                mvlist.tdata = TData {
                    pn: 0,
                    dn: INFINATE,
                    sh: item.tdata.sh.saturating_add(1),
                };
                mvlist.def_need = MKey::EMPTY;
                mvlist.set_cu(false);
                proof_koma_or(pos, mvlist, item);
                self.simple_dag.insert(
                    ks.zkey,
                    SimpleDagEntry {
                        sh: mvlist.tdata.sh,
                        best_mv: item.head_move(),
                        mkey: mvlist.mkey,
                        def_need: MKey::EMPTY,
                        safe_spare_mask: 0,
                        forbidden_spare_mask: 0,
                        nouse: mvlist.nouse,
                        hinc: mvlist.hinc(),
                        is_exact: false,
                    },
                );
                return (
                    NO_CU,
                    false,
                    Some((item.head_move(), item.mkey, item.tdata.sh)),
                );
            } else if let Some(e) = self.simple_dag.lookup(child_ks.zkey, c_att, c_def) {
                e.apply_to(item, c_att, c_def);
                item.set_pr(true);
            } else {
                self.tbase.make_tree_lookup(&child_ks, item, tn);
                item.set_pr(false);
                item.set_cu(false);
            }
            item.length = DISTANCE[enemy_ou][item.head_move().dest()];
            if item.length > 1
                && item.tdata.pn == 1
                && item.tdata.dn == 1
                && item.tdata.sh == 0
                && item.head_move().is_drop()
            {
                item.tdata.pn = item.length as u16;
            }
        }

        sort_or_simple(list);

        let mut probed_short = false;
        let mut probed_cu_escape = false;
        let mut fallback_best: Option<MvItem> = None;
        let mut is_fail_low = false;
        loop {
            while list[0].tdata.pn != 0 && !self.suspend {
                if list[0].tdata.pn >= INFINATE - 1 {
                    if let Some(prev) = fallback_best.take() {
                        list[0] = prev;
                        break;
                    }
                    self.mt_stack.remove(&state_key);
                    if skipped_cu_depth < my_depth {
                        mvlist.set_cu(true);
                        return (skipped_cu_depth, false, None);
                    }
                    self.error = true;
                    return (NO_CU, false, None);
                }
                let th_pn = if list.len() > 1 {
                    (INFINATE - 1).min(list[1].tdata.pn.saturating_add(1))
                } else {
                    INFINATE - 1
                };
                let thdata = TData {
                    pn: th_pn,
                    dn: INFINATE - 1,
                    sh: SIMPLE_MAX_DEPTH.saturating_sub(pos.count() + 1),
                };
                let child_pos = pos.move_forward(list[0].head_move());
                self.probe_and::<P, true>(&child_pos, thdata, &mut list[0]);
                list[0].set_pr(false);
                let cks = pos.key_forward_item(&list[0]);
                let ckey = (cks.zkey, cks.mkey_for(tn));
                if let Some(&anc_dep) = self.mt_stack.get(&ckey) {
                    if anc_dep < my_depth {
                        skipped_cu_depth = skipped_cu_depth.min(anc_dep);
                    }
                    list[0].set_cu(true);
                    if anc_dep >= my_depth || list[0].tdata.pn != 0 {
                        list[0].tdata.pn = INFINATE - 1;
                    }
                }
                reorder_first_or_simple(list);
            }
            if self.suspend || self.error {
                self.mt_stack.remove(&state_key);
                return (NO_CU, false, None);
            }
            if list[0].cu() {
                if !probed_cu_escape {
                    probed_cu_escape = true;
                    let mut found_alt = false;
                    let max_rem_sh = SIMPLE_MAX_DEPTH.saturating_sub(pos.count() + 1);
                    for j in 1..list.len() {
                        if self.suspend {
                            break;
                        }
                        if !list[j].cu() && list[j].tdata.dn != 0 && list[j].tdata.pn < 24 {
                            let child_pos = pos.move_forward(list[j].head_move());
                            let thdata = TData {
                                pn: 24,
                                dn: INFINATE - 1,
                                sh: max_rem_sh,
                            };
                            self.probe_and_bounded::<P, true>(
                                &child_pos,
                                thdata,
                                &mut list[j],
                                128,
                            );
                            list[j].set_pr(false);
                            list[j].set_cu(false);
                            if list[j].tdata.pn == 0 {
                                found_alt = true;
                                break;
                            }
                        }
                    }
                    if found_alt {
                        sort_or_simple(list);
                        continue;
                    }
                }
                if let Some(prev) = fallback_best.take() {
                    list[0] = prev;
                    break;
                }
                if skipped_cu_depth < my_depth {
                    self.mt_stack.remove(&state_key);
                    mvlist.set_cu(true);
                    return (skipped_cu_depth, false, None);
                }
                list[0].tdata.pn = INFINATE - 1;
                reorder_first_or_simple(list);
                continue;
            }

            if !list[0].pr() {
                let child_pos = pos.move_forward(list[0].head_move());
                let mut sub = self.acquire_list();
                let (sub_dep, _sub_exact) = self.make_tree_simple_and_inner(
                    &child_pos,
                    &mut list[0],
                    &mut sub,
                    lo.saturating_sub(1),
                );
                self.release_list(sub);
                if self.suspend || self.error {
                    self.mt_stack.remove(&state_key);
                    return (NO_CU, false, None);
                }
                if list[0].cu() {
                    if sub_dep < my_depth {
                        skipped_cu_depth = skipped_cu_depth.min(sub_dep);
                        list[0].tdata.pn = 0;
                    } else {
                        list[0].tdata.pn = INFINATE - 1;
                    }
                    reorder_first_or_simple(list);
                    continue;
                }
                if sub_dep < my_depth {
                    min_anc_dep = min_anc_dep.min(sub_dep);
                }
                list[0].set_pr(true);
            }

            if let Some(prev) = fallback_best.take() {
                if prev.tdata.sh < list[0].tdata.sh {
                    list[0] = prev;
                }
                if lo > 0 && list[0].tdata.sh.saturating_add(1) <= lo {
                    is_fail_low = true;
                }
                break;
            }

            let best_sh = list[0].tdata.sh;
            if lo > 0 && best_sh.saturating_add(1) <= lo {
                is_fail_low = true;
                break;
            }
            if best_sh >= 2 && !probed_short {
                probed_short = true;
                if let Some(m1) = pos.find_1ply_mate_simple() {
                    if let Some(idx) = list.iter().position(|it| it.head_move() == m1.mv && !it.cu())
                    {
                        fallback_best = Some(list[0].clone());
                        list.swap(0, idx);
                        list[0].tdata = TData::TSUMI;
                        list[0].set_pr(false);
                        continue;
                    }
                }
                if best_sh >= 12 {
                    let mut tmp_or = MvItem::empty_root();
                    proof_koma_or(pos, &mut tmp_or, &list[0]);
                    let cur_spare = compute_spare_mask(att_mkey, tmp_or.mkey);
                    let short_sh_cap = 10u16.min(best_sh.saturating_sub(1));
                    for j in 1..list.len() {
                        if self.suspend || self.error {
                            break;
                        }
                        if list[j].cu() || list[j].tdata.dn == 0 || list[j].tdata.pn == 0 {
                            continue;
                        }
                        let hm = list[j].head_move();
                        let is_spare_drop = hm.is_drop()
                            && (cur_spare & (1u8 << (hm.hand_ktype().saturating_sub(1) & 7))) != 0;
                        let (pn_cap, node_cap) = if is_spare_drop || tmp_or.hinc() {
                            (24u16, 96u64)
                        } else {
                            (6u16, 24u64)
                        };
                        if list[j].tdata.pn < pn_cap {
                            let child_pos = pos.move_forward(hm);
                            let thdata = TData {
                                pn: pn_cap,
                                dn: INFINATE - 1,
                                sh: short_sh_cap,
                            };
                            self.probe_and_bounded::<P, true>(
                                &child_pos,
                                thdata,
                                &mut list[j],
                                node_cap,
                            );
                            list[j].set_pr(false);
                            list[j].set_cu(false);
                        }
                    }
                    let mut best_cand: Option<usize> = None;
                    for j in 1..list.len() {
                        if list[j].tdata.pn == 0
                            && !list[j].cu()
                            && list[j].tdata.sh <= 10
                            && list[j].tdata.sh < best_sh
                        {
                            if best_cand.map_or(true, |b| list[j].tdata.sh < list[b].tdata.sh) {
                                best_cand = Some(j);
                            }
                        }
                    }
                    if let Some(j) = best_cand {
                        fallback_best = Some(list[0].clone());
                        list.swap(0, j);
                        continue;
                    }
                }
            }
            break;
        }

        self.mt_stack.remove(&state_key);

        let win_mv = list[0].head_move();
        let win_child_mkey = list[0].mkey;
        let win_child_sh = list[0].tdata.sh;
        mvlist.tdata.pn = 0;
        mvlist.tdata.dn = INFINATE;
        mvlist.tdata.sh = win_child_sh.saturating_add(1);
        mvlist.def_need = list[0].def_need;
        mvlist.set_cu(false);
        proof_koma_or(pos, mvlist, &list[0]);

        let is_exact = !is_fail_low && !(lo > 0 && mvlist.tdata.sh <= lo);
        if is_exact {
            self.simple_dag.record_pv_hint(
                ks.zkey,
                mvlist.mkey,
                mvlist.def_need,
                mvlist.tdata.sh,
                win_mv,
            );
        }

        let anc_dep = if is_exact {
            skipped_cu_depth.min(min_anc_dep)
        } else {
            min_anc_dep
        };
        if anc_dep < my_depth {
            return (anc_dep, is_exact, Some((win_mv, win_child_mkey, win_child_sh)));
        }

        self.simple_dag.insert(
            ks.zkey,
            SimpleDagEntry {
                sh: mvlist.tdata.sh,
                best_mv: win_mv,
                mkey: mvlist.mkey,
                def_need: if is_exact { mvlist.def_need } else { MKey::EMPTY },
                safe_spare_mask: if is_exact {
                    compute_spare_mask(att_mkey, mvlist.mkey)
                } else {
                    0
                },
                forbidden_spare_mask: 0,
                nouse: mvlist.nouse,
                hinc: mvlist.hinc(),
                is_exact,
            },
        );
        if is_exact {
            self.tbase.make_tree_simple_update(&ks, mvlist, tn);
        }
        (NO_CU, is_exact, Some((win_mv, win_child_mkey, win_child_sh)))
    }

    fn make_tree_simple_and_inner<P: PositionBackend>(
        &mut self,
        pos: &P,
        mvlist: &mut MvItem,
        list: &mut Vec<MvItem>,
        lo: u16,
    ) -> (u16, bool)
    where
        H: SearchHeuristic<P> + Clone + Send + Sync,
        F: Fn(usize) -> H + Sync,
    {
        if self.suspend {
            return (NO_CU, false);
        }
        let tn = pos.turn().flip();
        let ks = pos.key_state();
        let att_mkey = ks.mkey_for(tn);
        let def_mkey = ks.mkey_for(tn.flip());
        let state_key = (ks.zkey, att_mkey);

        if lo > 0 {
            if let Some(e) = self.simple_dag.lookup_ub(ks.zkey, att_mkey, lo) {
                e.apply_to(mvlist, att_mkey, def_mkey);
                return (NO_CU, false);
            }
        }
        if let Some(e) = self.simple_dag.lookup(ks.zkey, att_mkey, def_mkey) {
            e.apply_to(mvlist, att_mkey, def_mkey);
            return (NO_CU, true);
        }
        if let Some(&anc_dep) = self.mt_stack.get(&state_key) {
            mvlist.set_cu(true);
            return (anc_dep, false);
        }

        if let Some((cand_idx, cand_e, new_bits)) =
            self.simple_dag.find_candidate(ks.zkey, att_mkey, def_mkey)
        {
            if cand_e.sh == 0 || new_bits == 0 {
                self.simple_dag.mark_safe(ks.zkey, cand_idx, new_bits);
                cand_e.apply_to(mvlist, att_mkey, def_mkey);
                return (NO_CU, true);
            }
            if cand_e.best_mv != Move::default() {
                let best_mv = cand_e.best_mv;
                let src_k = if best_mv.is_board_move() {
                    pos.board_at(best_mv.prev_pos as usize)
                } else {
                    0
                };
                let cap_k = pos.board_at(best_mv.dest());
                let mut best_item = MvItem::single_with_hints(best_mv, src_k, cap_k);
                let cks = pos.key_forward_item(&best_item);
                let ckey = (cks.zkey, cks.mkey_for(tn));
                if !self.mt_stack.contains_key(&ckey) {
                    let my_dep = self.mt_stack.len() as u16;
                    self.mt_stack.insert(state_key, my_dep);
                    let child_pos = pos.move_forward(best_mv);
                    let mut sub = self.acquire_list();
                    let (sub_dep, sub_exact, _) = self.make_tree_simple_or_inner(
                        &child_pos,
                        &mut best_item,
                        &mut sub,
                        lo.saturating_sub(1),
                    );
                    self.release_list(sub);
                    self.mt_stack.remove(&state_key);
                    if self.suspend || self.error {
                        return (NO_CU, false);
                    }
                    if !best_item.cu() && best_item.tdata.pn == 0 {
                        if sub_exact && best_item.tdata.sh.saturating_add(1) >= cand_e.sh && sub_dep >= my_dep {
                            self.simple_dag.mark_safe(ks.zkey, cand_idx, new_bits);
                            cand_e.apply_to(mvlist, att_mkey, def_mkey);
                            return (NO_CU, true);
                        }
                        if best_item.tdata.sh.saturating_add(1) < cand_e.sh
                            && new_bits.is_power_of_two()
                        {
                            self.simple_dag.mark_forbidden(ks.zkey, cand_idx, new_bits);
                        }
                    }
                }
            }
        }

        pos.generate_evasion_simple_fast(list);
        self.nodes += 1;

        if list.is_empty() {
            tsumi_proof(pos, mvlist, false);
            mvlist.def_need = MKey::EMPTY;
            mvlist.set_cu(false);
            self.simple_dag.insert(
                ks.zkey,
                SimpleDagEntry {
                    sh: 0,
                    best_mv: Move::default(),
                    mkey: mvlist.mkey,
                    def_need: MKey::EMPTY,
                    safe_spare_mask: 0x7f,
                    forbidden_spare_mask: 0,
                    nouse: mvlist.nouse,
                    hinc: mvlist.hinc(),
                    is_exact: true,
                },
            );
            self.tbase.make_tree_simple_update(&ks, mvlist, tn);
            return (NO_CU, true);
        }

        let my_depth = self.mt_stack.len() as u16;
        self.mt_stack.insert(state_key, my_depth);

        let mut i = 0;
        if list.len() > 1 {
            self.tbase.prefetch(pos.key_forward_item(&list[0]).zkey);
        }
        while i < list.len() {
            if i + 1 < list.len() {
                self.tbase.prefetch(pos.key_forward_item(&list[i + 1]).zkey);
            }
            let child_ks = pos.key_forward_item(&list[i]);
            let c_att = child_ks.mkey_for(tn);
            let c_def = child_ks.mkey_for(tn.flip());
            if let Some(e) = self.simple_dag.lookup(child_ks.zkey, c_att, c_def) {
                e.apply_to(&mut list[i], c_att, c_def);
                list[i].set_pr(true);
            } else {
                self.tbase.make_tree_lookup(&child_ks, &mut list[i], tn);
                list[i].set_pr(false);
                list[i].set_cu(false);
            }
            if let Some(tail) = list[i].take_tail() {
                list.insert(i + 1, split_tail_item(pos, tail));
            }
            i += 1;
        }

        sort_and_simple_ab(list);

        let thdata = TData {
            pn: INFINATE - 1,
            dn: INFINATE - 1,
            sh: SIMPLE_MAX_DEPTH.saturating_sub(pos.count() + 1),
        };

        let mut and_cu_depth = NO_CU;
        let mut min_anc_dep = NO_CU;
        let mut best_child_sh: u16 = 0;
        let mut best_idx: usize = 0;
        let mut any_exact_above_lo = false;
        let mut max_ub_sh: u16 = 0;
        let mut sq_refute: smallvec::SmallVec<[(u8, MKey, u16); 8]> = smallvec::SmallVec::new();

        for idx in 0..list.len() {
            if self.suspend || self.error {
                break;
            }
            let child_lo = best_child_sh.max(lo.saturating_sub(1));
            let hm = list[idx].head_move();

            // 2ns futile-drop cutoff using verified same-square capture refutation + monotonicity.
            if hm.is_drop() && child_lo > 0 {
                let sq = hm.dest() as u8;
                let ktype = hm.hand_ktype();
                let mut att_after = att_mkey;
                att_after.inc(ktype);
                let mut refuted = None;
                for &(rsq, cap_req, cap_sh) in &sq_refute {
                    if rsq == sq
                        && cap_sh.saturating_add(1) <= child_lo
                        && att_after.ge_all(cap_req)
                    {
                        refuted = Some((cap_req, cap_sh));
                        break;
                    }
                }
                if let Some((cap_req, cap_sh)) = refuted {
                    let sh_or = cap_sh.saturating_add(1);
                    let mut mk = cap_req;
                    if mk.get(ktype) > 0 {
                        mk.dec(ktype);
                    }
                    list[idx].tdata = TData {
                        pn: 0,
                        dn: INFINATE,
                        sh: sh_or,
                    };
                    list[idx].mkey = mk;
                    list[idx].def_need = MKey::EMPTY;
                    list[idx].set_pr(true);
                    list[idx].set_cu(false);
                    max_ub_sh = max_ub_sh.max(sh_or);
                    continue;
                }
            }

            let child_ks = pos.key_forward_item(&list[idx]);
            let c_att = child_ks.mkey_for(tn);
            let c_def = child_ks.mkey_for(tn.flip());
            let child_key = (child_ks.zkey, c_att);
            if let Some(&anc_dep) = self.mt_stack.get(&child_key) {
                list[idx].set_cu(true);
                and_cu_depth = and_cu_depth.min(anc_dep);
                continue;
            }
            if child_lo > 0 {
                if let Some(e) = self.simple_dag.lookup_ub(child_ks.zkey, c_att, child_lo) {
                    e.apply_to(&mut list[idx], c_att, c_def);
                    list[idx].set_pr(true);
                    max_ub_sh = max_ub_sh.max(list[idx].tdata.sh);
                    continue;
                }
            }
            if let Some(e) = self.simple_dag.lookup(child_ks.zkey, c_att, c_def) {
                e.apply_to(&mut list[idx], c_att, c_def);
                list[idx].set_pr(true);
                let csh = list[idx].tdata.sh;
                max_ub_sh = max_ub_sh.max(csh);
                if csh > best_child_sh || (!any_exact_above_lo && lo == 0) {
                    best_child_sh = csh;
                    best_idx = idx;
                    any_exact_above_lo = true;
                }
                continue;
            }

            let child_pos = pos.move_forward(hm);
            if list[idx].tdata.pn != 0 {
                self.probe_or::<P, true>(&child_pos, thdata, &mut list[idx]);
                list[idx].set_pr(false);
                if self.suspend {
                    break;
                }
                if list[idx].tdata.dn == 0 || list[idx].tdata.pn != 0 {
                    self.mt_stack.remove(&state_key);
                    self.error = true;
                    return (NO_CU, false);
                }
            }

            let mut sub = self.acquire_list();
            let (sub_dep, sub_exact, cap_refute) =
                self.make_tree_simple_or_inner(&child_pos, &mut list[idx], &mut sub, child_lo);
            self.release_list(sub);
            if self.suspend || self.error {
                break;
            }
            if list[idx].cu() {
                and_cu_depth = and_cu_depth.min(sub_dep);
            } else {
                list[idx].set_pr(true);
                if sub_dep < my_depth {
                    min_anc_dep = min_anc_dep.min(sub_dep);
                }
                if hm.is_drop() && sub_dep == NO_CU {
                    if let Some((or_mv, cap_req, cap_sh)) = cap_refute {
                        if or_mv.is_board_move() && or_mv.dest() == hm.dest() {
                            let sq = hm.dest() as u8;
                            if let Some(slot) = sq_refute.iter_mut().find(|x| x.0 == sq) {
                                if cap_sh < slot.2 {
                                    *slot = (sq, cap_req, cap_sh);
                                }
                            } else {
                                sq_refute.push((sq, cap_req, cap_sh));
                            }
                        }
                    }
                }
                let csh = list[idx].tdata.sh;
                max_ub_sh = max_ub_sh.max(csh);
                if sub_exact && (csh > best_child_sh || (!any_exact_above_lo && lo == 0)) {
                    best_child_sh = csh;
                    best_idx = idx;
                    any_exact_above_lo = true;
                }
            }
        }

        self.mt_stack.remove(&state_key);
        if self.suspend || self.error {
            return (NO_CU, false);
        }
        if and_cu_depth != NO_CU {
            mvlist.set_cu(true);
            return (and_cu_depth, false);
        }

        mvlist.tdata.pn = 0;
        mvlist.tdata.dn = INFINATE;
        mvlist.set_cu(false);
        proof_koma_and(pos, mvlist, list);

        if any_exact_above_lo {
            mvlist.tdata.sh = list[best_idx].tdata.sh.saturating_add(1);
            let mut dn_need = list[best_idx].def_need;
            let best_mv = list[best_idx].head_move();
            if best_mv.is_drop() {
                dn_need.inc(best_mv.hand_ktype());
            } else if list[best_idx].cap_koma != 0 {
                let ktype = list[best_idx].cap_koma % PROMOTED;
                if dn_need.get(ktype) > 0 {
                    dn_need.dec(ktype);
                }
            }
            mvlist.def_need = dn_need;
            self.simple_dag
                .record_pv_hint(ks.zkey, mvlist.mkey, dn_need, mvlist.tdata.sh, best_mv);

            if min_anc_dep < my_depth {
                return (min_anc_dep, true);
            }

            self.simple_dag.insert(
                ks.zkey,
                SimpleDagEntry {
                    sh: mvlist.tdata.sh,
                    best_mv,
                    mkey: mvlist.mkey,
                    def_need: dn_need,
                    safe_spare_mask: compute_spare_mask(att_mkey, mvlist.mkey),
                    forbidden_spare_mask: 0,
                    nouse: mvlist.nouse,
                    hinc: mvlist.hinc(),
                    is_exact: true,
                },
            );
            self.tbase.make_tree_simple_update(&ks, mvlist, tn);
            (NO_CU, true)
        } else {
            mvlist.tdata.sh = max_ub_sh.saturating_add(1);
            mvlist.def_need = MKey::EMPTY;
            if min_anc_dep < my_depth {
                return (min_anc_dep, false);
            }
            self.simple_dag.insert(
                ks.zkey,
                SimpleDagEntry {
                    sh: mvlist.tdata.sh,
                    best_mv: Move::default(),
                    mkey: mvlist.mkey,
                    def_need: MKey::EMPTY,
                    safe_spare_mask: 0,
                    forbidden_spare_mask: 0,
                    nouse: mvlist.nouse,
                    hinc: mvlist.hinc(),
                    is_exact: false,
                },
            );
            (NO_CU, false)
        }
    }

    fn extract_pv_from_simple_dag<P: PositionBackend>(&self, root: &P, target_sh: u16) -> Vec<Move> {
        let mut pv = Vec::new();
        let mut seen = HashSet::new();
        let mut pos = root.clone();
        let root_tn = root.turn();
        let def_tn = root_tn.flip();
        while pv.len() < (SIMPLE_MAX_DEPTH as usize) {
            let rem_sh = target_sh.saturating_sub(pv.len() as u16);
            if rem_sh == 0 {
                break;
            }
            let zkey = pos.zkey();
            let att = pos.mkey(root_tn);
            let def = pos.mkey(def_tn);
            if !seen.insert((zkey, att)) {
                break;
            }
            let Some(mv) = self.simple_dag.lookup_pv_move(zkey, att, def, rem_sh) else {
                break;
            };
            pv.push(mv);
            pos = pos.move_forward(mv);
        }
        pv
    }

    fn extract_pv<P: PositionBackend>(&mut self, root: &P) -> Vec<Move>
    where
        H: SearchHeuristic<P> + Clone + Send + Sync,
        F: Fn(usize) -> H + Sync,
    {
        let mut pv = Vec::new();
        let mut seen = HashSet::new();
        let mut seen_and = HashSet::new();
        let mut pos = root.clone();
        let root_tn = root.turn();

        for ply in 0..TSUME_MAX_DEPTH {
            let key = (pos.zkey(), pos.mkey(root_tn));
            if !seen.insert(key) {
                break;
            }
            if ply % 2 == 0 {
                let mut list = Vec::with_capacity(32);
                pos.generate_check_fast(&mut list);
                if list.is_empty() {
                    break;
                }
                for item in &mut list {
                    let child_ks = pos.key_forward_item(item);
                    self.tbase.make_tree_lookup(&child_ks, item, root_tn);
                    if seen_and.contains(&(child_ks.zkey, child_ks.mkey_for(root_tn))) {
                        item.set_cu(true);
                    }
                }
                sort_or(&mut list);
                if list[0].tdata.pn != 0 || list[0].cu() {
                    let mut mate_mv = pos.find_1ply_mate().map(|m1| m1.mv);
                    if mate_mv.is_none() {
                        let mut ev = Vec::with_capacity(8);
                        for item in &list {
                            let child = pos.move_forward(item.head_move());
                            ev.clear();
                            child.generate_evasion_simple_fast(&mut ev);
                            if ev.is_empty() {
                                mate_mv = Some(item.head_move());
                                break;
                            }
                        }
                    }
                    if let Some(mv) = mate_mv {
                        pv.push(mv);
                        pos = pos.move_forward(mv);
                        break;
                    }
                    // Materialize children if `pos` was closed via `SharedNiCache` (`inc == true`).
                    let saved_susp = self.suspend;
                    self.suspend = false;
                    let mut or_item = MvItem::empty_root();
                    let or_th = TData {
                        pn: INFINATE - 1,
                        dn: INFINATE - 1,
                        sh: TSUME_MAX_DEPTH.saturating_sub(pos.count()),
                    };
                    self.probe_or_inner::<P, false>(&pos, or_th, &mut or_item, true);
                    self.suspend = saved_susp;
                    for item in &mut list {
                        let child_ks = pos.key_forward_item(item);
                        self.tbase.make_tree_lookup(&child_ks, item, root_tn);
                        if seen_and.contains(&(child_ks.zkey, child_ks.mkey_for(root_tn))) {
                            item.set_cu(true);
                        }
                    }
                    sort_or(&mut list);
                    if list[0].tdata.pn != 0 || list[0].cu() {
                        break;
                    }
                }
                let mv = list[0].head_move();
                pv.push(mv);
                pos = pos.move_forward(mv);
                seen_and.insert((pos.zkey(), pos.mkey(root_tn)));
            } else {
                let mut list = Vec::with_capacity(16);
                let mut oracle = WorkerOracle {
                    tbase: self.tbase,
                    local_tt: self.probe_raw.as_mut().map(|r| &mut *r.local_tt),
                    hs_depth: 0,
                };
                let _ = pos.generate_evasion_with(&mut oracle, &mut list);
                if list.is_empty() {
                    break;
                }
                let mut i = 0;
                while i < list.len() {
                    let child_ks = pos.key_forward_item(&list[i]);
                    self.tbase.make_tree_lookup(&child_ks, &mut list[i], root_tn);
                    if let Some(tail) = list[i].take_tail() {
                        list.insert(i + 1, split_tail_item(&pos, tail));
                    }
                    i += 1;
                }
                let saved_susp = self.suspend;
                self.suspend = false;
                for item in &mut list {
                    if item.tdata.pn != 0 {
                        let child = pos.move_forward(item.head_move());
                        let th = TData {
                            pn: INFINATE - 1,
                            dn: INFINATE - 1,
                            sh: TSUME_MAX_DEPTH.saturating_sub(pos.count() + 1),
                        };
                        self.probe_or_inner::<P, false>(&child, th, item, true);
                    }
                }
                self.suspend = saved_susp;
                sort_and(&mut list);
                if list[0].tdata.pn != 0 {
                    break;
                }
                let mv = list[0].head_move();
                pv.push(mv);
                pos = pos.move_forward(mv);
            }
        }

        let mut rem_ev = Vec::with_capacity(8);
        pos.generate_evasion_simple_fast(&mut rem_ev);
        if !pv.is_empty() && pv.len() % 2 == 1 && !rem_ev.is_empty() {
            let mut tsume_ev = Vec::with_capacity(8);
            let mut oracle = WorkerOracle {
                tbase: self.tbase,
                local_tt: self.probe_raw.as_mut().map(|r| &mut *r.local_tt),
                hs_depth: 0,
            };
            pos.generate_evasion_with(&mut oracle, &mut tsume_ev);
            if tsume_ev.is_empty() {
                let emv = rem_ev[0].head_move();
                let next_or = pos.move_forward(emv);
                let saved_susp = self.suspend;
                self.suspend = false;
                let tail = self.extract_pv_simple(&next_or);
                self.suspend = saved_susp;
                if !tail.is_empty() && tail.len() % 2 == 1 {
                    pv.push(emv);
                    pv.extend(tail);
                    return pv;
                }
            }
        }
        if pv.is_empty() || pv.len() % 2 == 0 || !rem_ev.is_empty() {
            let saved_susp = self.suspend;
            self.suspend = false;
            let recovered = self.extract_pv_simple(root);
            self.suspend = saved_susp;
            if !recovered.is_empty() && recovered.len() % 2 == 1 {
                return recovered;
            }
        }
        pv
    }

    fn extract_pv_simple<P: PositionBackend>(&mut self, root: &P) -> Vec<Move>
    where
        H: SearchHeuristic<P> + Clone + Send + Sync,
        F: Fn(usize) -> H + Sync,
    {
        let mut pv = Vec::new();
        let mut seen_and = HashSet::new();
        let mut or_visits: std::collections::HashMap<(u64, MKey), u8> =
            std::collections::HashMap::new();
        let mut forbidden: HashSet<((u64, MKey), Move)> = HashSet::new();
        let mut or_hist: Vec<(P, usize, (u64, MKey), (u64, MKey), Move)> = Vec::new();
        let mut backtracks: usize = 0;
        let mut pos = root.clone();
        let root_tn = root.turn();

        while pv.len() < (SIMPLE_MAX_DEPTH as usize) {
            let ply = pv.len();
            let key = (pos.zkey(), pos.mkey(root_tn));
            if ply % 2 == 0 {
                let cnt = *or_visits.get(&key).unwrap_or(&0);
                if cnt >= 3 {
                    if backtracks < 128 {
                        if let Some((p_pos, p_len, p_or, p_and, bad_mv)) = or_hist.pop() {
                            backtracks += 1;
                            if let Some(c) = or_visits.get_mut(&p_or) {
                                *c = c.saturating_sub(1);
                            }
                            seen_and.remove(&p_and);
                            forbidden.insert((p_or, bad_mv));
                            pv.truncate(p_len);
                            pos = p_pos;
                            continue;
                        }
                    }
                    break;
                }
                let mut list = Vec::with_capacity(32);
                pos.generate_check_fast(&mut list);
                for item in &mut list {
                    let child_ks = pos.key_forward_item(item);
                    self.tbase.make_tree_lookup(&child_ks, item, root_tn);
                    if seen_and.contains(&(child_ks.zkey, child_ks.mkey_for(root_tn)))
                        || forbidden.contains(&(key, item.head_move()))
                    {
                        item.set_cu(true);
                    }
                }
                let pick = list
                    .iter()
                    .filter(|it| it.tdata.pn == 0 && !it.cu())
                    .min_by_key(|it| {
                        let cks = pos.key_forward_item(it);
                        (!it.pr(), cks.mkey_for(root_tn).0 != it.mkey.0, it.tdata.sh)
                    })
                    .map(|it| it.head_move());
                let chosen = if let Some(m) = pick {
                    Some(m)
                } else {
                    let mut found = None;
                    'probe: for cap_pn in [4u16, 16, 64, 256, 1024, INFINATE - 1] {
                        let th = TData {
                            pn: cap_pn,
                            dn: INFINATE - 1,
                            sh: SIMPLE_MAX_DEPTH.saturating_sub(pos.count() + 1),
                        };
                        for item in &mut list {
                            if !item.cu()
                                && item.tdata.pn != 0
                                && item.tdata.dn != 0
                                && item.tdata.pn < cap_pn
                            {
                                let child = pos.move_forward(item.head_move());
                                self.probe_and::<P, true>(&child, th, item);
                                if item.tdata.pn == 0 {
                                    found = Some(item.head_move());
                                    break 'probe;
                                }
                            }
                        }
                    }
                    found
                };
                let Some(mv) = chosen else {
                    if backtracks < 128 {
                        if let Some((p_pos, p_len, p_or, p_and, bad_mv)) = or_hist.pop() {
                            backtracks += 1;
                            if let Some(c) = or_visits.get_mut(&p_or) {
                                *c = c.saturating_sub(1);
                            }
                            seen_and.remove(&p_and);
                            forbidden.insert((p_or, bad_mv));
                            pv.truncate(p_len);
                            pos = p_pos;
                            continue;
                        }
                    }
                    break;
                };
                *or_visits.entry(key).or_insert(0) += 1;
                let next_pos = pos.move_forward(mv);
                let and_key = (next_pos.zkey(), next_pos.mkey(root_tn));
                seen_and.insert(and_key);
                or_hist.push((pos, ply, key, and_key, mv));
                pv.push(mv);
                pos = next_pos;
            } else {
                let mut list = Vec::with_capacity(16);
                pos.generate_evasion_simple_fast(&mut list);
                if list.is_empty() {
                    break;
                }
                let mut i = 0;
                while i < list.len() {
                    let child_ks = pos.key_forward_item(&list[i]);
                    self.tbase.make_tree_lookup(&child_ks, &mut list[i], root_tn);
                    if let Some(tail) = list[i].take_tail() {
                        list.insert(i + 1, split_tail_item(&pos, tail));
                    }
                    i += 1;
                }
                for item in &mut list {
                    if item.tdata.pn != 0 {
                        let th = TData {
                            pn: INFINATE - 1,
                            dn: INFINATE - 1,
                            sh: SIMPLE_MAX_DEPTH.saturating_sub(pos.count() + 1),
                        };
                        let child = pos.move_forward(item.head_move());
                        self.probe_or::<P, true>(&child, th, item);
                    }
                }
                sort_and_simple(&mut list);
                if list[0].tdata.pn != 0 {
                    if backtracks < 128 {
                        if let Some((p_pos, p_len, p_or, p_and, bad_mv)) = or_hist.pop() {
                            backtracks += 1;
                            if let Some(c) = or_visits.get_mut(&p_or) {
                                *c = c.saturating_sub(1);
                            }
                            seen_and.remove(&p_and);
                            forbidden.insert((p_or, bad_mv));
                            pv.truncate(p_len);
                            pos = p_pos;
                            continue;
                        }
                    }
                    break;
                }
                let mv = list[0].head_move();
                pv.push(mv);
                pos = pos.move_forward(mv);
            }
        }
        pv
    }
}

/// Persistent multi-threaded search engine owned by [`Solver`](crate::futsuu::Solver) when `threads > 1`.
pub struct ParallelEngine {
    pool: rayon::ThreadPool,
    tbase: SharedTBase,
    ni_cache: SharedNiCache,
    inflight: InFlightTable,
    corridor: SharedCorridorTable,
    scratches: ScratchPool,
    solve_epoch: u64,
    mt_stack: std::collections::HashMap<(u64, MKey), u16>,
    simple_dag: SimpleDag,
    post_mv_pool: Vec<Vec<MvItem>>,
    last_cycle_stats: Option<CycleStats>,
    pub last_bn_nodes: u64,
    pub last_bn_secs: f64,
    pub last_make_tree_secs: f64,
    abort_flag: Option<Arc<AtomicBool>>,
    progress_cb: Option<Arc<dyn Fn(u64) + Send + Sync>>,
}

impl ParallelEngine {
    pub fn new(hash_mb: usize, num_threads: usize) -> Self {
        let n_workers = num_threads.max(2);
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(n_workers)
            .stack_size(16 * 1024 * 1024)
            .build()
            .expect("failed to build parallel solver thread pool");
        let (ni_buckets, ni_meta) = if hash_mb >= 4096 {
            (16384, 262144)
        } else if hash_mb >= 512 {
            (8192, 262144)
        } else {
            (2048, 65536)
        };
        Self {
            pool,
            tbase: SharedTBase::new(hash_mb),
            ni_cache: SharedNiCache::new(ni_buckets, ni_meta),
            inflight: InFlightTable::new(),
            corridor: SharedCorridorTable::new(),
            scratches: ScratchPool::new(n_workers + 1),
            solve_epoch: 0,
            mt_stack: std::collections::HashMap::new(),
            simple_dag: SimpleDag::with_capacity(0),
            post_mv_pool: Vec::with_capacity(64),
            last_cycle_stats: None,
            last_bn_nodes: 0,
            last_bn_secs: 0.0,
            last_make_tree_secs: 0.0,
            abort_flag: None,
            progress_cb: None,
        }
    }

    pub fn set_abort_flag(&mut self, flag: Option<Arc<AtomicBool>>) {
        self.abort_flag = flag;
    }

    pub fn set_progress_callback(&mut self, cb: Option<Arc<dyn Fn(u64) + Send + Sync>>) {
        self.progress_cb = cb;
    }

    pub fn num_threads(&self) -> usize {
        self.pool.current_num_threads()
    }

    pub fn cycle_stats(&self) -> Option<CycleStats> {
        self.last_cycle_stats
    }

    pub fn solve<P: PositionBackend>(&mut self, config: &SearchConfig, root: &P) -> SearchResult {
        self.solve_with_factory_pn_cap(config, root, |_tid| DefaultHeuristic, PROOF_MAX - 1)
    }

    pub fn solve_unbounded_pn<P: PositionBackend>(
        &mut self,
        config: &SearchConfig,
        root: &P,
    ) -> SearchResult {
        self.solve_with_factory_pn_cap(config, root, |_tid| DefaultHeuristic, INFINATE - 1)
    }

    pub fn solve_with_factory<P, H, F>(
        &mut self,
        config: &SearchConfig,
        root: &P,
        make_heuristic: F,
    ) -> SearchResult
    where
        P: PositionBackend,
        H: SearchHeuristic<P> + Clone + Send + Sync,
        F: Fn(usize) -> H + Sync,
    {
        self.solve_with_factory_pn_cap(config, root, make_heuristic, PROOF_MAX - 1)
    }

    fn solve_with_factory_pn_cap<P, H, F>(
        &mut self,
        config: &SearchConfig,
        root: &P,
        make_heuristic: F,
        max_pn_cap: u16,
    ) -> SearchResult
    where
        P: PositionBackend,
        H: SearchHeuristic<P> + Clone + Send + Sync,
        F: Fn(usize) -> H + Sync,
    {
        self.tbase.clear();
        self.ni_cache.clear();
        self.corridor.clear();
        self.last_cycle_stats = None;
        #[cfg(debug_assertions)]
        self.inflight.assert_empty();
        self.solve_epoch = self.solve_epoch.wrapping_add(1).max(1);
        let epoch = self.solve_epoch;
        let caller_slot = self.pool.current_num_threads();
        let start = Instant::now();
        let symmetry = root.symmetry_check();

        let th_tdata = TData {
            pn: (PROOF_MAX - 1).min(max_pn_cap),
            dn: INFINATE - 1,
            sh: if config.simple_mate {
                SIMPLE_MAX_DEPTH
            } else {
                TSUME_MAX_DEPTH
            },
        };

        let shared = SharedRunState::new(
            config,
            &self.tbase,
            &self.ni_cache,
            &self.inflight,
            &self.corridor,
            caller_slot,
            start,
            symmetry,
            0,
        )
        .with_hooks(self.abort_flag.clone(), self.progress_cb.clone());

        // Stage 1: Fast 2,048-node sequential warmup on caller thread (solves <0.5 ms problems
        // with zero thread-wakeup overhead, zero heap allocations, and seeds top-of-tree bounds!).
        let mut root_mv = MvItem::empty_root();
        let warmup_cap = match config.node_limit {
            Some(nl) => WARMUP_NODES.min(nl),
            None => WARMUP_NODES,
        };
        let mut caller_raw = self.scratches.checkout(caller_slot, epoch);
        let mut caller_h = make_heuristic(0);
        if H::HAS_ONLINE_LEARNING {
            caller_h.reset_for_root(root);
        }
        {
            let mut w0 =
                WorkerCtx::from_raw::<P>(0, &shared, caller_h, caller_raw, Some(warmup_cap), false);
            if config.simple_mate {
                w0.bn_search_or::<P, true>(root, th_tdata, &mut root_mv);
            } else {
                w0.bn_search_or::<P, false>(root, th_tdata, &mut root_mv);
            }
            w0.flush_stats();

            if !w0.abort && (root_mv.tdata.pn == 0 || root_mv.tdata.dn == 0) {
                *shared.winner_root.lock().unwrap() = Some(root_mv.clone());
                shared.abort_bn.store(true, Ordering::Release);
            } else if let Some(nl) = config.node_limit {
                if shared.global_nodes.load(Ordering::Relaxed) >= nl {
                    shared.suspended.store(true, Ordering::Release);
                    shared.abort_bn.store(true, Ordering::Release);
                }
            }
            let (r_back, h_back) = w0.into_raw();
            caller_raw = r_back;
            caller_h = h_back;
        }

        // Stage 2: Full parallel broadcast across all `N` threads if not yet solved!
        if !shared.abort_bn.load(Ordering::Acquire) {
            let scratches = &self.scratches;
            self.pool.broadcast(|bctx| {
                let tid = bctx.index();
                let raw = scratches.checkout(tid, epoch);
                let mut h = make_heuristic(tid);
                if H::HAS_ONLINE_LEARNING {
                    h.reset_for_root(root);
                }
                let mut w = WorkerCtx::from_raw::<P>(tid, &shared, h, raw, None, false);
                let mut local_root = MvItem::empty_root();
                if config.simple_mate {
                    w.bn_search_or::<P, true>(root, th_tdata, &mut local_root);
                } else {
                    w.bn_search_or::<P, false>(root, th_tdata, &mut local_root);
                }
                w.flush_stats();

                if !w.abort
                    && w.unwind_target.is_none()
                    && (local_root.tdata.pn == 0
                        || local_root.tdata.dn == 0
                        || local_root.tdata.pn >= th_tdata.pn)
                {
                    let mut guard = shared.winner_root.lock().unwrap();
                    if guard.is_none() {
                        *guard = Some(local_root);
                    }
                    shared.abort_bn.store(true, Ordering::Release);
                }
                let (raw_back, _) = w.into_raw();
                scratches.checkin(tid, raw_back);
            });
        }

        // Unbounded-pn continuation pass in the warm SharedTBase if Pass 1 stopped at `PROOF_MAX - 1`.
        if max_pn_cap > PROOF_MAX - 1 && !shared.suspended.load(Ordering::Acquire) {
            let need_ext = {
                let guard = shared.winner_root.lock().unwrap();
                guard
                    .as_ref()
                    .is_none_or(|r| r.tdata.pn != 0 && r.tdata.dn != 0)
            };
            let ext_aborted = self
                .abort_flag
                .as_ref()
                .is_some_and(|f| f.load(Ordering::Relaxed));
            if need_ext && !ext_aborted {
                *shared.winner_root.lock().unwrap() = None;
                shared.abort_bn.store(false, Ordering::Release);
                let th_ext = TData {
                    pn: max_pn_cap,
                    dn: INFINATE - 1,
                    sh: th_tdata.sh,
                };
                let scratches = &self.scratches;
                self.pool.broadcast(|bctx| {
                    let tid = bctx.index();
                    let raw = scratches.checkout(tid, epoch);
                    let mut h = make_heuristic(tid);
                    if H::HAS_ONLINE_LEARNING {
                        h.reset_for_root(root);
                    }
                    let mut w = WorkerCtx::from_raw::<P>(tid, &shared, h, raw, None, false);
                    let mut local_root = MvItem::empty_root();
                    if config.simple_mate {
                        w.bn_search_or::<P, true>(root, th_ext, &mut local_root);
                    } else {
                        w.bn_search_or::<P, false>(root, th_ext, &mut local_root);
                    }
                    w.flush_stats();

                    if !w.abort
                        && w.unwind_target.is_none()
                        && (local_root.tdata.pn == 0
                            || local_root.tdata.dn == 0
                            || local_root.tdata.pn >= th_ext.pn)
                    {
                        let mut guard = shared.winner_root.lock().unwrap();
                        if guard.is_none() {
                            *guard = Some(local_root);
                        }
                        shared.abort_bn.store(true, Ordering::Release);
                    }
                    let (raw_back, _) = w.into_raw();
                    scratches.checkin(tid, raw_back);
                });
            }
        }

        let bn_nodes = shared.global_nodes.load(Ordering::Relaxed);
        let bn_secs = start.elapsed().as_secs_f64();
        self.last_bn_nodes = bn_nodes;
        self.last_bn_secs = bn_secs;
        self.last_cycle_stats = shared.scout_stats.lock().unwrap().take();

        let mut mvlist = shared
            .winner_root
            .lock()
            .unwrap()
            .take()
            .unwrap_or_else(MvItem::empty_root);
        let mut suspend = shared.suspended.load(Ordering::Relaxed);
        let max_thpn = shared.max_thpn.load(Ordering::Relaxed);

        self.mt_stack.clear();
        self.simple_dag.clear();
        let mut post = PostTreeBuilder {
            config,
            tbase: &self.tbase,
            ni_cache: &self.ni_cache,
            inflight: &self.inflight,
            corridor: &self.corridor,
            pool: &self.pool,
            scratches: &self.scratches,
            solve_epoch: epoch,
            make_heuristic: &make_heuristic,
            start,
            symmetry,
            nodes: bn_nodes,
            suspend,
            error: false,
            redundant: false,
            skip_sh_capped_probes: false,
            add_thpn: max_thpn,
            mt_stack: &mut self.mt_stack,
            simple_dag: &mut self.simple_dag,
            mv_pool: &mut self.post_mv_pool,
            probe_raw: Some(caller_raw),
            probe_heuristic: Some(caller_h),
            external_abort: self.abort_flag.clone(),
            progress_cb: self.progress_cb.clone(),
        };

        if mvlist.tdata.pn == 0 && !post.suspend && !post.error {
            let mt_start = Instant::now();
            if config.simple_mate {
                self.tbase.clear_protect();
                let mut list = post.acquire_list();
                post.make_tree_simple_or_inner(root, &mut mvlist, &mut list, 0);
                post.release_list(list);
                post.mt_stack.clear();
            } else {
                post.make_tree(root, &mut mvlist);
                let mut lv = config.search_level;
                while lv > 0 && !post.redundant && !post.suspend && !post.error {
                    post.add_thpn = post.add_thpn.saturating_add(1);
                    self.tbase.clear_protect();
                    let ptsh = mvlist.tdata.sh;
                    let mut list = post.acquire_list();
                    post.bns_plus_or_inner(root, &mut mvlist, ptsh, &mut list);
                    post.release_list(list);
                    post.make_tree(root, &mut mvlist);
                    lv -= 1;
                }
            }
            self.last_make_tree_secs = mt_start.elapsed().as_secs_f64();
        } else {
            self.last_make_tree_secs = 0.0;
        }
        suspend = post.suspend;

        let mut mate = if mvlist.tdata.pn == 0 && !post.error && (!config.simple_mate || !suspend) {
            Some(true)
        } else if mvlist.tdata.dn == 0 && !post.error {
            Some(false)
        } else {
            None
        };

        let pv_cfg = if mate == Some(true) {
            let mut c = config.clone();
            c.node_limit = Some(post.nodes.saturating_add(500_000));
            c.time_limit_secs = None;
            Some(c)
        } else {
            None
        };
        if let Some(ref c) = pv_cfg {
            post.config = c;
            post.suspend = false;
        }

        let pv = if mate == Some(true) {
            if config.simple_mate {
                let target_sh = mvlist.tdata.sh;
                let mut p = post.extract_pv_from_simple_dag(root, target_sh);
                if p.len() != target_sh as usize || p.is_empty() || p.len() % 2 == 0 {
                    let p2 = post.extract_pv_simple(root);
                    if !p2.is_empty() && p2.len() % 2 == 1 {
                        p = p2;
                    }
                }
                if !p.is_empty() && p.len() % 2 == 1 {
                    mvlist.tdata.sh = p.len() as u16;
                }
                p
            } else {
                let mut p = post.extract_pv(root);
                if suspend {
                    if p.is_empty() || p.len() % 2 == 0 {
                        mate = None;
                    } else {
                        let mut end_pos = root.clone();
                        for &mv in &p {
                            end_pos = end_pos.move_forward(mv);
                        }
                        let mut rem_ev = Vec::with_capacity(8);
                        end_pos.generate_evasion_simple_fast(&mut rem_ev);
                        if !rem_ev.is_empty() {
                            mate = None;
                        } else {
                            let (_, rem_eff, eff_unused) = analyze_tsume_pv(root, &p);
                            mvlist.tdata.sh = rem_eff[0];
                            post.redundant = eff_unused > 0;
                        }
                    }
                } else if !p.is_empty() && p.len() % 2 == 1 {
                    let (_, rem_eff, eff_unused) = analyze_tsume_pv(root, &p);
                    mvlist.tdata.sh = rem_eff[0];
                    post.redundant = eff_unused > 0;
                    if root.count() == 0 && !post.error && mvlist.tdata.sh <= 13 {
                        crate::futsuu::perfection::refine_short_tsume_pv_if_needed(
                            root,
                            &mut p,
                            &mut mvlist.tdata.sh,
                            &mut post.redundant,
                            &mut post.nodes,
                        );
                    }
                }
                if mate == Some(true) { p } else { Vec::new() }
            }
        } else {
            Vec::new()
        };
        let elapsed_secs = start.elapsed().as_secs_f64();

        let post_nodes = post.nodes;
        let post_redundant = post.redundant;
        let post_error = post.error;
        if let Some(raw_back) = post.probe_raw.take() {
            self.scratches.checkin(caller_slot, raw_back);
        }
        self.mt_stack.clear();
        self.simple_dag.clear();

        SearchResult {
            mate,
            mate_steps: if mate == Some(true) {
                mvlist.tdata.sh
            } else {
                0
            },
            redundant: post_redundant,
            pv,
            nodes: post_nodes,
            elapsed_secs,
            gc_num: self.tbase.gc_num(),
            gc_max_level: self.tbase.gc_max_level(),
            root_pn: self.tbase.root_pn.load(Ordering::Relaxed),
            error: post_error,
        }
    }
}
