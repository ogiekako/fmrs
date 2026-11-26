//! df-pn (`bn_search`) tsume-shogi solver with dominance, make_tree, and bns_plus
//! (ported from shtsume's `shtsume.c` and `nsearchinf.c`, generic over `PositionBackend`).

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crate::shtsume::time::Instant;

use crate::shtsume::backend::PositionBackend;
use crate::shtsume::cycle_skip::{
    rollout_corridor, shift_move, shift_sq, CertEdge, CertOrNode, CovariantCert, CycleDetection,
    CycleStats, CycleTracker, MAX_CERT_NODES,
};
use crate::shtsume::ni_cache::{
    mkey_bound_dec_if_bounded, mkey_bound_inc, mkey_min_dec_sat, proof_tbase_mkey, BoardPlanes,
    NiCache, NiFootprint, NiHit, NiStats, MIN_NI_INSERT_SH, MIN_NI_INSERT_WORK,
};
use crate::shtsume::parallel::ParallelEngine;
use crate::shtsume::proof::{
    disproof_koma, disproof_koma_and, disproof_koma_or, proof_koma, proof_koma_and,
    proof_koma_and_simple, proof_koma_or, proof_koma_or_simple, reorder_first_and,
    reorder_first_and_simple, reorder_first_or, reorder_first_or_simple, sort_and, sort_and_simple,
    sort_or, sort_or_simple,
};
use crate::shtsume::tbase::TBase;
use crate::shtsume::types::{
    sq_file, sq_rank, MKey, Move, MoveChain, MvItem, SearchConfig, SearchResult, TData, DISTANCE,
    INFINATE, IS_SKOMA, MAKE_TREE_PN_PLUS, PRE_DISPROOF_MAX, PRE_PROOF_MAX, PROMOTED, PROOF_MAX,
    TSUME_MAX_DEPTH,
};

const ADD_SEARCH_SH: u16 = 30;
const EDGE_PN_CEIL: u16 = PRE_PROOF_MAX - 2;
const EDGE_DN_CEIL: u16 = PRE_DISPROOF_MAX - 2;
/// Depth threshold above which deep forcing-corridor OR refinements activate
/// (`first_pn = 2` for unsearched 1-evasion chains and `1 + (pn >> 3)` OR hysteresis).
const DEEP_CORRIDOR_DEPTH: u16 = 40;

/// Pluggable move-initialization and ordering heuristic for `Solver`.
///
/// Monomorphized over `DefaultHeuristic` in `Solver::solve` for zero runtime overhead.
/// Implementations may adjust initial `(pn, dn)` (or tie-break fields such as `length`)
/// on unsearched items (`!item.search()`) while preserving exact soundness (`1 <= pn, dn < PRE_PROOF_MAX`).
pub trait SearchHeuristic<P: PositionBackend> {
    const HAS_EDGE_DELTA: bool = false;
    const HAS_PN_FLOOR: bool = false;
    const HAS_ONLINE_LEARNING: bool = false;

    #[inline(always)]
    fn reset_for_root(&mut self, _root: &P) {}

    #[inline(always)]
    fn score_or_moves(&mut self, _pos: &P, _list: &mut [MvItem]) {}

    #[inline(always)]
    fn score_or_moves_ctx(&mut self, pos: &P, _parent_mv: Move, _gc_num: u16, list: &mut [MvItem]) {
        self.score_or_moves(pos, list);
    }

    #[inline(always)]
    fn score_and_moves(&mut self, _pos: &P, _list: &mut [MvItem]) {}

    #[inline(always)]
    fn score_and_moves_ctx(
        &mut self,
        pos: &P,
        _parent_mv: Move,
        gc_num: u16,
        list: &mut [MvItem],
    ) {
        if gc_num < 2 {
            self.score_and_moves(pos, list);
        }
    }

    #[inline(always)]
    fn on_or_step(
        &mut self,
        _pos: &P,
        _parent_mv: Move,
        _list: &[MvItem],
        _old_pn: u16,
        _old_dn: u16,
        _sub_nodes: u64,
    ) {
    }

    #[inline(always)]
    fn on_and_step(
        &mut self,
        _pos: &P,
        _parent_mv: Move,
        _list: &[MvItem],
        _old_pn: u16,
        _old_dn: u16,
        _sub_nodes: u64,
    ) {
    }
}

/// Default zero-overhead heuristic dispatching to `P::default_score_and_moves`
/// (no-op on `ShtsumePos`; L1-resident `4.7 KB` `AndTacV2` table on `FmrsPos`).
#[derive(Copy, Clone, Debug, Default)]
pub struct DefaultHeuristic;

impl<P: PositionBackend> SearchHeuristic<P> for DefaultHeuristic {
    #[inline(always)]
    fn score_and_moves(&mut self, pos: &P, list: &mut [MvItem]) {
        pos.default_score_and_moves(list);
    }
}

/// Initial proof number `pn_0` for an unsearched OR check move (`DISTANCE[enemy_ou][dest]`),
/// removing the dead `pos.n_oute() > 0` check ported from C `shtsume.c:376`.
#[inline(always)]
pub(crate) fn initial_or_pn0(_enemy_ou: usize, item: &MvItem) -> u16 {
    (item.length as u16).max(1)
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

#[inline(always)]
pub(crate) fn copy_proof_fields(dst: &mut MvItem, src: &MvItem) {
    dst.tdata = src.tdata;
    dst.mkey = src.mkey;
    dst.def_need = src.def_need;
    dst.nouse = src.nouse;
    dst.nouse2 = src.nouse2;
    dst.set_inc(src.inc());
    dst.set_hinc(src.hinc());
    dst.set_cu(false);
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

const SIMPLE_MAX_DEPTH: u16 = 8000;
pub(crate) const NO_CU: u16 = u16::MAX;

#[derive(Copy, Clone, Debug)]
pub(crate) struct SimpleDagEntry {
    pub sh: u16,
    pub best_mv: Move,
    pub mkey: MKey,
    pub def_need: MKey,
    pub safe_spare_mask: u8,
    pub forbidden_spare_mask: u8,
    pub nouse: u8,
    pub hinc: bool,
    pub is_exact: bool,
}

#[inline]
pub(crate) fn sort_and_simple_ab(list: &mut [MvItem]) {
    list.sort_unstable_by_key(|a| {
        let not_proven = u64::from(a.tdata.pn != 0);
        let sh_ord = if a.tdata.pn == 0 {
            u64::from(0x1fffu16.saturating_sub(a.tdata.sh.min(0x1fff)))
        } else {
            u64::from(a.tdata.pn.min(0x1fff))
        };
        let hm = a.head_move();
        let is_drop = u64::from(hm.is_drop());
        let drop_ord = if hm.is_drop() {
            u64::from(hm.hand_ktype() & 7)
        } else {
            0
        };
        (not_proven << 20) | (sh_ord << 6) | (is_drop << 4) | drop_ord
    });
}

#[inline(always)]
pub(crate) fn compute_spare_mask(att: MKey, req: MKey) -> u8 {
    u8::from(att.fu() > req.fu())
        | (u8::from(att.ky() > req.ky()) << 1)
        | (u8::from(att.ke() > req.ke()) << 2)
        | (u8::from(att.gi() > req.gi()) << 3)
        | (u8::from(att.ki() > req.ki()) << 4)
        | (u8::from(att.ka() > req.ka()) << 5)
        | (u8::from(att.hi() > req.hi()) << 6)
}

impl SimpleDagEntry {
    #[inline(always)]
    pub(crate) fn apply_to(self, mv: &mut MvItem, att_mkey: MKey, def_mkey: MKey) {
        mv.tdata = TData {
            pn: 0,
            dn: INFINATE,
            sh: self.sh,
        };
        let mut mkey = self.mkey;
        if def_mkey.hi() == 0 && mkey.hi() != att_mkey.hi() {
            mkey.set_hi(att_mkey.hi());
        }
        if def_mkey.ka() == 0 && mkey.ka() != att_mkey.ka() {
            mkey.set_ka(att_mkey.ka());
        }
        if def_mkey.ki() == 0 && mkey.ki() != att_mkey.ki() {
            mkey.set_ki(att_mkey.ki());
        }
        if def_mkey.gi() == 0 && mkey.gi() != att_mkey.gi() {
            mkey.set_gi(att_mkey.gi());
        }
        if def_mkey.ke() == 0 && mkey.ke() != att_mkey.ke() {
            mkey.set_ke(att_mkey.ke());
        }
        if def_mkey.ky() == 0 && mkey.ky() != att_mkey.ky() {
            mkey.set_ky(att_mkey.ky());
        }
        if def_mkey.fu() == 0 && mkey.fu() != att_mkey.fu() {
            mkey.set_fu(att_mkey.fu());
        }
        mv.mkey = mkey;
        mv.def_need = self.def_need;
        mv.nouse = self.nouse.saturating_add(mkey.total().saturating_sub(self.mkey.total()));
        mv.nouse2 = att_mkey
            .total()
            .saturating_sub(mkey.total())
            .saturating_add(mv.nouse);
        mv.set_hinc(self.hinc || mv.nouse != 0);
        mv.set_inc(mv.nouse2 != 0 || att_mkey.0 != mkey.0 || mv.hinc());
        mv.set_cu(false);
    }
}

pub(crate) struct SimpleDag {
    map: std::collections::HashMap<u64, smallvec::SmallVec<[SimpleDagEntry; 2]>>,
    pv_hint: std::collections::HashMap<u64, smallvec::SmallVec<[(MKey, MKey, u16, Move); 2]>>,
}

impl SimpleDag {
    pub(crate) fn with_capacity(cap: usize) -> Self {
        Self {
            map: std::collections::HashMap::with_capacity(cap),
            pv_hint: std::collections::HashMap::with_capacity(cap),
        }
    }

    #[inline]
    pub(crate) fn clear(&mut self) {
        self.map.clear();
        self.pv_hint.clear();
    }

    #[inline]
    pub(crate) fn record_pv_hint(
        &mut self,
        zkey: u64,
        req_mkey: MKey,
        def_need: MKey,
        sh: u16,
        mv: Move,
    ) {
        let hints = self.pv_hint.entry(zkey).or_default();
        for existing in hints.iter_mut() {
            if existing.0 .0 == req_mkey.0 && existing.1 .0 == def_need.0 && existing.2 == sh {
                existing.3 = mv;
                return;
            }
        }
        hints.push((req_mkey, def_need, sh, mv));
    }

    #[inline]
    pub(crate) fn lookup_pv_move(
        &self,
        zkey: u64,
        att_mkey: MKey,
        def_mkey: MKey,
        expect_sh: u16,
    ) -> Option<Move> {
        if expect_sh == 0 {
            return None;
        }
        if let Some(cards) = self.map.get(&zkey) {
            for &e in cards.iter() {
                if e.is_exact
                    && e.sh == expect_sh
                    && att_mkey.ge_all(e.mkey)
                    && def_mkey.ge_all(e.def_need)
                {
                    let sp = compute_spare_mask(att_mkey, e.mkey);
                    if (sp & e.forbidden_spare_mask) == 0 && e.best_mv != Move::default() {
                        return Some(e.best_mv);
                    }
                }
            }
        }
        if let Some(hints) = self.pv_hint.get(&zkey) {
            for &(req, dn, h_sh, h_mv) in hints.iter().rev() {
                if h_sh == expect_sh
                    && att_mkey.ge_all(req)
                    && def_mkey.ge_all(dn)
                    && h_mv != Move::default()
                {
                    return Some(h_mv);
                }
            }
        }
        if let Some((_, e, _)) = self.find_candidate(zkey, att_mkey, def_mkey) {
            if e.sh == 0 || e.best_mv == Move::default() {
                return None;
            }
            return Some(e.best_mv);
        }
        if let Some(hints) = self.pv_hint.get(&zkey) {
            for &(req, dn, _, h_mv) in hints.iter().rev() {
                if att_mkey.ge_all(req) && def_mkey.ge_all(dn) && h_mv != Move::default() {
                    return Some(h_mv);
                }
            }
        }
        None
    }

    /// Monotonicity upper-bound lookup (`V(B, H'_A) <= V(B, H_A) <= e.sh` for all `H'_A >= e.mkey`):
    /// returns any proven entry (exact or upper bound) with `e.sh <= lo` and `att_mkey >= e.mkey`.
    #[inline(always)]
    pub(crate) fn lookup_ub(
        &self,
        zkey: u64,
        att_mkey: MKey,
        lo: u16,
    ) -> Option<SimpleDagEntry> {
        let cards = self.map.get(&zkey)?;
        for &e in cards.iter() {
            if e.sh > lo {
                break;
            }
            if att_mkey.ge_all(e.mkey) {
                return Some(e);
            }
        }
        None
    }

    #[inline(always)]
    pub(crate) fn lookup(
        &self,
        zkey: u64,
        att_mkey: MKey,
        def_mkey: MKey,
    ) -> Option<SimpleDagEntry> {
        let cards = self.map.get(&zkey)?;
        for &e in cards.iter() {
            if e.is_exact && att_mkey.ge_all(e.mkey) && def_mkey.ge_all(e.def_need) {
                let sp = compute_spare_mask(att_mkey, e.mkey);
                if (sp & e.forbidden_spare_mask) == 0 && (sp & !e.safe_spare_mask) == 0 {
                    return Some(e);
                }
            }
        }
        None
    }

    #[inline]
    pub(crate) fn find_candidate(
        &self,
        zkey: u64,
        att_mkey: MKey,
        def_mkey: MKey,
    ) -> Option<(usize, SimpleDagEntry, u8)> {
        let cards = self.map.get(&zkey)?;
        for (idx, &e) in cards.iter().enumerate() {
            if e.is_exact && att_mkey.ge_all(e.mkey) && def_mkey.ge_all(e.def_need) {
                let sp = compute_spare_mask(att_mkey, e.mkey);
                if (sp & e.forbidden_spare_mask) == 0 {
                    let new_bits = sp & !e.safe_spare_mask;
                    return Some((idx, e, new_bits));
                }
            }
        }
        None
    }

    #[inline]
    pub(crate) fn mark_safe(&mut self, zkey: u64, idx: usize, bits: u8) {
        if let Some(cards) = self.map.get_mut(&zkey) {
            if let Some(e) = cards.get_mut(idx) {
                e.safe_spare_mask |= bits;
            }
        }
    }

    #[inline]
    pub(crate) fn mark_forbidden(&mut self, zkey: u64, idx: usize, bits: u8) {
        if let Some(cards) = self.map.get_mut(&zkey) {
            if let Some(e) = cards.get_mut(idx) {
                e.forbidden_spare_mask |= bits;
            }
        }
    }

    #[inline]
    pub(crate) fn insert(&mut self, zkey: u64, entry: SimpleDagEntry) {
        let cards = self.map.entry(zkey).or_default();
        if !entry.is_exact {
            for existing in cards.iter_mut() {
                if existing.sh <= entry.sh && entry.mkey.ge_all(existing.mkey) {
                    return;
                }
                if !existing.is_exact
                    && entry.sh <= existing.sh
                    && existing.mkey.ge_all(entry.mkey)
                {
                    *existing = entry;
                    if cards.len() > 1 {
                        cards.sort_by_key(|c| c.sh);
                    }
                    return;
                }
            }
            if cards.len() >= 12 {
                return;
            }
            cards.push(entry);
            if cards.len() > 1 {
                cards.sort_by_key(|c| c.sh);
            }
            return;
        }
        for existing in cards.iter_mut() {
            if existing.is_exact
                && existing.sh == entry.sh
                && existing.mkey.0 == entry.mkey.0
                && existing.def_need.0 == entry.def_need.0
                && (entry.safe_spare_mask & existing.forbidden_spare_mask) == 0
                && (existing.safe_spare_mask & entry.forbidden_spare_mask) == 0
            {
                existing.safe_spare_mask |= entry.safe_spare_mask;
                existing.forbidden_spare_mask |= entry.forbidden_spare_mask;
                return;
            }
        }
        cards.push(entry);
        if cards.len() > 1 {
            cards.sort_by_key(|c| c.sh);
        }
    }
}

pub struct Solver {
    config: SearchConfig,
    tbase: TBase,
    ni_cache: Option<NiCache>,
    cycle_tracker: Option<CycleTracker>,
    in_cycle_probe: bool,
    parallel: Option<ParallelEngine>,
    mv_pool: Vec<Vec<MvItem>>,
    mt_stack: std::collections::HashMap<(u64, MKey), u16>,
    simple_dag: SimpleDag,
    nodes: u64,
    start: Instant,
    suspend: bool,
    error: bool,
    redundant: bool,
    symmetry: bool,
    max_thpn: u16,
    add_thpn: u16,
    root_pn: u16,
    pub last_bn_nodes: u64,
    pub last_bn_secs: f64,
    pub last_make_tree_secs: f64,
    abort_flag: Option<Arc<AtomicBool>>,
    progress_cb: Option<Arc<dyn Fn(u64) + Send + Sync>>,
}

impl Solver {
    pub fn new(config: SearchConfig) -> Self {
        let n_threads = config.resolved_threads();
        let (tbase, parallel, ni_cache) = if n_threads > 1 {
            (
                TBase::new(1),
                Some(ParallelEngine::new(config.hash_mb, n_threads)),
                None,
            )
        } else {
            let ni = if config.use_ni_cache {
                if config.hash_mb >= 512 {
                    Some(NiCache::new(8192, 131072))
                } else {
                    Some(NiCache::new(4096, 65536))
                }
            } else {
                None
            };
            (TBase::new(config.hash_mb), None, ni)
        };
        let cycle_tracker = if n_threads == 1 && config.use_cycle_skip {
            Some(CycleTracker::new())
        } else {
            None
        };
        Self {
            config,
            tbase,
            ni_cache,
            cycle_tracker,
            in_cycle_probe: false,
            parallel,
            mv_pool: (0..128).map(|_| Vec::with_capacity(64)).collect(),
            mt_stack: std::collections::HashMap::with_capacity(1024),
            simple_dag: SimpleDag::with_capacity(4096),
            nodes: 0,
            start: Instant::now(),
            suspend: false,
            error: false,
            redundant: false,
            symmetry: false,
            max_thpn: 0,
            add_thpn: 0,
            root_pn: 1,
            last_bn_nodes: 0,
            last_bn_secs: 0.0,
            last_make_tree_secs: 0.0,
            abort_flag: None,
            progress_cb: None,
        }
    }

    /// Attach an external cancellation flag checked periodically during search.
    pub fn set_abort_flag(&mut self, flag: Option<Arc<AtomicBool>>) {
        if let Some(par) = &mut self.parallel {
            par.set_abort_flag(flag.clone());
        }
        self.abort_flag = flag;
    }

    /// Attach a progress callback invoked periodically with the total nodes searched.
    pub fn set_progress_callback(&mut self, cb: Option<Arc<dyn Fn(u64) + Send + Sync>>) {
        if let Some(par) = &mut self.parallel {
            par.set_progress_callback(cb.clone());
        }
        self.progress_cb = cb;
    }

    pub fn num_threads(&self) -> usize {
        self.parallel
            .as_ref()
            .map(|p| p.num_threads())
            .unwrap_or(1)
    }

    pub fn tbase(&self) -> &TBase {
        &self.tbase
    }

    pub fn tbase_mut(&mut self) -> &mut TBase {
        &mut self.tbase
    }

    pub fn ni_stats(&self) -> Option<NiStats> {
        self.ni_cache.as_ref().map(|c| c.stats)
    }

    pub fn cycle_stats(&self) -> Option<CycleStats> {
        if let Some(par) = &self.parallel {
            return par.cycle_stats();
        }
        self.cycle_tracker.as_ref().map(|c| c.stats)
    }

    pub fn nodes(&self) -> u64 {
        self.nodes
    }

    pub fn is_suspended(&self) -> bool {
        self.suspend || self.error
    }

    pub fn set_node_limit(&mut self, limit: Option<u64>) {
        self.config.node_limit = limit;
    }

    /// Probe an AND node directly (e.g. after applying an alternative Attacker check at an OR node)
    /// without clearing `self.tbase`, returning updated `(pn, dn, sh, inc)` in `mv_item`.
    pub fn probe_and_after_check<P: PositionBackend>(
        &mut self,
        and_pos: &P,
        max_sh: u16,
        mv_item: &mut MvItem,
    ) {
        self.nodes = 0;
        self.suspend = false;
        self.error = false;
        self.start = Instant::now();
        let th = TData {
            pn: PROOF_MAX - 1,
            dn: INFINATE - 1,
            sh: max_sh,
        };
        let mut h = DefaultHeuristic;
        let saved_ni = self.config.use_ni_cache;
        self.config.use_ni_cache = false;
        self.bn_search_and::<P, DefaultHeuristic, false>(and_pos, th, mv_item, &mut h);
        self.config.use_ni_cache = saved_ni;
    }

    /// Probe an OR node directly (e.g. after a candidate Defender evasion) without clearing `self.tbase`.
    pub fn probe_or_after_evasion<P: PositionBackend>(&mut self, or_pos: &P, max_sh: u16) -> TData {
        self.nodes = 0;
        self.suspend = false;
        self.error = false;
        self.start = Instant::now();
        let mut mv_item = MvItem::empty_root();
        let th = TData {
            pn: PROOF_MAX - 1,
            dn: INFINATE - 1,
            sh: max_sh,
        };
        let mut h = DefaultHeuristic;
        let saved_ni = self.config.use_ni_cache;
        self.config.use_ni_cache = false;
        self.bn_search_or::<P, DefaultHeuristic, false>(or_pos, th, &mut mv_item, &mut h);
        self.config.use_ni_cache = saved_ni;
        mv_item.tdata
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

    pub fn solve<P: PositionBackend>(&mut self, root: &P) -> SearchResult {
        if let Some(par) = &mut self.parallel {
            let res = par.solve(&self.config, root);
            self.nodes = res.nodes;
            self.last_bn_nodes = par.last_bn_nodes;
            self.last_bn_secs = par.last_bn_secs;
            self.last_make_tree_secs = par.last_make_tree_secs;
            return res;
        }
        let mut h = DefaultHeuristic;
        self.solve_with(root, &mut h)
    }

    pub fn solve_parallel_with<P, H, F>(&mut self, root: &P, make_heuristic: F) -> SearchResult
    where
        P: PositionBackend,
        H: SearchHeuristic<P> + Clone + Send + Sync,
        F: Fn(usize) -> H + Sync,
    {
        if let Some(par) = &mut self.parallel {
            let res = par.solve_with_factory(&self.config, root, make_heuristic);
            self.nodes = res.nodes;
            self.last_bn_nodes = par.last_bn_nodes;
            self.last_bn_secs = par.last_bn_secs;
            self.last_make_tree_secs = par.last_make_tree_secs;
            return res;
        }
        let mut h = make_heuristic(0);
        self.solve_with(root, &mut h)
    }

    pub fn solve_with<P: PositionBackend, H: SearchHeuristic<P>>(
        &mut self,
        root: &P,
        heuristic: &mut H,
    ) -> SearchResult {
        self.tbase.clear();
        self.tbase.preserve_even_tsumi = self.config.simple_mate;
        if let Some(ni) = &mut self.ni_cache {
            ni.clear();
        }
        if let Some(ct) = &mut self.cycle_tracker {
            ct.clear();
        }
        self.in_cycle_probe = false;
        self.nodes = 0;
        self.start = Instant::now();
        self.suspend = false;
        self.error = false;
        self.redundant = false;
        self.max_thpn = 0;
        self.add_thpn = 0;
        self.root_pn = 1;
        self.last_bn_nodes = 0;
        self.last_bn_secs = 0.0;
        self.last_make_tree_secs = 0.0;
        self.symmetry = root.symmetry_check();
        if H::HAS_ONLINE_LEARNING {
            heuristic.reset_for_root(root);
        }

        let mut mvlist = MvItem::empty_root();
        let th_tdata = TData {
            pn: PROOF_MAX - 1,
            dn: INFINATE - 1,
            sh: if self.config.simple_mate {
                SIMPLE_MAX_DEPTH
            } else {
                TSUME_MAX_DEPTH
            },
        };

        if self.config.simple_mate {
            self.bn_search_or::<P, H, true>(root, th_tdata, &mut mvlist, heuristic);
        } else {
            self.bn_search_or::<P, H, false>(root, th_tdata, &mut mvlist, heuristic);
        }
        self.last_bn_nodes = self.nodes;
        self.last_bn_secs = self.start.elapsed().as_secs_f64();

        if mvlist.tdata.pn == 0 && !self.suspend && !self.error {
            let mt_start = Instant::now();
            if self.config.simple_mate {
                self.tbase.clear_protect();
                self.mt_stack.clear();
                self.simple_dag.clear();
                self.make_tree_simple_or(root, &mut mvlist, 0, heuristic);
                self.mt_stack.clear();
            } else {
                self.make_tree(root, &mut mvlist, heuristic);
                self.add_thpn = self.max_thpn;
                let mut lv = self.config.search_level;
                while lv > 0 && !self.redundant && !self.suspend && !self.error {
                    self.add_thpn = self.add_thpn.saturating_add(1);
                    self.tbase.clear_protect();
                    let ptsh = mvlist.tdata.sh;
                    self.bns_plus_or(root, &mut mvlist, ptsh, heuristic);
                    self.make_tree(root, &mut mvlist, heuristic);
                    lv -= 1;
                }
            }
            self.last_make_tree_secs = mt_start.elapsed().as_secs_f64();
        }

        let elapsed_secs = self.start.elapsed().as_secs_f64();
        let mate = if mvlist.tdata.pn == 0
            && !self.error
            && (!self.config.simple_mate || !self.suspend)
        {
            Some(true)
        } else if mvlist.tdata.dn == 0 && !self.error {
            Some(false)
        } else {
            None
        };

        let pv = if mate == Some(true) {
            if self.config.simple_mate {
                let target_sh = mvlist.tdata.sh;
                let mut p = self.extract_pv_from_simple_dag(root, target_sh);
                if p.len() != target_sh as usize || p.is_empty() || p.len() % 2 == 0 {
                    let saved_nl = self.config.node_limit.take();
                    let saved_tl = self.config.time_limit_secs.take();
                    let p2 = self.extract_pv_simple(root);
                    self.config.node_limit = saved_nl;
                    self.config.time_limit_secs = saved_tl;
                    if !p2.is_empty() && p2.len() % 2 == 1 {
                        p = p2;
                    }
                }
                if !p.is_empty() && p.len() % 2 == 1 {
                    mvlist.tdata.sh = p.len() as u16;
                }
                self.simple_dag.clear();
                p
            } else {
                self.extract_pv(root)
            }
        } else {
            self.simple_dag.clear();
            Vec::new()
        };

        SearchResult {
            mate,
            mate_steps: if mate == Some(true) {
                mvlist.tdata.sh
            } else {
                0
            },
            redundant: self.redundant,
            pv,
            nodes: self.nodes,
            elapsed_secs,
            gc_num: self.tbase.gc_num,
            gc_max_level: self.tbase.gc_max_level,
            root_pn: self.root_pn,
            error: self.error,
        }
    }

    #[inline]
    fn probe_and_bounded_simple<P: PositionBackend, H: SearchHeuristic<P>>(
        &mut self,
        child_pos: &P,
        thdata: TData,
        item: &mut MvItem,
        max_nodes: u64,
        heuristic: &mut H,
    ) {
        if self.suspend {
            return;
        }
        let saved_nl = self.config.node_limit;
        let cap = self.nodes.saturating_add(max_nodes);
        self.config.node_limit = Some(saved_nl.map_or(cap, |nl| nl.min(cap)));
        self.bn_search_and::<P, H, true>(child_pos, thdata, item, heuristic);
        self.config.node_limit = saved_nl;
        if self.suspend {
            let hit_nl = saved_nl.map_or(false, |nl| self.nodes >= nl);
            let hit_tl = self
                .config
                .time_limit_secs
                .map_or(false, |s| self.start.elapsed().as_secs_f64() >= s);
            self.suspend = hit_nl || hit_tl;
        }
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
            if let Some(flag) = &self.abort_flag {
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

    fn make_tree<P: PositionBackend, H: SearchHeuristic<P>>(
        &mut self,
        root: &P,
        mvlist: &mut MvItem,
        heuristic: &mut H,
    ) {
        for _ in 0..self.config.n_make_tree {
            if self.error || self.suspend {
                break;
            }
            self.tbase.clear_protect();
            if !mvlist.inc() {
                self.make_tree_or(root, mvlist, heuristic);
            } else {
                self.redundant = true;
            }
        }
    }

    #[inline(always)]
    fn ni_apply_hit<P: PositionBackend, const SIMPLE: bool>(
        &mut self,
        pos: &P,
        hit: NiHit,
        att_cur: MKey,
        def_cur: MKey,
        mvlist: &mut MvItem,
        tn: crate::shtsume::types::Turn,
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
        if let Some(ni) = self.ni_cache.as_mut() {
            ni.record_meta(ks.zkey, &fp);
        }
        self.tbase.update(&ks, mvlist, tn);
        true
    }

    #[inline]
    fn ni_try_lookup<P: PositionBackend, const SIMPLE: bool>(
        &mut self,
        pos: &P,
        th_tdata: TData,
        mvlist: &mut MvItem,
        tn: crate::shtsume::types::Turn,
    ) -> bool {
        if tn != crate::shtsume::types::Turn::Sente {
            return false;
        }
        let rhash = pos.ni_ring_hash();
        let matches = {
            let Some(ni) = self.ni_cache.as_mut() else {
                return false;
            };
            ni.probe_tag(rhash)
        };
        if matches == 0 {
            return false;
        }
        let planes = pos.ni_planes();
        let att_cur = pos.mkey(crate::shtsume::types::Turn::Sente);
        let def_cur = pos.mkey(crate::shtsume::types::Turn::Gote);
        let forbid_inc = !SIMPLE && pos.count() < 30;
        let hit = {
            let Some(ni) = self.ni_cache.as_mut() else {
                return false;
            };
            let Some(h) = ni.lookup_with_matches(
                rhash,
                matches,
                &planes,
                att_cur,
                def_cur,
                th_tdata.sh,
                forbid_inc,
            ) else {
                return false;
            };
            h
        };
        self.ni_apply_hit::<P, SIMPLE>(pos, hit, att_cur, def_cur, mvlist, tn)
    }

    #[inline]
    fn ni_try_lookup_with_planes<P: PositionBackend, const SIMPLE: bool>(
        &mut self,
        pos: &P,
        planes: &BoardPlanes,
        rhash: u64,
        th_tdata: TData,
        mvlist: &mut MvItem,
        tn: crate::shtsume::types::Turn,
    ) -> bool {
        if tn != crate::shtsume::types::Turn::Sente {
            return false;
        }
        let att_cur = pos.mkey(crate::shtsume::types::Turn::Sente);
        let def_cur = pos.mkey(crate::shtsume::types::Turn::Gote);
        let forbid_inc = !SIMPLE && pos.count() < 30;
        let hit = {
            let Some(ni) = self.ni_cache.as_mut() else {
                return false;
            };
            let matches = ni.probe_tag(rhash);
            if matches == 0 {
                return false;
            }
            let Some(h) = ni.lookup_with_matches(
                rhash,
                matches,
                planes,
                att_cur,
                def_cur,
                th_tdata.sh,
                forbid_inc,
            ) else {
                return false;
            };
            h
        };
        self.ni_apply_hit::<P, SIMPLE>(pos, hit, att_cur, def_cur, mvlist, tn)
    }

    #[inline]
    fn ni_record_or_proof<P: PositionBackend>(
        &mut self,
        pos: &P,
        mvlist: &MvItem,
        best_child: &MvItem,
        work: u64,
    ) {
        let Some(ni) = self.ni_cache.as_mut() else {
            return;
        };
        let child_ks = pos.key_forward_item(best_child);
        let Some(child_fp) = ni.lookup_meta(child_ks.zkey, child_ks.mkey[0], child_ks.mkey[1])
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
        ni.record_meta(pos.zkey(), &fp);
        if work >= MIN_NI_INSERT_WORK && fp.sh >= MIN_NI_INSERT_SH {
            let (planes, rhash) = pos.ni_planes_and_ring_hash();
            ni.insert(rhash, &planes, &fp, work);
        }
    }

    #[inline]
    fn ni_record_and_leaf<P: PositionBackend>(&mut self, pos: &P) {
        let Some(ni) = self.ni_cache.as_mut() else {
            return;
        };
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
        ni.record_meta(pos.zkey(), &fp);
    }

    #[inline]
    fn ni_record_and_proof<P: PositionBackend>(
        &mut self,
        pos: &P,
        mvlist: &MvItem,
        evasions: &[MvItem],
        work: u64,
    ) {
        let Some(ni) = self.ni_cache.as_mut() else {
            return;
        };
        let Some((mut mask, mut def_max)) = pos.ni_proof_and_local(evasions) else {
            return;
        };
        let mut att_min = MKey::EMPTY;
        let mut head_nouse = 0u8;
        let mut head_att_tot = 0u8;
        let mut head_hinc = false;
        for (idx, ev_item) in evasions.iter().enumerate() {
            let child_ks = pos.key_forward_item(ev_item);
            let Some(child_fp) = ni.lookup_meta(child_ks.zkey, child_ks.mkey[0], child_ks.mkey[1])
            else {
                return;
            };
            if idx == 0 {
                head_nouse = child_fp.nouse;
                head_att_tot = child_fp.att_min.total();
                head_hinc = child_fp.hinc;
            }
            let ev = ev_item.head_move();
            mask |= if ev.is_drop() {
                crate::shtsume::ni_cache::upgrade_ni_mask_for_white_drop(
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
        ni.record_meta(pos.zkey(), &fp);
        if work >= MIN_NI_INSERT_WORK && fp.sh >= MIN_NI_INSERT_SH {
            let (planes, rhash) = pos.ni_planes_and_ring_hash();
            ni.insert(rhash, &planes, &fp, work);
        }
    }

    #[inline]
    fn try_1ply_mate_or<P: PositionBackend, const SIMPLE: bool>(
        &mut self,
        pos: &P,
        mvlist: &mut MvItem,
        tn: crate::shtsume::types::Turn,
        ks: &crate::shtsume::types::KeyState,
    ) -> bool {
        let m1_opt = if SIMPLE {
            pos.find_1ply_mate_simple()
        } else {
            pos.find_1ply_mate()
        };
        let Some(m1) = m1_opt else {
            return false;
        };
        self.nodes += 1;
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
        self.tbase.update(&child_ks, &mut child_item, tn);

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
        self.tbase.update(ks, mvlist, tn);
        if P::SUPPORTS_NI && self.config.use_ni_cache && pos.count() > 0 {
            let child_pos = pos.move_forward(m1.mv);
            self.ni_record_and_leaf(&child_pos);
            self.ni_record_or_proof(pos, mvlist, &child_item, 0);
        }
        true
    }

    fn probe_or_bounded<P: PositionBackend, H: SearchHeuristic<P>, const SIMPLE: bool>(
        &mut self,
        or_pos: &P,
        thdata: TData,
        item: &mut MvItem,
        max_nodes: u64,
        heuristic: &mut H,
    ) {
        let saved_limit = self.config.node_limit;
        let saved_suspend = self.suspend;
        let cap = self.nodes.saturating_add(max_nodes);
        self.config.node_limit = Some(match saved_limit {
            Some(l) => l.min(cap),
            None => cap,
        });
        let saved_probe = self.in_cycle_probe;
        self.in_cycle_probe = true;
        self.bn_search_or::<P, H, SIMPLE>(or_pos, thdata, item, heuristic);
        self.in_cycle_probe = saved_probe;
        self.config.node_limit = saved_limit;
        if let Some(orig) = saved_limit {
            if self.nodes < orig {
                self.suspend = saved_suspend;
            }
        } else {
            self.suspend = saved_suspend;
        }
        self.check_limits();
    }

    /// Extract a compact DAG-memoized `CovariantCert` from `TBase` for a proven OR node `root_or`.
    fn extract_covariant_cert<P: PositionBackend, const SIMPLE: bool>(
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
        self.extract_cert_or_rec::<P, SIMPLE>(
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

    fn extract_cert_or_rec<P: PositionBackend, const SIMPLE: bool>(
        &mut self,
        pos: &P,
        cur_tracked: Option<u8>,
        tn: crate::shtsume::types::Turn,
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
        pos.generate_check_into(&mut self.tbase, &mut list);
        for item in list.iter_mut() {
            let cks = pos.key_forward_item(item);
            self.tbase.lookup(&cks, item, tn);
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
            // Sub-leaf closed via NiCache or dominance without expanded children in TBase
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
                and_pos.generate_evasion_simple_into(&mut self.tbase, &mut ev_list);
            } else {
                let _ = and_pos.generate_evasion_into(&mut self.tbase, &mut ev_list);
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
                if let Some(c_idx) = self.extract_cert_or_rec::<P, SIMPLE>(
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

    /// Translation-covariant proof verifier: replays `cert` onto `pos` (where the translating
    /// piece is now at `cur_tracked`), verifying 100% of legal evasions and writing exact
    /// proof metadata (`mkey, inc, hinc, nouse, nouse2, sh`) to `TBase` and `NiCache`.
    #[allow(clippy::too_many_arguments)]
    fn replay_covariant_or<P: PositionBackend, H: SearchHeuristic<P>, const SIMPLE: bool>(
        &mut self,
        pos: &P,
        cert: &CovariantCert,
        node_idx: u16,
        cur_tracked: Option<u8>,
        th_tdata: TData,
        out_item: &mut MvItem,
        rem_depth: u16,
        rem_fallback_nodes: &mut u64,
        heuristic: &mut H,
    ) -> bool {
        self.check_limits();
        let max_d = if SIMPLE {
            SIMPLE_MAX_DEPTH
        } else {
            TSUME_MAX_DEPTH
        };
        if self.suspend || rem_depth == 0 || pos.count() >= max_d - 1 || th_tdata.sh < 2 {
            return false;
        }
        let tn = pos.turn();
        let or_ks = pos.key_state();
        self.tbase.lookup(&or_ks, out_item, tn);
        if out_item.tdata.pn == 0 && out_item.tdata.sh < th_tdata.sh {
            return true;
        }
        if out_item.tdata.dn == 0 {
            return false;
        }
        if P::SUPPORTS_NI
            && self.config.use_ni_cache
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
                let n0 = self.nodes;
                self.probe_or_bounded::<P, H, SIMPLE>(pos, th_tdata, out_item, b, heuristic);
                *rem_fallback_nodes = rem_fallback_nodes.saturating_sub(self.nodes.saturating_sub(n0));
                return out_item.tdata.pn == 0 && out_item.tdata.sh < th_tdata.sh;
            }
            return false;
        }

        let best_mv = node.best_mv;
        let moved_tracked = node.moved_tracked;
        let orig_tracked = node.orig_tracked;
        let edges = node.edges.clone();

        let mut check_list = self.acquire_list();
        pos.generate_check_into(&mut self.tbase, &mut check_list);
        self.nodes += 1;
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
            // (a) Ray-convergent move: `cur_src -> orig_dest` (same destination near King!)
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
            // (b) Parallel-shifted move: `cur_src -> orig_dest + (cur_src - orig_src)`
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
            // (c) Any other legal check from `cur_src` (at most 1-2 rays)
            for (idx, it) in check_list.iter().enumerate() {
                let m = it.head_move();
                if m.is_board_move() && m.prev_pos == cur_src {
                    push_unique(idx, &mut cand_indices);
                }
            }
        }

        for &c_idx in &cand_indices {
            if self.suspend {
                break;
            }
            let mut cand_item = check_list[c_idx].clone();
            let cand_mv = cand_item.head_move();
            let and_ks = pos.key_forward_item(&cand_item);
            self.tbase.lookup(&and_ks, &mut cand_item, tn);
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
                && self.config.use_ni_cache
                && self.ni_try_lookup::<P, SIMPLE>(&and_pos, and_th, &mut cand_item, tn)
            {
                // Proven via NiCache
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
                    and_pos.generate_evasion_simple_into(&mut self.tbase, &mut ev_list);
                    (false, false)
                } else {
                    and_pos.generate_evasion_into(&mut self.tbase, &mut ev_list)
                };
                self.nodes += 1;

                if ev_list.is_empty() {
                    if SIMPLE {
                        cand_item.tdata = TData::TSUMI;
                        cand_item.mkey = proof_koma(&and_pos);
                    } else {
                        Self::tsumi_proof(&and_pos, &mut cand_item, proof_flag);
                    }
                    self.tbase.update(&and_ks, &mut cand_item, tn);
                    if P::SUPPORTS_NI && self.config.use_ni_cache && !proof_flag && !invalid_flag {
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
                        self.tbase.lookup(&cks, ev_item, tn);
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
                            && self.config.use_ni_cache
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
                            let _ = self.replay_covariant_or::<P, H, SIMPLE>(
                                &cpos,
                                cert,
                                edge.child_idx,
                                next_tr,
                                cth,
                                ev_item,
                                rem_depth - 1,
                                rem_fallback_nodes,
                                heuristic,
                            );
                        }
                        if ev_item.tdata.pn != 0
                            && ev_item.tdata.dn != 0
                            && *rem_fallback_nodes >= 16
                        {
                            let b = (*rem_fallback_nodes).min(512);
                            let n0 = self.nodes;
                            self.probe_or_bounded::<P, H, SIMPLE>(
                                &cpos,
                                cth,
                                ev_item,
                                b,
                                heuristic,
                            );
                            *rem_fallback_nodes =
                                rem_fallback_nodes.saturating_sub(self.nodes.saturating_sub(n0));
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
                            Self::tsumi_proof(&and_pos, &mut cand_item, proof_flag);
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
                        self.tbase.update(&and_ks, &mut cand_item, tn);
                        if P::SUPPORTS_NI
                            && self.config.use_ni_cache
                            && !proof_flag
                            && !invalid_flag
                        {
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
                self.tbase.update(&or_ks, out_item, tn);
                if P::SUPPORTS_NI && self.config.use_ni_cache && pos.count() > 0 {
                    self.ni_record_or_proof(pos, out_item, &cand_item, MIN_NI_INSERT_WORK);
                }
                self.release_list(check_list);
                return true;
            }
        }

        self.release_list(check_list);
        false
    }

    /// Online periodic cycle fast-forward + Covariant Proof Replay + Soft-Corridor Guidance.
    /// Returns `true` iff `pos` itself was fully proven (`pn == 0`) all the way back from `P_r`.
    fn try_cycle_skip<P: PositionBackend, H: SearchHeuristic<P>, const SIMPLE: bool>(
        &mut self,
        pos: &P,
        planes: &BoardPlanes,
        rhash: u64,
        th_tdata: TData,
        mvlist: &mut MvItem,
        heuristic: &mut H,
    ) -> bool {
        let cur_nodes = self.nodes;
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
                // Cap total probe + backup effort to <= 25% of search nodes (after 50K warmup)
                let spent = ct.stats.probe_nodes + ct.stats.backup_nodes;
                if cur_nodes > 50_000 && spent * 4 > cur_nodes {
                    return false;
                }
                let Some(d) = ct.detect_cycle(pos, planes, rhash, cur_nodes) else {
                    return false;
                };
                if d.sigma.len() < 16 && cur_nodes > 30_000 && spent * 10 > cur_nodes {
                    return false;
                }
                d
            };
            let l_try = det.sigma.len();
            let (cycles, boundary_pos) = rollout_corridor(pos, &det, max_d);
            let total_plies = cycles.len() * l_try;
            if cycles.is_empty() || total_plies < 16 {
                if let Some(ct) = self.cycle_tracker.as_mut() {
                    let cd = if l_try < 16 { 45_000 } else { 20_000 };
                    ct.set_pattern_cooldown(
                        l_try,
                        det.kind,
                        det.shift,
                        det.cur_ply,
                        self.nodes + cd,
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
            100_000u64
        } else if l >= 16 {
            28_000u64
        } else {
            6_000u64
        };

        // Probe furthest boundary `P_r` first (`P_r, P_{r-1}, ...`).
        // On Type I (`kind.is_hand_monotone()`), hand monotonicity guarantees `P_r` is strictly
        // easier than `P_{r-1}`. On Type II / Type III (`!kind.is_hand_monotone()`, e.g. hand
        // conversion `2P -> 2L -> B` or piece translation/peeling), `P_r` may have over-converted
        // or over-rolled past the mating state, so also allow stepping back one boundary on unknown.
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
            if self.suspend {
                break;
            }
            let bks = bpos.key_state();
            let mut bitem = MvItem::empty_root();
            self.tbase.lookup(&bks, &mut bitem, tn);
            if bitem.tdata.pn != 0 && bitem.tdata.dn != 0 {
                let bth = TData {
                    pn: PROOF_MAX - 1,
                    dn: INFINATE - 1,
                    sh: max_d.saturating_sub(bpos.count() + 1),
                };
                if !(P::SUPPORTS_NI
                    && self.config.use_ni_cache
                    && self.ni_try_lookup::<P, SIMPLE>(&bpos, bth, &mut bitem, tn))
                {
                    let bud = if unknown_stepbacks == 0 {
                        base_budget
                    } else {
                        base_budget / 2
                    };
                    let n0 = self.nodes;
                    self.probe_or_bounded::<P, H, SIMPLE>(&bpos, bth, &mut bitem, bud, heuristic);
                    let used = self.nodes.saturating_sub(n0);
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
                let cd = if l < 16 { 120_000 } else { 70_000 };
                ct.set_pattern_cooldown(l, kind, shift, cur_ply, self.nodes + cd);
            }
            return false;
        };

        // ---- Step 1: Build explicit `(or_pos, or_mv, and_pos, ev_mv, tr_and)` steps ----
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

        // ---- Step 2: Backward Closure (`proven_idx` down to `0`) with CovariantProofReplay ----
        let backup_n0 = self.nodes;
        let is_multi_long = proven_idx >= 1 && l >= 40;
        let template_side_budget: u64 = if is_multi_long {
            32_000
        } else if proven_idx >= 1 {
            14_000
        } else {
            6_000
        };
        let max_backup_nodes: u64 = if is_multi_long {
            450_000
        } else if proven_idx >= 1 {
            160_000
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
            if self.suspend || (!chain_intact && c < active_top) || (refuted_cycle && c < active_top) {
                break;
            }
            if self.nodes.saturating_sub(backup_n0) >= max_backup_nodes {
                chain_intact = false;
                break;
            }
            let pairs = &step_data[c];
            for p_idx in (0..pairs.len()).rev() {
                if self.suspend || self.nodes.saturating_sub(backup_n0) >= max_backup_nodes {
                    chain_intact = false;
                    break 'outer_bak;
                }
                let (ref or_pos, or_mv, ref and_pos, ev_mv, tr_and) = pairs[p_idx];
                let and_ks = and_pos.key_state();

                // Fast check if AND node is already proven in TBase or NiCache
                let mut and_item = MvItem::single_with_hints(
                    or_mv,
                    if or_mv.is_board_move() {
                        or_pos.board_at(or_mv.prev_pos as usize)
                    } else {
                        0
                    },
                    or_pos.board_at(or_mv.dest()),
                );
                self.tbase.lookup(&and_ks, &mut and_item, tn);
                if and_item.tdata.pn != 0
                    && P::SUPPORTS_NI
                    && self.config.use_ni_cache
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
                    // Proven via NiCache!
                }

                if and_item.tdata.pn != 0 {
                    let mut ev_list = self.acquire_list();
                    let (proof_flag, invalid_flag) = if SIMPLE {
                        and_pos.generate_evasion_simple_into(&mut self.tbase, &mut ev_list);
                        (false, false)
                    } else {
                        and_pos.generate_evasion_into(&mut self.tbase, &mut ev_list)
                    };
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
                        self.tbase.lookup(&cks, ev_item, tn);
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
                        // 1) Try NiCache
                        if P::SUPPORTS_NI
                            && self.config.use_ni_cache
                            && self.ni_try_lookup::<P, SIMPLE>(&cpos, cth, ev_item, tn)
                        {
                            continue;
                        }
                        // 2) Try immediate 1-ply mate
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

                        // 3) For earlier cycles `c < active_top`, try CovariantProofReplay!
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
                                if self.replay_covariant_or::<P, H, SIMPLE>(
                                    &cpos,
                                    &cert_clone,
                                    0,
                                    next_tr,
                                    cth,
                                    ev_item,
                                    160,
                                    &mut rem_fb,
                                    heuristic,
                                ) {
                                    if let Some(ct) = self.cycle_tracker.as_mut() {
                                        ct.stats.covariant_replays_ok += 1;
                                    }
                                    if c > 0 {
                                        if let Some(updated_cert) = self
                                            .extract_covariant_cert::<P, SIMPLE>(
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

                        // 4) Two-stage bounded df-pn probe
                        let stage1_budget = if c == active_top {
                            4_000u64.min(template_side_budget)
                        } else {
                            replay_miss_side_budget
                        };
                        self.probe_or_bounded::<P, H, SIMPLE>(
                            &cpos,
                            cth,
                            ev_item,
                            stage1_budget,
                            heuristic,
                        );
                        if c == active_top
                            && ev_item.tdata.pn > 0
                            && ev_item.tdata.dn > 0
                            && template_side_budget > stage1_budget
                            && !(ev_item.tdata.pn >= 9 && ev_item.tdata.dn >= 45)
                        {
                            self.probe_or_bounded::<P, H, SIMPLE>(
                                &cpos,
                                cth,
                                ev_item,
                                template_side_budget - stage1_budget,
                                heuristic,
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
                        // Extract CovariantCert so earlier cycles `c - 1 .. 0` replay in O(|Cert|)!
                        if c > 0 {
                            if let Some(cert) = self.extract_covariant_cert::<P, SIMPLE>(
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
                        // Over-roll fallback for Type II (`shift != (0, 0)`): if cycle `c == active_top`
                        // failed (e.g. translating piece stepped into capture range at `c`), test if
                        // stopping BEFORE cycle `c` (`cycles[c].0`) is already `pn == 0`!
                        if c == active_top
                            && c > 0
                            && !kind.is_hand_monotone()
                            && !self.suspend
                            && self.nodes.saturating_sub(backup_n0) < max_backup_nodes
                        {
                            let prev_bpos = &cycles[c].0;
                            let prev_ks = prev_bpos.key_state();
                            let mut prev_bitem = MvItem::empty_root();
                            self.tbase.lookup(&prev_ks, &mut prev_bitem, tn);
                            if prev_bitem.tdata.pn != 0 && prev_bitem.tdata.dn != 0 {
                                let pbth = TData {
                                    pn: PROOF_MAX - 1,
                                    dn: INFINATE - 1,
                                    sh: max_d.saturating_sub(prev_bpos.count() + 1),
                                };
                                if !(P::SUPPORTS_NI
                                    && self.config.use_ni_cache
                                    && self.ni_try_lookup::<P, SIMPLE>(
                                        prev_bpos,
                                        pbth,
                                        &mut prev_bitem,
                                        tn,
                                    ))
                                {
                                    let n0 = self.nodes;
                                    self.probe_or_bounded::<P, H, SIMPLE>(
                                        prev_bpos,
                                        pbth,
                                        &mut prev_bitem,
                                        8_000,
                                        heuristic,
                                    );
                                    let used = self.nodes.saturating_sub(n0);
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
                            Self::tsumi_proof(and_pos, &mut and_item, proof_flag);
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
                        self.tbase.update(&and_ks, &mut and_item, tn);
                        if P::SUPPORTS_NI
                            && self.config.use_ni_cache
                            && !proof_flag
                            && !invalid_flag
                        {
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
                    // Close parent OR node `or_pos` to `pn == 0` in TBase + NiCache!
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
                    self.tbase.update(&or_ks, &mut or_item, tn);
                    if P::SUPPORTS_NI && self.config.use_ni_cache && or_pos.count() > 0 {
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

        // Register Soft-Corridor hints on non-refuted cycles up to `active_top`
        if !chain_intact && !refuted_cycle && !likely_escape && (active_top + 1) * l >= 16 {
            if let Some(ct) = self.cycle_tracker.as_mut() {
                for pairs in &step_data[..=active_top] {
                    for &(ref op, omv, _, _, _) in pairs {
                        ct.register_corridor_move(op.zkey(), op.mkey(tn), omv);
                    }
                }
            }
        }

        let used_bak = self.nodes.saturating_sub(backup_n0);
        if let Some(ct) = self.cycle_tracker.as_mut() {
            ct.stats.backup_nodes += used_bak;
            let cd = if refuted_cycle || likely_escape {
                400_000
            } else {
                200_000
            };
            ct.set_pattern_cooldown(l, kind, shift, cur_ply, self.nodes + cd);
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
    fn bn_search_or<P: PositionBackend, H: SearchHeuristic<P>, const SIMPLE: bool>(
        &mut self,
        pos: &P,
        th_tdata: TData,
        mvlist: &mut MvItem,
        heuristic: &mut H,
    ) {
        let tn = pos.turn();
        let max_d = if SIMPLE {
            SIMPLE_MAX_DEPTH
        } else {
            TSUME_MAX_DEPTH
        };
        let want_ni = P::SUPPORTS_NI
            && self.config.use_ni_cache
            && pos.count() > 0
            && th_tdata.pn >= 2
            && th_tdata.sh >= 2
            && pos.count() < max_d - 1;
        let want_cyc = P::SUPPORTS_NI
            && self.config.use_cycle_skip
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
                && self.try_cycle_skip::<P, H, SIMPLE>(
                    pos, &planes, rhash, th_tdata, mvlist, heuristic,
                )
            {
                return;
            }
            if let Some(ct) = self.cycle_tracker.as_mut() {
                ct.push_or_frame(
                    pos.count(),
                    pos.zkey(),
                    rhash,
                    planes,
                    pos.mkey(crate::shtsume::types::Turn::Sente),
                    pos.mkey(crate::shtsume::types::Turn::Gote),
                );
                pushed_cycle_frame = true;
            }
        } else if want_ni && self.ni_try_lookup::<P, SIMPLE>(pos, th_tdata, mvlist, tn) {
            return;
        }

        let mut list = self.acquire_list();
        self.bn_search_or_inner::<P, H, SIMPLE>(pos, th_tdata, mvlist, &mut list, heuristic);
        self.release_list(list);

        if pushed_cycle_frame {
            if let Some(ct) = self.cycle_tracker.as_mut() {
                ct.pop_or_frame();
            }
        }
    }

    fn bn_search_or_inner<P: PositionBackend, H: SearchHeuristic<P>, const SIMPLE: bool>(
        &mut self,
        pos: &P,
        th_tdata: TData,
        mvlist: &mut MvItem,
        list: &mut Vec<MvItem>,
        heuristic: &mut H,
    ) {
        self.check_limits();
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
        let entry_nodes = self.nodes;

        if self.config.use_1ply_mate
            && pos.count() > 0
            && th_tdata.pn <= 2
            && th_tdata.sh >= 2
            && self.try_1ply_mate_or::<P, SIMPLE>(pos, mvlist, tn, &ks)
        {
            return;
        }

        pos.generate_check_into(&mut self.tbase, list);
        self.nodes += 1;

        if list.is_empty() {
            mvlist.tdata = TData::FUDUMI;
            mvlist.mkey = disproof_koma(pos);
            self.tbase.update(&ks, mvlist, tn);
            return;
        }

        // Initial horizontal symmetry filter (files 1..=5, i.e. file index < 5)
        if pos.count() == 0 && self.symmetry {
            list.retain(|item| sq_file(item.head_move().dest()) < 5);
            if list.is_empty() {
                mvlist.tdata = TData::FUDUMI;
                mvlist.mkey = disproof_koma(pos);
                self.tbase.update(&ks, mvlist, tn);
                return;
            }
        }

        let enemy_ou = pos.enemy_ou();

        let n = list.len();
        let mut ks0 = pos.key_forward_item(&list[0]);
        self.tbase.prefetch(ks0.zkey);
        let mut ks1 = if n > 1 {
            let k = pos.key_forward_item(&list[1]);
            self.tbase.prefetch(k.zkey);
            k
        } else {
            ks0
        };
        let mut ks2 = if n > 2 {
            let k = pos.key_forward_item(&list[2]);
            self.tbase.prefetch(k.zkey);
            k
        } else {
            ks1
        };
        if n > 1 {
            self.tbase.prefetch_zfolder(ks0.zkey);
        }
        let mut has_mate = false;
        let mut has_fudumi = false;
        let mut all_unsearched = true;
        let mut min_pn: u16 = INFINATE;
        let mut max_dn: u16 = 0;
        for i in 0..n {
            if i + 3 < n {
                let ks3 = pos.key_forward_item(&list[i + 3]);
                self.tbase.prefetch(ks3.zkey);
                self.tbase.prefetch_zfolder(ks1.zkey);
                self.tbase.lookup(&ks0, &mut list[i], tn);
                ks0 = ks1;
                ks1 = ks2;
                ks2 = ks3;
            } else {
                if i + 1 < n {
                    self.tbase.prefetch_zfolder(ks1.zkey);
                }
                self.tbase.lookup(&ks0, &mut list[i], tn);
                ks0 = ks1;
                ks1 = ks2;
            }
            let item = &mut list[i];
            if item.tdata.pn == 0 {
                if SIMPLE {
                    mvlist.tdata = TData {
                        pn: 0,
                        dn: INFINATE,
                        sh: item.tdata.sh.saturating_add(1),
                    };
                    proof_koma_or_simple(mvlist, item);
                    self.tbase.update(&ks, mvlist, tn);
                    if P::SUPPORTS_NI && self.config.use_ni_cache && pos.count() > 0 {
                        let work = self.nodes.saturating_sub(entry_nodes);
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

        // Fast cutoff before ML scoring & sorting when this OR probe is guaranteed to
        // exceed `th_tdata` immediately without expanding any child.
        if pos.count() > 0
            && all_unsearched
            && !has_mate
            && !has_fudumi
            && !H::HAS_PN_FLOOR
            && !H::HAS_EDGE_DELTA
        {
            let dn_val = (INFINATE - 1).min(max_dn.saturating_add((n - 1) as u16));
            if dn_val >= th_tdata.dn || min_pn >= th_tdata.pn || 1 >= th_tdata.sh {
                // Before exiting as `unknown` (especially under `dn` choking from side evasions),
                // check for an immediate 1-ply mate so trivial evasions clear instead of inflating `pcnt`.
                if th_tdata.sh >= 2 && self.try_1ply_mate_or::<P, SIMPLE>(pos, mvlist, tn, &ks) {
                    return;
                }
                mvlist.tdata = TData {
                    pn: min_pn,
                    dn: dn_val,
                    sh: 1,
                };
                mvlist.set_inc(false);
                self.tbase.update(&ks, mvlist, tn);
                return;
            }
        }

        if self.config.use_ml_ordering {
            heuristic.score_or_moves_ctx(pos, mvlist.head_move(), self.tbase.gc_num, list);
        }
        if H::HAS_PN_FLOOR {
            for item in list.iter_mut() {
                let fl = item.pn_delta as u16;
                if fl > 1 && item.tdata.pn > 0 && item.tdata.pn < fl {
                    item.tdata.pn = fl;
                }
            }
        }
        if H::HAS_EDGE_DELTA {
            for item in list.iter_mut() {
                if item.pn_delta > 0 && item.tdata.pn > 0 && item.tdata.pn <= EDGE_PN_CEIL {
                    item.tdata.pn = item
                        .tdata
                        .pn
                        .saturating_add(item.pn_delta as u16)
                        .min(EDGE_PN_CEIL);
                }
            }
        }
        if SIMPLE {
            sort_or_simple(list);
        } else {
            sort_or(list);
        }

        let mut expanded_any = false;
        let mut corridor_mv = if !self.in_cycle_probe {
            self.cycle_tracker
                .as_mut()
                .and_then(|ct| ct.lookup_corridor(ks.zkey, ks.mkey[0]))
        } else {
            None
        };

        loop {
            // Check for 1-ply non-redundant mate when redundant mate > 1 ply is found
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
                    if !item.search() {
                        let child_pos = pos.move_forward(item.head_move());
                        self.bn_search_and::<P, H, false>(&child_pos, c_th, item, heuristic);
                        item.set_search(true);
                        if H::HAS_PN_FLOOR {
                            let fl = item.pn_delta as u16;
                            if fl > 1 && item.tdata.pn > 0 && item.tdata.pn < fl {
                                item.tdata.pn = fl;
                            }
                        }
                    }
                }
                sort_or(list);
            }

            mvlist.tdata.pn = list[0].tdata.pn;
            if pos.count() == 0 {
                self.root_pn = mvlist.tdata.pn;
                self.tbase.root_pn = self.root_pn;
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

            if mvlist.tdata.pn >= th_tdata.pn
                || mvlist.tdata.dn >= th_tdata.dn
                || mvlist.tdata.sh >= th_tdata.sh
                || self.suspend
            {
                if !expanded_any
                    && !self.suspend
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
                let ks = pos.key_state();
                self.tbase.update(&ks, mvlist, tn);
                if mvlist.tdata.pn == 0
                    && P::SUPPORTS_NI
                    && self.config.use_ni_cache
                    && pos.count() > 0
                {
                    let work = self.nodes.saturating_sub(entry_nodes);
                    self.ni_record_or_proof(pos, mvlist, &list[0], work);
                }
                return;
            }

            // Soft-Corridor Guidance: bounded 2x boost for proven-boundary corridor moves
            // (only when corridor pn <= 2 * best pn, preserving strict df-pn progress `c_pn > pn`).
            let mut on_corridor_boost = false;
            if let Some(cmv) = corridor_mv {
                if let Some(cidx) = list.iter().position(|it| it.head_move() == cmv) {
                    if list[cidx].tdata.dn == 0 || list[cidx].cu() {
                        if let Some(ct) = self.cycle_tracker.as_mut() {
                            ct.invalidate_corridor(ks.zkey, ks.mkey[0]);
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
                        }
                    }
                } else {
                    corridor_mv = None;
                }
            }

            let d_or = if H::HAS_EDGE_DELTA {
                list[0].pn_delta as u16
            } else {
                0
            };

            let c_threshold = if list.len() > 1 {
                let next_pn = if on_corridor_boost {
                    let cur_pn = list[0].tdata.pn;
                    let s1 = list[1].tdata.pn;
                    let boosted = s1.saturating_mul(2).max(s1.saturating_add(4));
                    boosted
                        .max(cur_pn.saturating_add(1).saturating_add(d_or))
                        .min(INFINATE)
                } else if list[0].search() || (H::HAS_PN_FLOOR && list[0].pn_delta > 1) {
                    let s1 = list[1].tdata.pn;
                    let eps_shift = if self.config.eps_pn_shift > 0 {
                        self.config.eps_pn_shift
                    } else if pos.count() >= DEEP_CORRIDOR_DEPTH {
                        3
                    } else {
                        0
                    };
                    let step = if eps_shift > 0 {
                        1u16.saturating_add(s1 >> eps_shift)
                    } else {
                        1
                    };
                    s1.saturating_add(step).min(INFINATE)
                } else {
                    let first_pn = if pos.count() >= DEEP_CORRIDOR_DEPTH {
                        2u16
                    } else {
                        1u16
                    };
                    first_pn.saturating_add(d_or).min(INFINATE)
                };
                let c_pn = th_tdata.pn.min(next_pn);
                let c_dn = if th_tdata.dn == INFINATE - 1 {
                    INFINATE - 1
                } else if pos.count() > 0 {
                    th_tdata.dn.saturating_sub(dcnt)
                } else {
                    th_tdata.dn
                };
                TData {
                    pn: c_pn,
                    dn: c_dn,
                    sh: th_tdata.sh.saturating_sub(1),
                }
            } else {
                TData {
                    pn: th_tdata.pn,
                    dn: th_tdata.dn,
                    sh: th_tdata.sh.saturating_sub(1),
                }
            };

            mvlist.tdata = c_threshold;
            if c_threshold.pn < PROOF_MAX - 1 {
                self.max_thpn = self.max_thpn.max(c_threshold.pn);
            }
            list[0].set_search(true);
            expanded_any = true;

            let child_th = if H::HAS_EDGE_DELTA && d_or > 0 && c_threshold.pn <= EDGE_PN_CEIL {
                TData {
                    pn: c_threshold.pn.saturating_sub(d_or).max(1),
                    dn: c_threshold.dn,
                    sh: c_threshold.sh,
                }
            } else {
                c_threshold
            };

            let old_pn0 = list[0].tdata.pn;
            let old_dn0 = list[0].tdata.dn;
            let nodes_before = self.nodes;
            let ks = pos.key_state();
            let current = self.tbase.set_current(&ks, mvlist, tn);
            let chosen_mv = list[0].head_move();
            let child_pos = pos.move_forward(chosen_mv);
            let pushed_mv = if !self.in_cycle_probe {
                if let Some(ct) = self.cycle_tracker.as_mut() {
                    ct.push_move(chosen_mv);
                    true
                } else {
                    false
                }
            } else {
                false
            };
            self.bn_search_and::<P, H, SIMPLE>(&child_pos, child_th, &mut list[0], heuristic);
            if pushed_mv {
                if let Some(ct) = self.cycle_tracker.as_mut() {
                    ct.pop_move();
                }
            }
            self.tbase.clear_current(current);
            if H::HAS_ONLINE_LEARNING {
                heuristic.on_or_step(
                    pos,
                    mvlist.head_move(),
                    list,
                    old_pn0,
                    old_dn0,
                    self.nodes.saturating_sub(nodes_before),
                );
            }

            if SIMPLE && list[0].tdata.pn == 0 {
                mvlist.tdata = TData {
                    pn: 0,
                    dn: INFINATE,
                    sh: list[0].tdata.sh.saturating_add(1),
                };
                proof_koma_or_simple(mvlist, &list[0]);
                self.tbase.update(&ks, mvlist, tn);
                return;
            }

            if H::HAS_PN_FLOOR {
                let fl = list[0].pn_delta as u16;
                if fl > 1 && list[0].tdata.pn > 0 && list[0].tdata.pn < fl {
                    list[0].tdata.pn = fl;
                }
            }
            if H::HAS_EDGE_DELTA
                && d_or > 0
                && list[0].tdata.pn > 0
                && list[0].tdata.pn <= EDGE_PN_CEIL
            {
                list[0].tdata.pn = list[0]
                    .tdata
                    .pn
                    .saturating_add(d_or)
                    .min(EDGE_PN_CEIL);
            }

            if SIMPLE {
                reorder_first_or_simple(list);
            } else {
                reorder_first_or(list);
            }
        }
    }

    #[inline]
    fn bn_search_and<P: PositionBackend, H: SearchHeuristic<P>, const SIMPLE: bool>(
        &mut self,
        pos: &P,
        th_tdata: TData,
        mvlist: &mut MvItem,
        heuristic: &mut H,
    ) {
        let mut list = self.acquire_list();
        self.bn_search_and_inner::<P, H, SIMPLE>(pos, th_tdata, mvlist, &mut list, heuristic);
        self.release_list(list);
    }

    fn bn_search_and_inner<P: PositionBackend, H: SearchHeuristic<P>, const SIMPLE: bool>(
        &mut self,
        pos: &P,
        th_tdata: TData,
        mvlist: &mut MvItem,
        list: &mut Vec<MvItem>,
        heuristic: &mut H,
    ) {
        let ryuma_flag = mvlist.length > 2;
        let tn = pos.turn().flip();
        let ks = pos.key_state();
        let entry_nodes = self.nodes;

        if P::SUPPORTS_NI
            && self.config.use_ni_cache
            && th_tdata.pn >= 2
            && th_tdata.sh >= 2
            && self.ni_try_lookup::<P, SIMPLE>(pos, th_tdata, mvlist, tn)
        {
            return;
        }

        let (proof_flag, invalid_flag) = if SIMPLE {
            pos.generate_evasion_simple_into(&mut self.tbase, list);
            (false, false)
        } else {
            pos.generate_evasion_into(&mut self.tbase, list)
        };
        self.nodes += 1;

        if list.is_empty() {
            if SIMPLE {
                mvlist.tdata = TData::TSUMI;
                mvlist.mkey = proof_koma(pos);
            } else {
                Self::tsumi_proof(pos, mvlist, proof_flag);
            }
            self.tbase.update(&ks, mvlist, tn);
            if P::SUPPORTS_NI && self.config.use_ni_cache && !proof_flag && !invalid_flag {
                self.ni_record_and_leaf(pos);
            }
            return;
        }

        // Lookup children and expand sequential interpositions that are already proven mate
        let mut i = 0;
        let mut all_fresh = !ryuma_flag;
        if list.len() > 1 {
            let cks0 = pos.key_forward_item(&list[0]);
            self.tbase.prefetch(cks0.zkey);
        }
        while i < list.len() {
            if i + 1 < list.len() {
                let cks_next = pos.key_forward_item(&list[i + 1]);
                self.tbase.prefetch(cks_next.zkey);
            }
            let child_ks = pos.key_forward_item(&list[i]);
            self.tbase.lookup(&child_ks, &mut list[i], tn);
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
                    self.tbase.update(&ks, mvlist, tn);
                    return;
                }
                all_fresh = false;
            }
            list[i].sort_key = (1u64 << 16) | 1u64;
            i += 1;
        }

        // Fast cutoff before ML scoring & sorting when all evasions are fresh TT misses
        // and `pn = list.len()` already meets `th_tdata.pn` (e.g. every `th_tdata.pn == 1` probe!).
        if all_fresh && !H::HAS_EDGE_DELTA && list.len() <= (PRE_PROOF_MAX as usize) {
            let pn_val = list.len() as u16;
            if pn_val >= th_tdata.pn || 1 >= th_tdata.dn || 1 >= th_tdata.sh {
                mvlist.tdata = TData {
                    pn: pn_val,
                    dn: 1,
                    sh: 1,
                };
                self.tbase.update(&ks, mvlist, tn);
                return;
            }
        }

        if self.config.use_ml_ordering {
            heuristic.score_and_moves_ctx(pos, mvlist.head_move(), self.tbase.gc_num, list);
        }
        if H::HAS_EDGE_DELTA {
            for item in list.iter_mut() {
                let dp = item.pn_delta;
                if dp > 0 && item.tdata.pn > 0 && item.tdata.pn <= EDGE_PN_CEIL {
                    item.tdata.pn = item
                        .tdata
                        .pn
                        .saturating_add(dp as u16)
                        .min(EDGE_PN_CEIL);
                }
                let d = item.dn_delta();
                if d > 0 && item.tdata.dn > 0 && item.tdata.dn <= EDGE_DN_CEIL {
                    item.tdata.dn = item
                        .tdata
                        .dn
                        .saturating_add(d as u16)
                        .min(EDGE_DN_CEIL);
                }
            }
        }
        if SIMPLE {
            sort_and_simple(list);
        } else {
            sort_and(list);
        }

        loop {
            // Ryuma middle-interposition folding
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
                Self::tsumi_proof(pos, mvlist, proof_flag);
                let ks = pos.key_state();
                self.tbase.update(&ks, mvlist, tn);
                return;
            }

            let pcnt;
            loop {
                mvlist.tdata.dn = list[0].tdata.dn;
                let (pn_val, p) = proof_number(&list);
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
                    // GHI (pn growth) mitigation: fold max-pn move into head
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
                    // GHI (dn growth) mitigation: fold second move into head
                    let second = list.remove(1);
                    list[0].fold_moves_from(second);
                } else {
                    pcnt = p;
                    break;
                }
            }

            if mvlist.tdata.pn >= th_tdata.pn
                || mvlist.tdata.dn >= th_tdata.dn
                || mvlist.tdata.sh >= th_tdata.sh
                || self.suspend
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
                        proof_koma_and(pos, mvlist, &list);
                    }
                }
                let ks = pos.key_state();
                self.tbase.update(&ks, mvlist, tn);
                if mvlist.tdata.pn == 0
                    && P::SUPPORTS_NI
                    && self.config.use_ni_cache
                    && !proof_flag
                    && !invalid_flag
                {
                    let work = self.nodes.saturating_sub(entry_nodes);
                    self.ni_record_and_proof(pos, mvlist, list, work);
                }
                return;
            }

            let c_threshold = if list.len() > 1 {
                let s1_dn = list[1].tdata.dn;
                let edn = if self.config.eps_dn_shift > 0 {
                    self.config.eps_dn_shift
                } else if self.config.use_ml_ordering && self.tbase.gc_num < 2 {
                    P::DEFAULT_EPS_DN_SHIFT
                } else {
                    0
                };
                let dn_step = if edn > 0 {
                    1u16.saturating_add(s1_dn >> edn)
                } else {
                    1
                };
                TData {
                    dn: th_tdata.dn.min(s1_dn.saturating_add(dn_step)),
                    pn: th_tdata.pn.saturating_sub(pcnt),
                    sh: th_tdata.sh.saturating_sub(1),
                }
            } else {
                TData {
                    pn: th_tdata.pn,
                    dn: th_tdata.dn,
                    sh: th_tdata.sh.saturating_sub(1),
                }
            };

            let d_and = if H::HAS_EDGE_DELTA {
                list[0].dn_delta() as u16
            } else {
                0
            };
            let dp_and = if H::HAS_EDGE_DELTA {
                list[0].pn_delta as u16
            } else {
                0
            };

            mvlist.tdata = c_threshold;
            let child_th = if H::HAS_EDGE_DELTA && (d_and > 0 || dp_and > 0) {
                TData {
                    pn: if dp_and > 0 && c_threshold.pn <= EDGE_PN_CEIL {
                        c_threshold.pn.saturating_sub(dp_and).max(1)
                    } else {
                        c_threshold.pn
                    },
                    dn: if d_and > 0 && c_threshold.dn <= EDGE_DN_CEIL {
                        c_threshold.dn.saturating_sub(d_and).max(1)
                    } else {
                        c_threshold.dn
                    },
                    sh: c_threshold.sh,
                }
            } else {
                c_threshold
            };
            let old_pn0 = list[0].tdata.pn;
            let old_dn0 = list[0].tdata.dn;
            let nodes_before = self.nodes;
            let ks = pos.key_state();
            let current = self.tbase.set_current(&ks, mvlist, tn);
            let chosen_mv = list[0].head_move();
            let child_pos = pos.move_forward(chosen_mv);
            let pushed_mv = if !self.in_cycle_probe {
                if let Some(ct) = self.cycle_tracker.as_mut() {
                    ct.push_move(chosen_mv);
                    true
                } else {
                    false
                }
            } else {
                false
            };
            self.bn_search_or::<P, H, SIMPLE>(&child_pos, child_th, &mut list[0], heuristic);
            if pushed_mv {
                if let Some(ct) = self.cycle_tracker.as_mut() {
                    ct.pop_move();
                }
            }
            self.tbase.clear_current(current);
            if H::HAS_ONLINE_LEARNING {
                heuristic.on_and_step(
                    pos,
                    mvlist.head_move(),
                    list,
                    old_pn0,
                    old_dn0,
                    self.nodes.saturating_sub(nodes_before),
                );
            }

            if SIMPLE && list[0].tdata.dn == 0 {
                mvlist.tdata = TData {
                    pn: INFINATE,
                    dn: 0,
                    sh: list[0].tdata.sh.saturating_add(1),
                };
                disproof_koma_and(pos, mvlist, &list[0]);
                self.tbase.update(&ks, mvlist, tn);
                return;
            }

            if H::HAS_EDGE_DELTA {
                if dp_and > 0 && list[0].tdata.pn > 0 && list[0].tdata.pn <= EDGE_PN_CEIL {
                    list[0].tdata.pn = list[0]
                        .tdata
                        .pn
                        .saturating_add(dp_and)
                        .min(EDGE_PN_CEIL);
                }
                if d_and > 0 && list[0].tdata.dn > 0 && list[0].tdata.dn <= EDGE_DN_CEIL {
                    list[0].tdata.dn = list[0]
                        .tdata
                        .dn
                        .saturating_add(d_and)
                        .min(EDGE_DN_CEIL);
                }
            }

            // Expand next folded interposition if current head is now proven mate
            let mut n_inserted = 0usize;
            while list[0].tdata.pn == 0 && list[0].has_tail() {
                let tail = list[0].take_tail().unwrap();
                let mut next_item = split_tail_item(pos, tail);
                let child_ks = pos.key_forward_item(&next_item);
                self.tbase.lookup(&child_ks, &mut next_item, tn);
                list.insert(0, next_item);
                n_inserted += 1;
            }
            if n_inserted > 0 {
                if self.config.use_ml_ordering {
                    heuristic.score_and_moves_ctx(
                        pos,
                        mvlist.head_move(),
                        self.tbase.gc_num,
                        &mut list[..n_inserted],
                    );
                }
                if H::HAS_EDGE_DELTA {
                    for item in &mut list[..n_inserted] {
                        let dp = item.pn_delta;
                        if dp > 0 && item.tdata.pn > 0 && item.tdata.pn <= EDGE_PN_CEIL {
                            item.tdata.pn = item
                                .tdata
                                .pn
                                .saturating_add(dp as u16)
                                .min(EDGE_PN_CEIL);
                        }
                        let d = item.dn_delta();
                        if d > 0 && item.tdata.dn > 0 && item.tdata.dn <= EDGE_DN_CEIL {
                            item.tdata.dn = item
                                .tdata
                                .dn
                                .saturating_add(d as u16)
                                .min(EDGE_DN_CEIL);
                        }
                    }
                }
                if SIMPLE {
                    sort_and_simple(list);
                } else {
                    sort_and(list);
                }
            } else if SIMPLE {
                reorder_first_and_simple(list);
            } else {
                reorder_first_and(list);
            }
        }
    }

    #[inline]
    fn make_tree_or<P: PositionBackend, H: SearchHeuristic<P>>(
        &mut self,
        pos: &P,
        mvlist: &mut MvItem,
        heuristic: &mut H,
    ) {
        let mut list = self.acquire_list();
        self.make_tree_or_inner(pos, mvlist, &mut list, heuristic);
        self.release_list(list);
    }

    fn make_tree_or_inner<P: PositionBackend, H: SearchHeuristic<P>>(
        &mut self,
        pos: &P,
        mvlist: &mut MvItem,
        list: &mut Vec<MvItem>,
        heuristic: &mut H,
    ) {
        self.check_limits();
        if pos.count() >= TSUME_MAX_DEPTH - 1 || self.suspend {
            return;
        }
        let tn = pos.turn();
        pos.generate_check_into(&mut self.tbase, list);
        self.nodes += 1;
        if list.is_empty() {
            self.error = true;
            return;
        }

        let enemy_ou = pos.enemy_ou();
        let n = list.len();
        let th_probe_pn = (self.tbase.gc_max_level + MAKE_TREE_PN_PLUS).max(self.config.mt_min_pn);
        let mut th_probe_sh = TSUME_MAX_DEPTH.saturating_sub(pos.count() + 1);
        let mut min_proven_sh = u16::MAX;
        let mut to_probe: smallvec::SmallVec<[usize; 32]> = smallvec::SmallVec::new();
        for i in 0..n {
            // Fast key-only lookup first; only materialize child_pos if a shallow search is needed!
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
                let child_pos = pos.move_forward(list[i].head_move());
                self.bn_search_and::<P, H, false>(&child_pos, thdata, &mut list[i], heuristic);
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
            self.bn_search_and::<P, H, false>(&child_pos, thdata, &mut list[0], heuristic);
            reorder_first_or(list);
        }

        if self.tbase.gc_num > 0 && mvlist.tdata.sh < list[0].tdata.sh && !list[0].inc() {
            let ptsh = mvlist.tdata.sh;
            for item in list.iter_mut() {
                if item.tdata.pn != 0 {
                    break;
                }
                let child_pos = pos.move_forward(item.head_move());
                self.bns_plus_and(&child_pos, item, ptsh, heuristic);
            }
            sort_or(list);
        }

        if list[0].cu() {
            return;
        }

        while !list[0].inc() && !list[0].pr() && !self.suspend && !self.error {
            let child_pos = pos.move_forward(list[0].head_move());
            self.make_tree_and(&child_pos, &mut list[0], heuristic);
            list[0].set_pr(true);
            reorder_first_or(list);
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

    #[inline]
    fn make_tree_and<P: PositionBackend, H: SearchHeuristic<P>>(
        &mut self,
        pos: &P,
        mvlist: &mut MvItem,
        heuristic: &mut H,
    ) {
        let mut list = self.acquire_list();
        self.make_tree_and_inner(pos, mvlist, &mut list, heuristic);
        self.release_list(list);
    }

    fn make_tree_and_inner<P: PositionBackend, H: SearchHeuristic<P>>(
        &mut self,
        pos: &P,
        mvlist: &mut MvItem,
        list: &mut Vec<MvItem>,
        heuristic: &mut H,
    ) {
        if self.suspend {
            return;
        }
        let tn = pos.turn().flip();
        let (proof_flag, invalid_flag) = pos.generate_evasion_into(&mut self.tbase, list);
        self.nodes += 1;

        if list.is_empty() {
            Self::tsumi_proof(pos, mvlist, proof_flag);
            let ks = pos.key_state();
            self.tbase.make_tree_update(&ks, mvlist, tn);
            return;
        }

        // Expand all sequential interpositions
        let mut i = 0;
        while i < list.len() {
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
            self.bn_search_or::<P, H, false>(&child_pos, thdata, &mut list[0], heuristic);
            if list[0].tdata.dn == 0 {
                self.error = true;
                return;
            }
            reorder_first_and(list);
        }

        let ks = pos.key_state();
        let current = self.tbase.make_tree_set_item(&ks, tn, false);
        for item in list.iter_mut() {
            if !item.inc() && !item.cu() && !item.pr() && !self.suspend && !self.error {
                let child_pos = pos.move_forward(item.head_move());
                self.make_tree_or(&child_pos, item, heuristic);
                item.set_pr(true);
            }
        }
        sort_and(list);
        self.tbase.clear_current(current);

        if invalid_flag && list[0].tdata.pn == 0 && list[0].inc() {
            Self::tsumi_proof(pos, mvlist, proof_flag);
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

    #[inline]
    fn bns_plus_or<P: PositionBackend, H: SearchHeuristic<P>>(
        &mut self,
        pos: &P,
        mvlist: &mut MvItem,
        ptsh: u16,
        heuristic: &mut H,
    ) {
        let mut list = self.acquire_list();
        self.bns_plus_or_inner(pos, mvlist, ptsh, &mut list, heuristic);
        self.release_list(list);
    }

    fn bns_plus_or_inner<P: PositionBackend, H: SearchHeuristic<P>>(
        &mut self,
        pos: &P,
        mvlist: &mut MvItem,
        ptsh: u16,
        list: &mut Vec<MvItem>,
        heuristic: &mut H,
    ) {
        self.check_limits();
        if self.suspend {
            return;
        }
        let tn = pos.turn();
        pos.generate_check_into(&mut self.tbase, list);
        self.nodes += 1;
        if list.is_empty() {
            self.error = true;
            return;
        }

        let enemy_ou = pos.enemy_ou();
        let n = list.len();
        for i in 0..n {
            let child_ks = pos.key_forward_item(&list[i]);
            self.tbase.make_tree_lookup(&child_ks, &mut list[i], tn);
            if !list[i].search() {
                let child_pos = pos.move_forward(list[i].head_move());
                let thdata = TData {
                    pn: 1,
                    dn: INFINATE - 1,
                    sh: 3u16.max(self.tbase.gc_max_level + 1),
                };
                self.bn_search_and::<P, H, false>(&child_pos, thdata, &mut list[i], heuristic);
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
                    let current = self.tbase.set_current(&ks, mvlist, tn);
                    let child_pos = pos.move_forward(item.head_move());
                    self.bns_plus_and(&child_pos, item, tsh, heuristic);
                    self.tbase.clear_current(current);
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
                        .min(item.tdata.pn.max(self.tbase.gc_max_level))
                        .saturating_add(1),
                    dn: INFINATE - 1,
                    sh: TSUME_MAX_DEPTH
                        .saturating_sub(pos.count() + 1)
                        .min(mvlist.tdata.sh.saturating_add(ADD_SEARCH_SH)),
                };
                let child_pos = pos.move_forward(item.head_move());
                self.bn_search_and::<P, H, false>(&child_pos, thdata, item, heuristic);
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

    #[inline]
    fn bns_plus_and<P: PositionBackend, H: SearchHeuristic<P>>(
        &mut self,
        pos: &P,
        mvlist: &mut MvItem,
        ptsh: u16,
        heuristic: &mut H,
    ) {
        let mut list = self.acquire_list();
        self.bns_plus_and_inner(pos, mvlist, ptsh, &mut list, heuristic);
        self.release_list(list);
    }

    fn bns_plus_and_inner<P: PositionBackend, H: SearchHeuristic<P>>(
        &mut self,
        pos: &P,
        mvlist: &mut MvItem,
        ptsh: u16,
        list: &mut Vec<MvItem>,
        heuristic: &mut H,
    ) {
        if self.suspend {
            return;
        }
        let tn = pos.turn().flip();
        let (proof_flag, invalid_flag) = pos.generate_evasion_into(&mut self.tbase, list);
        self.nodes += 1;

        if list.is_empty() {
            Self::tsumi_proof(pos, mvlist, proof_flag);
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
        while i < list.len() {
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
            self.bn_search_or::<P, H, false>(&child_pos, thdata, &mut list[0], heuristic);
            reorder_first_and(list);
        }

        let mut tsh = ptsh.saturating_sub(1);
        let ks = pos.key_state();
        let current = self.tbase.make_tree_set_item(&ks, tn, false);
        while !list[0].inc() && !list[0].pr() && !self.suspend {
            tsh = tsh.min(list[0].tdata.sh);
            let child_pos = pos.move_forward(list[0].head_move());
            self.bns_plus_or(&child_pos, &mut list[0], tsh, heuristic);
            list[0].set_pr(true);
            reorder_first_and(list);
        }
        self.tbase.clear_current(current);

        if invalid_flag && list[0].tdata.pn == 0 && list[0].inc() {
            Self::tsumi_proof(pos, mvlist, proof_flag);
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

    /// Reconstruct principal variation from the transposition table (like `tsearchpv_update`).
    fn extract_pv<P: PositionBackend>(&mut self, root: &P) -> Vec<Move> {
        let mut pv = Vec::new();
        let mut seen = HashSet::new();
        let mut pos = root.clone();
        let root_tn = root.turn();

        for ply in 0..TSUME_MAX_DEPTH {
            let key = (pos.zkey(), pos.mkey(root_tn));
            if !seen.insert(key) {
                break;
            }
            if ply % 2 == 0 {
                // OR node
                let mut list = pos.generate_check(&mut self.tbase);
                if list.is_empty() {
                    break;
                }
                for item in &mut list {
                    let child_ks = pos.key_forward_item(item);
                    self.tbase.make_tree_lookup(&child_ks, item, root_tn);
                }
                sort_or(&mut list);
                if list[0].tdata.pn != 0 {
                    let mut mate_mv = pos.find_1ply_mate().map(|m1| m1.mv);
                    if mate_mv.is_none() {
                        for item in &list {
                            let child = pos.move_forward(item.head_move());
                            let (ev, _, _) = child.generate_evasion(&mut self.tbase);
                            if ev.is_empty() {
                                mate_mv = Some(item.head_move());
                                break;
                            }
                        }
                    }
                    if let Some(mv) = mate_mv {
                        pv.push(mv);
                        pos = pos.move_forward(mv);
                    }
                    break;
                }
                let mv = list[0].head_move();
                pv.push(mv);
                pos = pos.move_forward(mv);
            } else {
                // AND node
                let (mut list, _, _) = pos.generate_evasion(&mut self.tbase);
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
                sort_and(&mut list);
                if list[0].tdata.pn != 0 {
                    break;
                }
                let mv = list[0].head_move();
                pv.push(mv);
                pos = pos.move_forward(mv);
            }
        }

        if pv.is_empty()
            || pv.len() % 2 == 0
            || !pos.generate_evasion_simple(&mut self.tbase).is_empty()
        {
            let saved_nl = self.config.node_limit.take();
            let saved_tl = self.config.time_limit_secs.take();
            let recovered = self.extract_pv_simple(root);
            self.config.node_limit = saved_nl;
            self.config.time_limit_secs = saved_tl;
            if !recovered.is_empty() && recovered.len() % 2 == 1 {
                return recovered;
            }
        }
        pv
    }

    /// Exact minimax (`攻方最短・受方最長`, allowing `駒余り` and `無駄合`) DAG pass for `simple_mate` mode:
    /// - Memoizes exact `(zkey, attacker_mkey) -> SimpleDagEntry` (`simple_dag`), avoiding unsound
    ///   `super_tsumi` `sh`/`pr` sharing across distinct hands while staying O(DAG).
    /// - OR nodes (`min sh`): probes unsearched / surplus-piece sibling checks so detour transpositions
    ///   cannot inflate a Defender blunder's `sh`.
    /// - AND nodes (`max sh`): evaluates all legal evasions without futile-drop filtering.
    fn make_tree_simple_or<P: PositionBackend, H: SearchHeuristic<P>>(
        &mut self,
        pos: &P,
        mvlist: &mut MvItem,
        lo: u16,
        heuristic: &mut H,
    ) -> (u16, bool, Option<(Move, MKey, u16)>) {
        let mut list = self.acquire_list();
        let res = self.make_tree_simple_or_inner(pos, mvlist, &mut list, lo, heuristic);
        self.release_list(list);
        res
    }

    fn make_tree_simple_or_inner<P: PositionBackend, H: SearchHeuristic<P>>(
        &mut self,
        pos: &P,
        mvlist: &mut MvItem,
        list: &mut Vec<MvItem>,
        lo: u16,
        heuristic: &mut H,
    ) -> (u16, bool, Option<(Move, MKey, u16)>) {
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
                pos.generate_check_into(&mut self.tbase, list);
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
                            self.probe_and_bounded_simple(
                                &child_pos,
                                thdata,
                                item,
                                96,
                                heuristic,
                            );
                        }
                        if item.tdata.pn == 0 && item.tdata.sh.saturating_add(1) < cand_e.sh {
                            forbid_now |= bit;
                            rem_bits &= !bit;
                        }
                    }
                }
            }

            // Also walk up to 8 OR steps along `cand_e`'s PV in case a spare piece (e.g. Knight
            // when King is currently on rank 8/9) becomes droppable for a fast mate 1..8 moves later.
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
                            walk_pos.generate_check_into(&mut self.tbase, &mut sub_checks);
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
                                    self.probe_and_bounded_simple(
                                        &cpos,
                                        th,
                                        sc,
                                        64,
                                        heuristic,
                                    );
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
            pos.generate_check_into(&mut self.tbase, list);
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
                self.bn_search_and::<P, H, true>(&child_pos, thdata, &mut list[0], heuristic);
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
                            self.probe_and_bounded_simple(
                                &child_pos,
                                thdata,
                                &mut list[j],
                                128,
                                heuristic,
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
                let (sub_dep, _sub_exact) = self.make_tree_simple_and(
                    &child_pos,
                    &mut list[0],
                    lo.saturating_sub(1),
                    heuristic,
                );
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
                            self.probe_and_bounded_simple(
                                &child_pos,
                                thdata,
                                &mut list[j],
                                node_cap,
                                heuristic,
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

    fn make_tree_simple_and<P: PositionBackend, H: SearchHeuristic<P>>(
        &mut self,
        pos: &P,
        mvlist: &mut MvItem,
        lo: u16,
        heuristic: &mut H,
    ) -> (u16, bool) {
        let mut list = self.acquire_list();
        let res = self.make_tree_simple_and_inner(pos, mvlist, &mut list, lo, heuristic);
        self.release_list(list);
        res
    }

    fn make_tree_simple_and_inner<P: PositionBackend, H: SearchHeuristic<P>>(
        &mut self,
        pos: &P,
        mvlist: &mut MvItem,
        list: &mut Vec<MvItem>,
        lo: u16,
        heuristic: &mut H,
    ) -> (u16, bool) {
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
                    let (sub_dep, sub_exact, _) = self.make_tree_simple_or(
                        &child_pos,
                        &mut best_item,
                        lo.saturating_sub(1),
                        heuristic,
                    );
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

        pos.generate_evasion_simple_into(&mut self.tbase, list);
        self.nodes += 1;

        if list.is_empty() {
            Self::tsumi_proof(pos, mvlist, false);
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
        while i < list.len() {
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
                self.bn_search_or::<P, H, true>(&child_pos, thdata, &mut list[idx], heuristic);
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

            let (sub_dep, sub_exact, cap_refute) =
                self.make_tree_simple_or(&child_pos, &mut list[idx], child_lo, heuristic);
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

    fn extract_pv_simple<P: PositionBackend>(&mut self, root: &P) -> Vec<Move> {
        let mut pv = Vec::new();
        let mut seen_and = HashSet::new();
        let mut or_visits: std::collections::HashMap<(u64, MKey), u8> =
            std::collections::HashMap::new();
        let mut forbidden: HashSet<((u64, MKey), Move)> = HashSet::new();
        let mut or_hist: Vec<(P, usize, (u64, MKey), (u64, MKey), Move)> = Vec::new();
        let mut backtracks: usize = 0;
        let mut pos = root.clone();
        let root_tn = root.turn();
        let mut dummy_h = DefaultHeuristic;

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
                let mut list = pos.generate_check(&mut self.tbase);
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
                    'probe: for cap_pn in [4u16, 16, 64, 256, 1024] {
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
                                self.suspend = false;
                                self.config.node_limit = Some(self.nodes + 100_000);
                                self.bn_search_and::<P, DefaultHeuristic, true>(
                                    &child,
                                    th,
                                    item,
                                    &mut dummy_h,
                                );
                                self.config.node_limit = None;
                                self.suspend = false;
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
                let mut list = pos.generate_evasion_simple(&mut self.tbase);
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
                        self.suspend = false;
                        self.config.node_limit = Some(self.nodes + 200_000);
                        self.bn_search_or::<P, DefaultHeuristic, true>(
                            &child,
                            th,
                            item,
                            &mut dummy_h,
                        );
                        self.config.node_limit = None;
                        self.suspend = false;
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
