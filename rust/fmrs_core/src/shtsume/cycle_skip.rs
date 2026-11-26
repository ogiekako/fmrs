//! Online Periodic-Cycle Detection, Non-Interference Soft Backup & Soft-Corridor Guidance (B2).
//!
//! Combines:
//! 1. L1-resident `ring_stack_idx` (4 KB) tracking active OR ancestors sharing the Defender King's
//!    3x3 neighborhood (`ring_hash`) without recomputing `BoardPlanes`.
//! 2. Generalized periodic cycle detection & legal rollout `P_1 -> P_2 -> ... -> P_r`:
//!    - Type I (`n_diff == 0, ΔH` monotone piece gain)
//!    - Type II (`n_diff ∈ {2, 3}`, single-piece board translation `v != (0, 0)`, e.g. 馬鋸 / 龍鋸)
//!    - Type IIIa (`n_diff == 0`, constructive hand conversion `持駒変換`, e.g. `2P -> 2L -> B`)
//!    - Type IIIb/c (`n_diff ∈ {1, 2}`, ray-excursion / conveyor / static enemy piece peeling `はがし`)
//! 3. Non-aborting backward `soft_backup` accelerated by `NiCache` (`B1-pn0`) and `CovariantCert`
//!    (`CovariantProofReplay`) so non-interfering side evasions close in O(|Cert|) across cycles.
//! 4. `CorridorTable` & lock-free `SharedCorridorTable` (soft df-pn corridor guidance): registers
//!    proven-boundary corridor OR moves so normal / parallel df-pn commits straight down to any
//!    remaining deep side evasions without oscillating across losing sibling checks. 100% sound.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{fence, AtomicU32, AtomicU64, Ordering};

use smallvec::SmallVec;

use crate::shtsume::backend::PositionBackend;
use crate::shtsume::ni_cache::BoardPlanes;
use crate::shtsume::types::{is_gote_koma, sq_file, sq_rank, MKey, Move, Turn, GOU};

pub const MIN_CYCLE_LEN: usize = 8;
pub const MAX_CYCLE_LEN: usize = 420;
pub const MAX_ROLLOUT_CYCLES: usize = 10;
pub const MAX_HAND_EXCHANGE_ROLLOUT_CYCLES: usize = 10;
pub const MAX_TYPE1_ROLLOUT_CYCLES: usize = 3;
pub const MAX_CERT_NODES: usize = 512;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum CycleKind {
    /// `n_diff == 0`, monotone hand gain (`cur_att >= anc_att && cur_def <= anc_def`).
    Type1Monotone,
    /// `n_diff == 0`, constructive hand conversion (`cur_att` and `anc_att` incomparable,
    /// e.g. `-1P +1L` or `-2L +1B` in 持駒変換).
    Type3HandExchange,
    /// `n_diff == 1`, Attacker sliding ray excursion `pivot_sq -> peel_sq -> pivot_sq`
    /// peeling consecutive enemy pieces along `(ray_df, ray_dr)` (`と金はがし` / `連取り`).
    Type3RayPeel {
        pivot_sq: u8,
        peel_sq: u8,
        ray_df: i8,
        ray_dr: i8,
        j_out: u16,
        j_back: u16,
    },
    /// `n_diff == 1`, Defender single-move conveyor recapture `peel_sq -> dest_sq` at `j_def`
    /// (`と金コンベア`).
    Type3ConveyorPeel {
        dest_sq: u8,
        j_def: u16,
    },
    /// `n_diff in {1, 2}`, enemy piece removed without `sigma` moving from that square again.
    Type3StaticPeel,
    /// `n_diff in {2, 3}`, single board piece translated by `shift != (0, 0)` (`馬鋸` / `龍鋸`).
    Type2Translate,
}

impl CycleKind {
    #[inline(always)]
    pub const fn cooldown_tag(self, shift: (i32, i32)) -> (i8, i8) {
        match self {
            CycleKind::Type1Monotone => (0, 0),
            CycleKind::Type3HandExchange => (64, 0),
            CycleKind::Type3RayPeel { ray_df, ray_dr, .. } => (32 + ray_df, 32 + ray_dr),
            CycleKind::Type3ConveyorPeel { .. } => (65, 0),
            CycleKind::Type3StaticPeel => (66, 0),
            CycleKind::Type2Translate => (shift.0 as i8, shift.1 as i8),
        }
    }

    #[inline(always)]
    pub const fn is_hand_monotone(self) -> bool {
        matches!(self, CycleKind::Type1Monotone)
    }
}

#[derive(Clone, Debug)]
pub struct CycleDetection {
    pub kind: CycleKind,
    pub sigma: Vec<Move>,
    pub shift_flags: Vec<bool>,
    pub shift: (i32, i32),
    pub tracked_start_sq: Option<u8>,
    pub cur_ply: u16,
}

#[derive(Copy, Clone, Debug)]
pub struct CertEdge {
    pub ev_mv: Move,
    pub captured_tracked: bool,
    pub moved_tracked: bool,
    pub child_idx: u16,
}

#[derive(Clone, Debug, Default)]
pub struct CertOrNode {
    pub best_mv: Move,
    pub orig_tracked: Option<u8>,
    pub moved_tracked: bool,
    pub edges: SmallVec<[CertEdge; 4]>,
}

/// Compact translation-covariant proof DAG certificate extracted from `TBase`
/// for a proven side evasion at cycle `c` and replayed onto cycle `c - 1`.
#[derive(Clone, Debug)]
pub struct CovariantCert {
    pub root_ev_mv: Move,
    pub root_captured_tracked: bool,
    pub root_moved_tracked: bool,
    pub nodes: Vec<CertOrNode>,
}

const RING_TABLE_SIZE: usize = 2048;
const RING_TABLE_MASK: usize = RING_TABLE_SIZE - 1;
const CORRIDOR_TABLE_SIZE: usize = 8192;
const CORRIDOR_TABLE_MASK: usize = CORRIDOR_TABLE_SIZE - 1;
const MAX_PROBED_CACHE: usize = 16384;

#[inline(always)]
fn corridor_hash_idx(zkey: u64, att_mkey: MKey) -> usize {
    let h = zkey ^ att_mkey.0.wrapping_mul(0x9e37_79b9_7f4a_7c15);
    ((h ^ (h >> 29)) as usize) & CORRIDOR_TABLE_MASK
}

#[repr(C, align(32))]
struct SharedCorridorSlot {
    seq: AtomicU64,
    zkey: AtomicU64,
    att_mkey: AtomicU64,
    mv_epoch: AtomicU64,
}

impl Default for SharedCorridorSlot {
    fn default() -> Self {
        Self {
            seq: AtomicU64::new(0),
            zkey: AtomicU64::new(0),
            att_mkey: AtomicU64::new(0),
            mv_epoch: AtomicU64::new(0),
        }
    }
}

/// Lock-free 256 KB concurrent seqlock corridor hint table shared across `ParallelEngine` workers.
///
/// Readers execute a single `Ordering::Relaxed` load (`active_count == 0`) on non-periodic problems.
pub struct SharedCorridorTable {
    slots: Box<[SharedCorridorSlot]>,
    epoch: AtomicU32,
    active_count: AtomicU32,
}

impl Default for SharedCorridorTable {
    fn default() -> Self {
        Self::new()
    }
}

impl SharedCorridorTable {
    pub fn new() -> Self {
        let mut v = Vec::with_capacity(CORRIDOR_TABLE_SIZE);
        v.resize_with(CORRIDOR_TABLE_SIZE, SharedCorridorSlot::default);
        Self {
            slots: v.into_boxed_slice(),
            epoch: AtomicU32::new(1),
            active_count: AtomicU32::new(0),
        }
    }

    #[inline]
    pub fn clear(&self) {
        let prev = self.epoch.load(Ordering::Relaxed);
        let next = prev.wrapping_add(1);
        if next == 0 {
            for s in self.slots.iter() {
                s.mv_epoch.store(0, Ordering::Relaxed);
            }
            self.epoch.store(1, Ordering::Release);
        } else {
            self.epoch.store(next, Ordering::Release);
        }
        self.active_count.store(0, Ordering::Release);
    }

    #[inline(always)]
    pub fn has_any(&self) -> bool {
        self.active_count.load(Ordering::Relaxed) != 0
    }

    #[inline]
    pub fn register(&self, zkey: u64, att_mkey: MKey, mv: Move) {
        let ep = self.epoch.load(Ordering::Relaxed) as u64;
        let idx = corridor_hash_idx(zkey, att_mkey);
        let slot = &self.slots[idx];
        let s0 = slot.seq.load(Ordering::Relaxed);
        if (s0 & 1) != 0 {
            return;
        }
        if slot
            .seq
            .compare_exchange(s0, s0.wrapping_add(1), Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            return;
        }
        fence(Ordering::Release);
        let old_me = slot.mv_epoch.load(Ordering::Relaxed);
        let packed_mv = (mv.prev_pos as u64) | ((mv.new_pos as u64) << 8) | (ep << 32);
        slot.zkey.store(zkey, Ordering::Relaxed);
        slot.att_mkey.store(att_mkey.0, Ordering::Relaxed);
        slot.mv_epoch.store(packed_mv, Ordering::Relaxed);
        slot.seq.store(s0.wrapping_add(2), Ordering::Release);
        if (old_me >> 32) != ep {
            self.active_count.fetch_add(1, Ordering::Relaxed);
        }
    }

    #[inline(always)]
    pub fn lookup(&self, zkey: u64, att_mkey: MKey) -> Option<Move> {
        if self.active_count.load(Ordering::Relaxed) == 0 {
            return None;
        }
        let ep = self.epoch.load(Ordering::Relaxed) as u64;
        let idx = corridor_hash_idx(zkey, att_mkey);
        let slot = &self.slots[idx];
        let s1 = slot.seq.load(Ordering::Acquire);
        if (s1 & 1) != 0 {
            return None;
        }
        let zk = slot.zkey.load(Ordering::Relaxed);
        let mk = slot.att_mkey.load(Ordering::Relaxed);
        let me = slot.mv_epoch.load(Ordering::Relaxed);
        fence(Ordering::Acquire);
        if slot.seq.load(Ordering::Relaxed) != s1 {
            return None;
        }
        if (me >> 32) == ep && zk == zkey && mk == att_mkey.0 {
            Some(Move {
                prev_pos: (me & 0xff) as u8,
                new_pos: ((me >> 8) & 0xff) as u8,
            })
        } else {
            None
        }
    }

    #[inline]
    pub fn invalidate(&self, zkey: u64, att_mkey: MKey) {
        if self.active_count.load(Ordering::Relaxed) == 0 {
            return;
        }
        let ep = self.epoch.load(Ordering::Relaxed) as u64;
        let idx = corridor_hash_idx(zkey, att_mkey);
        let slot = &self.slots[idx];
        let s0 = slot.seq.load(Ordering::Relaxed);
        if (s0 & 1) != 0 {
            return;
        }
        if slot
            .seq
            .compare_exchange(s0, s0.wrapping_add(1), Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            return;
        }
        fence(Ordering::Release);
        let zk = slot.zkey.load(Ordering::Relaxed);
        let mk = slot.att_mkey.load(Ordering::Relaxed);
        let me = slot.mv_epoch.load(Ordering::Relaxed);
        let matched = (me >> 32) == ep && zk == zkey && mk == att_mkey.0;
        if matched {
            slot.mv_epoch.store(0, Ordering::Relaxed);
        }
        slot.seq.store(s0.wrapping_add(2), Ordering::Release);
        if matched {
            let _ = self
                .active_count
                .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |c| {
                    Some(c.saturating_sub(1))
                });
        }
    }
}

#[derive(Copy, Clone, Debug)]
pub struct OrCycleFrame {
    pub ply: u16,
    pub mv_idx: u16,
    pub zkey: u64,
    pub ring_hash: u64,
    pub planes: BoardPlanes,
    pub att_mkey: MKey,
    pub def_mkey: MKey,
    pub prev_slot_val: u16,
}

#[derive(Copy, Clone, Default)]
struct CorridorSlot {
    zkey: u64,
    att_mkey: MKey,
    mv: Move,
    epoch: u16,
}

#[derive(Copy, Clone, Debug, Default)]
pub struct CycleStats {
    pub detected: u64,
    pub rollouts_ok: u64,
    pub max_rollout_cycles: usize,
    pub max_rollout_plies: usize,
    pub boundary_pn0: u64,
    pub boundary_dn0: u64,
    pub corridor_registered: u64,
    pub corridor_hits: u64,
    pub corridor_invalidated: u64,
    pub soft_bak_nodes_proven: u64,
    pub soft_bak_full_ok: u64,
    pub soft_bak_deep_deferred: u64,
    pub soft_bak_dn0_abort: u64,
    pub certs_extracted: u64,
    pub covariant_replays_ok: u64,
    pub covariant_replays_fail: u64,
    pub probe_nodes: u64,
    pub backup_nodes: u64,
}

pub struct CycleTracker {
    ring_stack_idx: [u16; RING_TABLE_SIZE],
    or_frames: Vec<OrCycleFrame>,
    stack_mvs: Vec<Move>,
    probed: HashSet<(u64, MKey, u16)>,
    pattern_cooldown: HashMap<(u16, i8, i8, u8), u64>,
    corridor: Vec<CorridorSlot>,
    corridor_epoch: u16,
    corridor_count: u32,
    pub stats: CycleStats,
}

impl Default for CycleTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl CycleTracker {
    pub fn new() -> Self {
        Self {
            ring_stack_idx: [0; RING_TABLE_SIZE],
            or_frames: Vec::with_capacity(1024),
            stack_mvs: Vec::with_capacity(2048),
            probed: HashSet::with_capacity(1024),
            pattern_cooldown: HashMap::with_capacity(128),
            corridor: vec![CorridorSlot::default(); CORRIDOR_TABLE_SIZE],
            corridor_epoch: 1,
            corridor_count: 0,
            stats: CycleStats::default(),
        }
    }

    pub fn clear(&mut self) {
        self.ring_stack_idx.fill(0);
        self.or_frames.clear();
        self.stack_mvs.clear();
        self.probed.clear();
        self.pattern_cooldown.clear();
        self.corridor_epoch = self.corridor_epoch.wrapping_add(1);
        if self.corridor_epoch == 0 {
            self.corridor_epoch = 1;
            for s in &mut self.corridor {
                s.epoch = 0;
            }
        }
        self.corridor_count = 0;
        self.stats = CycleStats::default();
    }

    /// Truncate stack frames & moves when an ancestor-solved unwind skips normal pop calls.
    #[inline]
    pub fn truncate_to_ply(&mut self, max_ply: u16, max_mv_len: usize) {
        while let Some(&top) = self.or_frames.last() {
            if top.ply >= max_ply {
                self.pop_or_frame();
            } else {
                break;
            }
        }
        if self.stack_mvs.len() > max_mv_len {
            self.stack_mvs.truncate(max_mv_len);
        }
    }

    #[inline(always)]
    pub fn mv_stack_len(&self) -> usize {
        self.stack_mvs.len()
    }

    #[inline(always)]
    pub fn has_corridor(&self) -> bool {
        self.corridor_count > 0
    }

    #[inline(always)]
    pub fn register_corridor_move(&mut self, zkey: u64, att_mkey: MKey, mv: Move) {
        let idx = corridor_hash_idx(zkey, att_mkey);
        let slot = &mut self.corridor[idx];
        if slot.epoch != self.corridor_epoch {
            self.corridor_count = self.corridor_count.saturating_add(1);
        }
        *slot = CorridorSlot {
            zkey,
            att_mkey,
            mv,
            epoch: self.corridor_epoch,
        };
        self.stats.corridor_registered += 1;
    }

    #[inline(always)]
    pub fn lookup_corridor(&mut self, zkey: u64, att_mkey: MKey) -> Option<Move> {
        if self.corridor_count == 0 {
            return None;
        }
        let idx = corridor_hash_idx(zkey, att_mkey);
        let slot = self.corridor[idx];
        if slot.epoch == self.corridor_epoch && slot.zkey == zkey && slot.att_mkey.0 == att_mkey.0 {
            self.stats.corridor_hits += 1;
            return Some(slot.mv);
        }
        None
    }

    #[inline(always)]
    pub fn invalidate_corridor(&mut self, zkey: u64, att_mkey: MKey) {
        if self.corridor_count == 0 {
            return;
        }
        let idx = corridor_hash_idx(zkey, att_mkey);
        let slot = &mut self.corridor[idx];
        if slot.epoch == self.corridor_epoch && slot.zkey == zkey && slot.att_mkey.0 == att_mkey.0 {
            slot.epoch = 0;
            self.corridor_count = self.corridor_count.saturating_sub(1);
            self.stats.corridor_invalidated += 1;
        }
    }

    #[inline]
    pub fn is_pattern_ready(
        &self,
        l: usize,
        kind: CycleKind,
        shift: (i32, i32),
        cur_ply: u16,
        cur_nodes: u64,
    ) -> bool {
        let (tx, ty) = kind.cooldown_tag(shift);
        let band = (cur_ply >> 6) as u8;
        let key = (l as u16, tx, ty, band);
        self.pattern_cooldown
            .get(&key)
            .is_none_or(|&until| cur_nodes >= until)
    }

    #[inline]
    pub fn set_pattern_cooldown(
        &mut self,
        l: usize,
        kind: CycleKind,
        shift: (i32, i32),
        cur_ply: u16,
        until_nodes: u64,
    ) {
        let (tx, ty) = kind.cooldown_tag(shift);
        let band = (cur_ply >> 6) as u8;
        let key = (l as u16, tx, ty, band);
        self.pattern_cooldown.insert(key, until_nodes);
    }

    #[inline(always)]
    pub fn push_move(&mut self, mv: Move) {
        self.stack_mvs.push(mv);
    }

    #[inline(always)]
    pub fn pop_move(&mut self) {
        self.stack_mvs.pop();
    }

    #[inline(always)]
    pub fn has_ring_ancestor(&self, ring_hash: u64) -> bool {
        self.ring_stack_idx[(ring_hash as usize) & RING_TABLE_MASK] != 0
    }

    #[inline(always)]
    pub fn push_or_frame(
        &mut self,
        ply: u16,
        zkey: u64,
        ring_hash: u64,
        planes: BoardPlanes,
        att_mkey: MKey,
        def_mkey: MKey,
    ) {
        let slot = (ring_hash as usize) & RING_TABLE_MASK;
        let prev_slot_val = self.ring_stack_idx[slot];
        let idx1 = (self.or_frames.len() + 1) as u16;
        self.or_frames.push(OrCycleFrame {
            ply,
            mv_idx: self.stack_mvs.len() as u16,
            zkey,
            ring_hash,
            planes,
            att_mkey,
            def_mkey,
            prev_slot_val,
        });
        self.ring_stack_idx[slot] = idx1;
    }

    #[inline(always)]
    pub fn pop_or_frame(&mut self) {
        if let Some(frame) = self.or_frames.pop() {
            let slot = (frame.ring_hash as usize) & RING_TABLE_MASK;
            self.ring_stack_idx[slot] = frame.prev_slot_val;
        }
    }

    /// Inspect ancestors sharing `ring_hash` for a clean periodic cycle `P_a --(σ)--> P_d`.
    pub fn detect_cycle<P: PositionBackend>(
        &mut self,
        pos: &P,
        planes: &BoardPlanes,
        ring_hash: u64,
        cur_nodes: u64,
    ) -> Option<CycleDetection> {
        let cur_ply = pos.count();
        let cur_zkey = pos.zkey();
        let cur_att = pos.mkey(Turn::Sente);
        let cur_def = pos.mkey(Turn::Gote);
        let cur_mv_len = self.stack_mvs.len();

        let slot = (ring_hash as usize) & RING_TABLE_MASK;
        let mut idx1 = self.ring_stack_idx[slot];
        let mut best_prio: u8 = 0;
        let mut best_l: usize = 0;
        let mut best_start: usize = 0;
        let mut best_kind = CycleKind::Type1Monotone;
        let mut best_shift: (i32, i32) = (0, 0);
        let mut best_tracked: Option<u8> = None;
        let mut best_flags = [false; MAX_CYCLE_LEN];
        let mut tmp_flags = [false; MAX_CYCLE_LEN];

        while idx1 != 0 {
            let anc = self.or_frames[(idx1 - 1) as usize];
            idx1 = anc.prev_slot_val;
            if anc.ring_hash != ring_hash {
                continue;
            }
            if cur_ply <= anc.ply {
                continue;
            }
            let l = (cur_ply - anc.ply) as usize;
            if l < MIN_CYCLE_LEN || l > MAX_CYCLE_LEN || (l & 1) != 0 {
                continue;
            }
            let start_mv = anc.mv_idx as usize;
            if start_mv + l != cur_mv_len {
                continue;
            }

            let diff = anc.planes.diff(planes);
            let n_diff = diff.count_ones();
            if n_diff > 3 {
                continue;
            }
            if n_diff == 0 && cur_att == anc.att_mkey && cur_def == anc.def_mkey {
                continue;
            }
            if self.probed.contains(&(cur_zkey, cur_att, l as u16)) {
                continue;
            }

            let sigma = &self.stack_mvs[start_mv..cur_mv_len];

            // ---- Case 1: Exact same board (`n_diff == 0`) — Type I Monotone OR Type III Hand-Conversion ----
            if n_diff == 0 {
                if cur_zkey != anc.zkey {
                    continue;
                }
                // Reject pure hand-loss千日手 (Attacker lost pieces while Defender gained pieces)
                if anc.att_mkey.ge_all(cur_att) && cur_def.ge_all(anc.def_mkey) {
                    continue;
                }
                // Conservation & first-drop feasibility: at least 1 more cycle `cur -> next` must leave hands >= 0
                if !MKey(cur_att.0 << 1).ge_all(anc.att_mkey)
                    || !MKey(cur_def.0 << 1).ge_all(anc.def_mkey)
                    || (sigma[0].is_drop() && cur_att.get(sigma[0].hand_ktype()) == 0)
                {
                    continue;
                }
                let kind = if cur_att.ge_all(anc.att_mkey) && anc.def_mkey.ge_all(cur_def) {
                    CycleKind::Type1Monotone
                } else {
                    CycleKind::Type3HandExchange
                };
                let prio = 4u8;
                if (prio > best_prio || (prio == best_prio && l > best_l))
                    && self.is_pattern_ready(l, kind, (0, 0), cur_ply, cur_nodes)
                {
                    best_prio = prio;
                    best_l = l;
                    best_start = start_mv;
                    best_kind = kind;
                    best_shift = (0, 0);
                    best_tracked = None;
                    best_flags[..l].fill(false);
                }
                continue;
            }

            // ---- Case 2: Single square peeled (`n_diff == 1`) — Ray-Excursion / Conveyor / Static Peel ----
            if n_diff == 1 {
                let fmrs_idx = diff.trailing_zeros() as usize;
                let s_peel = (fmrs_idx % 9) * 9 + (fmrs_idx / 9);
                if pos.board_at(s_peel) != 0 {
                    continue;
                }
                if !MKey(cur_att.0 << 1).ge_all(anc.att_mkey)
                    || !MKey(cur_def.0 << 1).ge_all(anc.def_mkey)
                {
                    continue;
                }

                let mut n_touch = 0usize;
                let mut first_touch: (usize, Move) = (0, Move::default());
                let mut second_touch: (usize, Move) = (0, Move::default());
                for (j, &mv) in sigma.iter().enumerate() {
                    let touches = mv.dest() == s_peel
                        || (mv.is_board_move() && mv.prev_pos as usize == s_peel);
                    if touches {
                        if n_touch == 0 {
                            first_touch = (j, mv);
                        } else if n_touch == 1 {
                            second_touch = (j, mv);
                        }
                        n_touch += 1;
                        if n_touch > 2 {
                            break;
                        }
                    }
                }

                // Subcase 2a: Attacker sliding ray excursion `p_sq -> s_peel -> p_sq` (`と金はがし`)
                if n_touch == 2 {
                    let (j_out, mv_out) = first_touch;
                    let (j_back, mv_back) = second_touch;
                    if (j_out & 1) == 0
                        && (j_back & 1) == 0
                        && mv_out.is_board_move()
                        && mv_back.is_board_move()
                        && mv_out.dest() == s_peel
                        && mv_back.prev_pos as usize == s_peel
                        && mv_back.dest() == mv_out.prev_pos as usize
                        && !mv_back.is_promote()
                    {
                        let p_sq = mv_out.prev_pos as usize;
                        let pivot_untouched_between = sigma[j_out + 1..j_back].iter().all(|&m| {
                            m.dest() != p_sq && (!m.is_board_move() || m.prev_pos as usize != p_sq)
                        });
                        if pivot_untouched_between {
                            let df_raw = sq_file(s_peel) as i32 - sq_file(p_sq) as i32;
                            let dr_raw = sq_rank(s_peel) as i32 - sq_rank(p_sq) as i32;
                            if (df_raw != 0 || dr_raw != 0)
                                && (df_raw == 0 || dr_raw == 0 || df_raw.abs() == dr_raw.abs())
                            {
                                let uf = df_raw.signum();
                                let ur = dr_raw.signum();
                                if let Some(next_sq) = shift_sq(s_peel, uf, ur) {
                                    if is_gote_koma(pos.board_at(next_sq as usize)) {
                                        let kind = CycleKind::Type3RayPeel {
                                            pivot_sq: p_sq as u8,
                                            peel_sq: s_peel as u8,
                                            ray_df: uf as i8,
                                            ray_dr: ur as i8,
                                            j_out: j_out as u16,
                                            j_back: j_back as u16,
                                        };
                                        let prio = 3u8;
                                        if (prio > best_prio || (prio == best_prio && l > best_l))
                                            && self.is_pattern_ready(
                                                l,
                                                kind,
                                                (uf, ur),
                                                cur_ply,
                                                cur_nodes,
                                            )
                                        {
                                            best_prio = prio;
                                            best_l = l;
                                            best_start = start_mv;
                                            best_kind = kind;
                                            best_shift = (0, 0);
                                            best_tracked = None;
                                            best_flags[..l].fill(false);
                                        }
                                        continue;
                                    }
                                }
                            }
                        }
                    }
                }

                // Subcase 2b: Defender single-move conveyor recapture `s_peel -> d_sq` (`と金コンベア`)
                if n_touch == 1 {
                    let (j_def, mv_def) = first_touch;
                    if (j_def & 1) == 1
                        && mv_def.is_board_move()
                        && mv_def.prev_pos as usize == s_peel
                    {
                        let d_sq = mv_def.dest();
                        let mut has_adj_defender = false;
                        for dr in -1..=1 {
                            for df in -1..=1 {
                                if df == 0 && dr == 0 {
                                    continue;
                                }
                                if let Some(adj) = shift_sq(d_sq, df, dr) {
                                    let k = pos.board_at(adj as usize);
                                    if is_gote_koma(k) && k != GOU {
                                        has_adj_defender = true;
                                        break;
                                    }
                                }
                            }
                            if has_adj_defender {
                                break;
                            }
                        }
                        if has_adj_defender {
                            let kind = CycleKind::Type3ConveyorPeel {
                                dest_sq: d_sq as u8,
                                j_def: j_def as u16,
                            };
                            let prio = if cur_att.ge_all(anc.att_mkey) { 3u8 } else { 2u8 };
                            if (prio > best_prio || (prio == best_prio && l > best_l))
                                && self.is_pattern_ready(l, kind, (0, 0), cur_ply, cur_nodes)
                            {
                                best_prio = prio;
                                best_l = l;
                                best_start = start_mv;
                                best_kind = kind;
                                best_shift = (0, 0);
                                best_tracked = None;
                                best_flags[..l].fill(false);
                            }
                            continue;
                        }
                    }
                }

                // Subcase 2c: Static blocker peel where `sigma` never moves from `s_peel`
                if sigma
                    .iter()
                    .all(|&m| !m.is_board_move() || m.prev_pos as usize != s_peel)
                {
                    let kind = CycleKind::Type3StaticPeel;
                    let prio = 1u8;
                    if (prio > best_prio || (prio == best_prio && l > best_l))
                        && self.is_pattern_ready(l, kind, (0, 0), cur_ply, cur_nodes)
                    {
                        best_prio = prio;
                        best_l = l;
                        best_start = start_mv;
                        best_kind = kind;
                        best_shift = (0, 0);
                        best_tracked = None;
                        best_flags[..l].fill(false);
                    }
                }
                continue;
            }

            // ---- Case 3: `n_diff == 2 || n_diff == 3` — Type II Translation (`馬鋸`/`龍鋸`) or 2-sq Static Peel ----
            if n_diff == 2 || n_diff == 3 {
                let mut diff_sqs = [0usize; 3];
                let mut nd = 0usize;
                let mut rem = diff;
                while rem != 0 && nd < 3 {
                    let fmrs_idx = rem.trailing_zeros() as usize;
                    rem &= rem - 1;
                    let col = fmrs_idx / 9;
                    let row = fmrs_idx % 9;
                    diff_sqs[nd] = row * 9 + col;
                    nd += 1;
                }

                let mut found: Option<(usize, usize)> = None;
                for i in 0..nd {
                    let s_from = diff_sqs[i];
                    if pos.board_at(s_from) != 0 {
                        continue;
                    }
                    let mut tracked = s_from;
                    tmp_flags[..l].fill(false);
                    let mut moved = false;
                    for (j, &mv) in sigma.iter().enumerate() {
                        if mv.is_board_move() && mv.prev_pos as usize == tracked {
                            if mv.is_promote() {
                                moved = false;
                                break;
                            }
                            tmp_flags[j] = true;
                            tracked = mv.dest();
                            moved = true;
                        } else if mv.dest() == tracked {
                            moved = false;
                            break;
                        }
                    }
                    if moved && tracked != s_from && pos.board_at(tracked) != 0 {
                        if (0..nd).any(|k| diff_sqs[k] == tracked) {
                            found = Some((s_from, tracked));
                            break;
                        }
                    }
                }

                if let Some((sq_from, sq_to)) = found {
                    let df = sq_file(sq_to) as i32 - sq_file(sq_from) as i32;
                    let dr = sq_rank(sq_to) as i32 - sq_rank(sq_from) as i32;
                    let kind = CycleKind::Type2Translate;
                    let prio = 3u8;
                    if (df != 0 || dr != 0)
                        && df.abs() <= 2
                        && dr.abs() <= 2
                        && (prio > best_prio || (prio == best_prio && l > best_l))
                        && self.is_pattern_ready(l, kind, (df, dr), cur_ply, cur_nodes)
                    {
                        best_prio = prio;
                        best_l = l;
                        best_start = start_mv;
                        best_kind = kind;
                        best_shift = (df, dr);
                        best_tracked = Some(sq_to as u8);
                        best_flags[..l].copy_from_slice(&tmp_flags[..l]);
                    }
                } else if n_diff == 2
                    && pos.board_at(diff_sqs[0]) == 0
                    && pos.board_at(diff_sqs[1]) == 0
                    && MKey(cur_att.0 << 1).ge_all(anc.att_mkey)
                    && MKey(cur_def.0 << 1).ge_all(anc.def_mkey)
                    && sigma.iter().all(|&m| {
                        !m.is_board_move()
                            || (m.prev_pos as usize != diff_sqs[0]
                                && m.prev_pos as usize != diff_sqs[1])
                    })
                {
                    let kind = CycleKind::Type3StaticPeel;
                    let prio = 1u8;
                    if (prio > best_prio || (prio == best_prio && l > best_l))
                        && self.is_pattern_ready(l, kind, (0, 0), cur_ply, cur_nodes)
                    {
                        best_prio = prio;
                        best_l = l;
                        best_start = start_mv;
                        best_kind = kind;
                        best_shift = (0, 0);
                        best_tracked = None;
                        best_flags[..l].fill(false);
                    }
                }
            }
        }
        if best_l > 0 {
            if self.probed.len() < MAX_PROBED_CACHE {
                self.probed.insert((cur_zkey, cur_att, best_l as u16));
            }
            self.stats.detected += 1;
            let sigma = self.stack_mvs[best_start..best_start + best_l].to_vec();
            let shift_flags = best_flags[..best_l].to_vec();
            return Some(CycleDetection {
                kind: best_kind,
                sigma,
                shift_flags,
                shift: best_shift,
                tracked_start_sq: best_tracked,
                cur_ply,
            });
        }
        None
    }
}

#[inline]
pub fn shift_sq(sq: usize, df: i32, dr: i32) -> Option<u8> {
    let nf = sq_file(sq) as i32 + df;
    let nr = sq_rank(sq) as i32 + dr;
    if (0..9).contains(&nf) && (0..9).contains(&nr) {
        Some((nr * 9 + nf) as u8)
    } else {
        None
    }
}

#[inline]
pub fn shift_move(mv: Move, df: i32, dr: i32) -> Option<Move> {
    let dest = mv.dest();
    let nf = sq_file(dest) as i32 + df;
    let nr = sq_rank(dest) as i32 + dr;
    if !(0..9).contains(&nf) || !(0..9).contains(&nr) {
        return None;
    }
    let ndest = (nr * 9 + nf) as u8;
    if mv.is_drop() {
        Some(Move::drop(mv.hand_ktype(), ndest))
    } else {
        let src = mv.prev_pos as usize;
        let sf = sq_file(src) as i32 + df;
        let sr = sq_rank(src) as i32 + dr;
        if !(0..9).contains(&sf) || !(0..9).contains(&sr) {
            return None;
        }
        let nsrc = (sr * 9 + sf) as u8;
        if mv.is_promote() {
            Some(Move::promote(nsrc, ndest))
        } else {
            Some(Move::normal(nsrc, ndest))
        }
    }
}

/// Roll forward from `start_pos` applying adapted copies `σ^{(1)}, σ^{(2)}, ...`
/// verifying exact move legality at every ply.
pub fn rollout_corridor<P: PositionBackend>(
    start_pos: &P,
    det: &CycleDetection,
    max_depth: u16,
) -> (Vec<(P, Vec<Move>)>, P) {
    let sigma = &det.sigma;
    let shift_flags = &det.shift_flags;
    let shift = det.shift;
    let l = sigma.len();
    let mut cycles: Vec<(P, Vec<Move>)> = Vec::new();
    let mut cur = start_pos.clone();
    let mut tmp_list = Vec::with_capacity(32);

    let max_k = match det.kind {
        CycleKind::Type1Monotone => MAX_TYPE1_ROLLOUT_CYCLES,
        CycleKind::Type3HandExchange => MAX_HAND_EXCHANGE_ROLLOUT_CYCLES,
        CycleKind::Type3ConveyorPeel { .. } | CycleKind::Type3StaticPeel => 6,
        CycleKind::Type3RayPeel { .. } | CycleKind::Type2Translate => MAX_ROLLOUT_CYCLES,
    };

    for k in 1..=max_k {
        if (cur.count() as usize) + l + 16 >= max_depth as usize {
            break;
        }
        let df = shift.0 * (k as i32);
        let dr = shift.1 * (k as i32);

        let ray_target = match det.kind {
            CycleKind::Type3RayPeel {
                peel_sq,
                ray_df,
                ray_dr,
                ..
            } => match shift_sq(
                peel_sq as usize,
                (ray_df as i32) * (k as i32),
                (ray_dr as i32) * (k as i32),
            ) {
                Some(sq) => Some(sq),
                None => break,
            },
            _ => None,
        };

        let mut adapted = Vec::with_capacity(l);
        let mut sim = cur.clone();
        let mut ok = true;

        for j in 0..l {
            let mut mv = if let (
                CycleKind::Type3RayPeel {
                    pivot_sq,
                    j_out,
                    j_back,
                    ..
                },
                Some(tsq),
            ) = (det.kind, ray_target)
            {
                if j == j_out as usize {
                    if !is_gote_koma(sim.board_at(tsq as usize)) {
                        ok = false;
                        break;
                    }
                    if sigma[j].is_promote() {
                        Move::promote(pivot_sq, tsq)
                    } else {
                        Move::normal(pivot_sq, tsq)
                    }
                } else if j == j_back as usize {
                    if sigma[j].is_promote() {
                        Move::promote(tsq, pivot_sq)
                    } else {
                        Move::normal(tsq, pivot_sq)
                    }
                } else {
                    sigma[j]
                }
            } else if shift_flags[j] {
                match shift_move(sigma[j], df, dr) {
                    Some(m) => m,
                    None => {
                        ok = false;
                        break;
                    }
                }
            } else {
                sigma[j]
            };

            tmp_list.clear();
            if (j & 1) == 0 {
                sim.generate_check_fast(&mut tmp_list);
                if !tmp_list.iter().any(|it| it.head_move() == mv) {
                    ok = false;
                    break;
                }
            } else {
                sim.generate_evasion_simple_fast(&mut tmp_list);
                let mut found = tmp_list
                    .iter()
                    .any(|it| it.head_move() == mv || it.all_moves().contains(&mv));
                if !found {
                    if let CycleKind::Type3ConveyorPeel { dest_sq, j_def } = det.kind {
                        if j == j_def as usize {
                            let dsq = dest_sq as usize;
                            let orig_k = cur.board_at(mv.prev_pos as usize);
                            let mut alt: Option<Move> = None;
                            for it in &tmp_list {
                                for m in it.all_moves() {
                                    if m.is_board_move() && m.dest() == dsq {
                                        let mk = sim.board_at(m.prev_pos as usize);
                                        if mk != GOU {
                                            if alt.is_none() || (orig_k != 0 && mk == orig_k) {
                                                alt = Some(m);
                                            }
                                        }
                                    }
                                }
                            }
                            if let Some(m_alt) = alt {
                                mv = m_alt;
                                found = true;
                            }
                        }
                    }
                }
                if !found {
                    ok = false;
                    break;
                }
            }
            sim = sim.move_forward(mv);
            adapted.push(mv);
        }

        if !ok {
            break;
        }
        cycles.push((cur, adapted));
        cur = sim;
    }

    (cycles, cur)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shtsume::{FmrsPos, PositionBackend, SearchConfig, Solver};

    #[test]
    fn test_shift_move_and_corridor_tables() {
        let mv = Move::normal(4 * 9 + 4, 3 * 9 + 3);
        let shifted = shift_move(mv, -1, -1).expect("in-bounds shift");
        assert_eq!(shifted.prev_pos, (3 * 9 + 3) as u8);
        assert_eq!(shifted.dest(), 2 * 9 + 2);
        assert!(shift_move(Move::normal(0, 1), -1, 0).is_none());

        let mut ct = CycleTracker::new();
        assert!(!ct.has_corridor());
        ct.register_corridor_move(0x1234, MKey(7 << 8), mv);
        assert!(ct.has_corridor());
        assert_eq!(ct.lookup_corridor(0x1234, MKey(7 << 8)), Some(mv));
        assert_eq!(ct.lookup_corridor(0x1234, MKey(8 << 8)), None);
        ct.invalidate_corridor(0x1234, MKey(7 << 8));
        assert_eq!(ct.lookup_corridor(0x1234, MKey(7 << 8)), None);
        ct.register_corridor_move(0x9999, MKey(1 << 8), shifted);
        ct.clear();
        assert!(!ct.has_corridor());
        assert_eq!(ct.lookup_corridor(0x9999, MKey(1 << 8)), None);

        let sct = SharedCorridorTable::new();
        assert!(!sct.has_any());
        sct.register(0xabcd, MKey(3 << 8), mv);
        assert!(sct.has_any());
        assert_eq!(sct.lookup(0xabcd, MKey(3 << 8)), Some(mv));
        assert_eq!(sct.lookup(0xabcd, MKey(4 << 8)), None);
        sct.invalidate(0xabcd, MKey(3 << 8));
        assert_eq!(sct.lookup(0xabcd, MKey(3 << 8)), None);
        sct.register(0x7777, MKey(2 << 8), shifted);
        sct.clear();
        assert!(!sct.has_any());
        assert_eq!(sct.lookup(0x7777, MKey(2 << 8)), None);
    }

    #[test]
    fn test_cycle_skip_soundness_mate_and_nomate() {
        let mut solver = Solver::new(SearchConfig {
            hash_mb: 32,
            threads: 1,
            use_ni_cache: true,
            use_cycle_skip: true,
            time_limit_secs: Some(5.0),
            ..Default::default()
        });
        // Kotobuki (611p)
        let kotobuki = FmrsPos::from_sfen(
            "g3p3+r/3lPG3/1s4ppp/1l1B4P/L2b1kPl1/3+p3+R+p/3+p2+s1S/+NP+p1gS3/1g+p2P3 b 4P3n 1",
        )
        .unwrap();
        let r = solver.solve(&kotobuki);
        assert_eq!(r.mate, Some(true));
        assert_eq!(r.mate_steps, 611);
        assert_eq!(r.pv.len(), 611);

        // Uchifuzume trap (must remain NoMate)
        let uchi = FmrsPos::from_sfen("4k4/9/4G4/9/9/9/9/9/9 b P2rb3g4s4n4l17p 1").unwrap();
        let ru = solver.solve(&uchi);
        assert_eq!(ru.mate, Some(false));

        // Perpetual check only (must remain NoMate)
        let perp = FmrsPos::from_sfen("4k4/9/9/9/9/9/9/9/4R4 b 2r2b4g4s4n4l18p 1").unwrap();
        let rp = solver.solve(&perp);
        assert_eq!(rp.mate, Some(false));
    }

    #[test]
    fn test_covariant_proof_replay_superlong_type2() {
        use crate::shtsume::tbase::TBase;
        let mut solver = Solver::new(SearchConfig {
            hash_mb: 128,
            threads: 1,
            use_ni_cache: true,
            use_cycle_skip: true,
            node_limit: Some(5_000_000),
            ..Default::default()
        });
        // 阿修羅 (365p) — multi-cycle Type II 馬鋸 where CovariantProofReplay extracts and replays certificates
        let ashura = FmrsPos::from_sfen(
            "S1p2ppS1/p2pG2l1/3Np2pP/LG5Ls/4k2N+B/1S+B4P+p/l1G4+R1/4+r4/G3N3N b 2P6p 1",
        )
        .unwrap();
        let r = solver.solve(&ashura);
        assert_eq!(r.mate, Some(true));
        assert!(
            r.mate_steps >= 365,
            "unexpected short mate_steps={}",
            r.mate_steps
        );
        assert_eq!(r.pv.len(), r.mate_steps as usize);
        assert_eq!(r.pv.len() % 2, 1);

        // Verify 100% PV legality and terminal checkmate
        let mut tb = TBase::new(1);
        let mut pos = ashura.clone();
        for (ply, &mv) in r.pv.iter().enumerate() {
            let legal: Vec<Move> = if ply % 2 == 0 {
                pos.generate_check(&mut tb)
                    .into_iter()
                    .flat_map(|it| it.all_moves())
                    .collect()
            } else {
                pos.generate_evasion_simple(&mut tb)
                    .into_iter()
                    .flat_map(|it| it.all_moves())
                    .collect()
            };
            assert!(
                legal.contains(&mv),
                "illegal move {} at ply {}",
                mv.to_sfen(),
                ply + 1
            );
            pos = pos.move_forward(mv);
        }
        assert!(pos.generate_evasion_simple(&mut tb).is_empty());

        let cs = solver.cycle_stats().expect("cycle_stats enabled");
        assert!(
            cs.covariant_replays_ok > 0,
            "expected covariant_replays_ok > 0, got {:?}",
            cs
        );
        assert!(
            cs.soft_bak_full_ok > 0,
            "expected soft_bak_full_ok > 0, got {:?}",
            cs
        );
    }

    #[test]
    fn test_type3_hand_conversion_and_parallel_cycle_scout() {
        use crate::shtsume::tbase::TBase;

        // Meta-Shinsekai checkpoint (`ply = 830`, periodic Type III hand conversion + ray peel)
        let sfen_830 =
            "1g7/1S1+R2+p1B/N1Nrp4/2p1kL1n1/1g1pB1+pg1/2sSL4/6+p2/N1g2+p3/9 b Lsl11p 1";
        let root = FmrsPos::from_sfen(sfen_830).unwrap();

        // Parallel 8T solver with `--cycle-skip` activates Dedicated Cycle-Scout!
        let mut par_solver = Solver::new(SearchConfig {
            hash_mb: 128,
            threads: 8,
            use_ni_cache: true,
            use_cycle_skip: true,
            time_limit_secs: Some(8.0),
            ..Default::default()
        });
        let r8 = par_solver.solve(&root);
        assert_eq!(r8.mate, Some(true));
        assert!(!r8.redundant);
        assert_eq!(r8.pv.len(), r8.mate_steps as usize);
        assert_eq!(r8.pv.len() % 2, 1);

        let mut tb = TBase::new(1);
        let mut pos = root.clone();
        for (ply, &mv) in r8.pv.iter().enumerate() {
            let legal: Vec<Move> = if ply % 2 == 0 {
                pos.generate_check(&mut tb)
                    .into_iter()
                    .flat_map(|it| it.all_moves())
                    .collect()
            } else {
                pos.generate_evasion_simple(&mut tb)
                    .into_iter()
                    .flat_map(|it| it.all_moves())
                    .collect()
            };
            assert!(
                legal.contains(&mv),
                "illegal move {} at ply {}",
                mv.to_sfen(),
                ply + 1
            );
            pos = pos.move_forward(mv);
        }
        assert!(pos.generate_evasion_simple(&mut tb).is_empty());

        let cs8 = par_solver
            .cycle_stats()
            .expect("8T parallel cycle scout stats");
        assert!(cs8.detected > 0, "expected cycle scout detections > 0");
    }
}
