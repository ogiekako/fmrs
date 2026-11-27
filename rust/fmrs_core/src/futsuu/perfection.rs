//! Conventional Tsume-Shogi Perfection Verifier (`完全作判定`) and Dead-Piece Trimmer (`死に駒除去`).
//!
//! Follows standard Tsume-Shogi conventions (`普通詰将棋の慣習`):
//! - Strictly rejects:
//!   1. `NoMate` (`不詰`) or `Timeout` (`未解決` — including any alternative check whose
//!      refutation or mate status cannot be mathematically resolved within the node budget)
//!   2. `RedundantHand` (`駒余り`: unused attacker hand pieces at checkmate)
//!   3. Early shortcuts (`早詰`: any alternative attacker check mating in `< rem_sh` plies,
//!      EVEN IF it leaves unused pieces in the attacker's hand!)
//!   4. Equal-length distinct mates (`同手数余詰`: non-intended check mating in `== rem_sh` plies
//!      without unused pieces, outside the allowed minor conventions below)
//! - Permits standard minor conventions (tracked in `minor_flaw_count`):
//!   1. Final-move alternatives (`最終手余詰・非限定` at `rem_sh == 1`)
//!   2. Same-piece / drop-line minor non-uniqueness (`成・不成非限定` on same src/dst)
//!   3. Endgame transposition (`収束の同手数手順前後` at `rem_sh <= endgame_transposition_rem_sh`
//!      where every Defender-optimal non-redundant evasion converges back to a position on the
//!      intended main line in equal plies without early mate)
//!   4. Shorter defender-blunder sidelines (`変化余詰` inside `< rem_sh` defender variations)

use std::collections::{HashMap, HashSet};
#[cfg(not(target_arch = "wasm32"))]
use std::sync::atomic::{AtomicU64, AtomicUsize};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
#[cfg(not(target_arch = "wasm32"))]
use std::sync::Mutex;

use crate::futsuu::solver::{analyze_tsume_pv, effective_tsume_pv_plies};
use crate::futsuu::tbase::TBase;
use crate::futsuu::time::Instant;
use crate::futsuu::types::{MKey, MvItem, Turn, FU, GI, HI, KA, KE, KI, KY, SOU, TSUME_MAX_DEPTH};
use crate::futsuu::{
    move_to_movement, FmrsPos, Move, PositionBackend, SearchConfig, SearchResult, Solver,
};
use crate::piece::{Color, Kind};
use crate::position::{position::PositionAux, Movement};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FlawKind {
    /// Mates strictly faster (`< rem_sh`), with or without leftover pieces.
    EarlyShortcut,
    /// Mates in the exact same length (`== rem_sh`) with no leftover pieces via an unrelated line.
    EqualLengthShortcut,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PerfectionStatus {
    /// Valid complete composition under standard tsume-shogi conventions.
    Perfect,
    /// Attacker has at least one prohibited shortcut (`余詰` / `早詰`).
    Shortcut {
        first_ply: u16,
        alt_move: Move,
        intended_move: Move,
        kind: FlawKind,
    },
    /// Main line mates, but leaves unused attacker pieces (`駒余り`).
    RedundantHand,
    /// Initial position has no mate (`不詰`).
    NoMate,
    /// Node or time limit exceeded during solve or verification.
    Timeout,
}

#[derive(Clone, Debug)]
pub struct PerfectionReport {
    pub status: PerfectionStatus,
    pub mate_steps: u16,
    pub pv: Vec<Move>,
    /// When `status` is `PerfectionStatus::Shortcut`, holds a complete legal checkmating line
    /// (`pv[..first_ply] + [alt_move, defender_evasion, ...]`) demonstrating the shortcut for KIF forks.
    pub shortcut_pv: Vec<Move>,
    pub solve_nodes: u64,
    pub check_nodes: u64,
    /// Number of PV OR-positions that exhibited a prohibited shortcut.
    pub shortcut_or_count: u16,
    /// Count of permitted minor conventions (final-move ambiguity, prom/unprom, `rem_sh <= 5` convergence).
    pub minor_flaw_count: u16,
}

impl PerfectionReport {
    #[inline]
    pub fn is_perfect(&self) -> bool {
        matches!(self.status, PerfectionStatus::Perfect)
    }
}

#[derive(Clone, Debug)]
pub struct PerfectionConfig {
    pub hash_mb: usize,
    pub solve_node_limit: u64,
    pub check_node_limit_per_move: u64,
    pub search_level: u8,
    /// Stop checking remaining PV OR nodes as soon as the first shortcut is found.
    pub fail_fast: bool,
    /// Maximum remaining plies (`rem_sh`) at which equal-length convergent transpositions are permitted.
    pub endgame_transposition_rem_sh: u16,
}

impl Default for PerfectionConfig {
    fn default() -> Self {
        Self {
            hash_mb: 32,
            solve_node_limit: 500_000,
            check_node_limit_per_move: 80_000,
            search_level: 0,
            fail_fast: true,
            endgame_transposition_rem_sh: 5,
        }
    }
}

#[inline]
pub fn is_same_ray_sliding_drop(or_pos: &FmrsPos, mv: Move, intended_mv: Move) -> bool {
    if !mv.is_drop() || !intended_mv.is_drop() || mv.hand_ktype() != intended_mv.hand_ktype() {
        return false;
    }
    let kt = mv.hand_ktype();
    if kt != HI && kt != KA && kt != KY {
        return false;
    }
    let king = or_pos.enemy_ou();
    let kf = (king % 9) as i32;
    let kr = (king / 9) as i32;
    let mf = (mv.dest() % 9) as i32 - kf;
    let mr = (mv.dest() / 9) as i32 - kr;
    let inf = (intended_mv.dest() % 9) as i32 - kf;
    let inr = (intended_mv.dest() / 9) as i32 - kr;
    if (mf == 0 && mr == 0) || (inf == 0 && inr == 0) {
        return false;
    }
    let valid_m = mf == 0 || mr == 0 || mf.abs() == mr.abs();
    let valid_in = inf == 0 || inr == 0 || inf.abs() == inr.abs();
    valid_m && valid_in && mf.signum() == inf.signum() && mr.signum() == inr.signum()
}

/// Build a single-move `MvItem` with accurate piece hints for probing any individual check `mv`
/// (including secondary unpromoted moves inside a multi-move `MvItem`).
#[inline]
pub fn single_check_item(or_pos: &FmrsPos, mv: Move) -> MvItem {
    let src_koma = if mv.is_board_move() {
        or_pos.board_at(mv.prev_pos as usize)
    } else {
        0
    };
    let cap_koma = or_pos.board_at(mv.dest());
    MvItem::single_with_hints(mv, src_koma, cap_koma)
}

const BPN_INF: u32 = 1_000_000_000;
const MAX_BOUNDED_DEPTH: u16 = 160;

#[inline]
fn max_all_mkey() -> MKey {
    MKey::new(18, 4, 4, 4, 4, 2, 2)
}

#[inline]
fn max_for_ktype(kt: u8) -> u8 {
    match kt {
        FU => 18,
        KY | KE | GI | KI => 4,
        KA | HI => 2,
        _ => 0,
    }
}

#[inline]
fn mkey_with(mut mk: MKey, kt: u8, val: u8) -> MKey {
    match kt {
        FU => mk.set_fu(val),
        KY => mk.set_ky(val),
        KE => mk.set_ke(val),
        GI => mk.set_gi(val),
        KI => mk.set_ki(val),
        KA => mk.set_ka(val),
        HI => mk.set_hi(val),
        _ => {}
    }
    mk
}

#[inline]
fn chebyshev_dist(sq_a: usize, sq_b: usize) -> u32 {
    let fa = (sq_a % 9) as i32;
    let ra = (sq_a / 9) as i32;
    let fb = (sq_b % 9) as i32;
    let rb = (sq_b / 9) as i32;
    (fa - fb).unsigned_abs().max((ra - rb).unsigned_abs())
}

#[derive(Clone, Copy)]
struct BoundedBoundsEntry {
    disproved_up_to: Option<u16>,
    min_proved_checks: u16,
}

impl Default for BoundedBoundsEntry {
    fn default() -> Self {
        Self {
            disproved_up_to: None,
            min_proved_checks: u16::MAX,
        }
    }
}

#[derive(Clone, Copy)]
struct DisproofDomCard {
    s_mk: MKey,
    g_mk: MKey,
    max_checks: u16,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum BoundedMode {
    /// Sound lower-bound disproof mode (`L_no_stall <= L_tsume`):
    /// Grants `0` check-cost credit when any interposition (`打合` or `移動合`) is immediately
    /// recaptured on the same square. `dn == 0` mathematically guarantees no Tsume-Shogi mate
    /// of length `<= max_plies` exists.
    DisproveNoStall,
    /// Sound upper-bound Tsume-Shogi mate prover (`L_tsume <= L_futile_stripped`):
    /// Grants `0` check-cost credit only when an undefended drop interposition is immediately
    /// recaptured AND the captured piece is stripped from the attacker's hand (proving `打無駄合`
    /// constructively). `pn == 0` mathematically guarantees a Tsume-Shogi mate `<= max_plies` exists.
    ProveTsume { require_clean: bool },
}

/// Exact depth-bounded `u32` DAG df-pn verifier (`no 255 PROOF_MAX cap, zero GHI`).
pub struct BoundedDfPn {
    /// Proven/disproved depth bounds `(zkey, s_mk, g_mk, inter_sq, drop_kt, is_or)`.
    bounds_tt: HashMap<(u64, u64, u64, u8, u8, bool), BoundedBoundsEntry>,
    /// Depth-exact working `(pn, dn)` table keyed WITH `rem_checks` so DAG transpositions across
    /// different remaining check counts NEVER overwrite each other's df-pn thresholds!
    work_tt: HashMap<(u64, u64, u64, u8, u8, u16, bool), (u32, u32)>,
    /// Persistent monotone `(H_A, H_D)` disproof dominance table for `DisproveNoStall`
    /// (`H_A_card >= H_A && H_D >= H_D_card && rem_checks <= max_checks => dn == 0`).
    /// Soundly shared across all PV plies and evasions of a root verification.
    disproof_dom: HashMap<(u64, u8, bool), Vec<DisproofDomCard>>,
    /// Active AND-node recursion stack `(zkey, s_mk, g_mk) -> depth` for GHI-free perpetual-check detection.
    active_and: HashMap<(u64, u64, u64), u16>,
    scratch_tb: TBase,
    nodes: u64,
    budget: u64,
    suspended: bool,
    mode: BoundedMode,
    abort_flag: Option<Arc<AtomicBool>>,
}

impl Default for BoundedDfPn {
    fn default() -> Self {
        Self::new()
    }
}

impl BoundedDfPn {
    pub fn new() -> Self {
        Self::with_capacity(16_384)
    }

    pub(crate) fn with_capacity(cap: usize) -> Self {
        Self {
            bounds_tt: HashMap::with_capacity(cap),
            work_tt: HashMap::with_capacity(cap),
            disproof_dom: HashMap::with_capacity(cap),
            active_and: HashMap::with_capacity(128),
            scratch_tb: TBase::new(1),
            nodes: 0,
            budget: 0,
            suspended: false,
            mode: BoundedMode::DisproveNoStall,
            abort_flag: None,
        }
    }

    pub fn set_abort_flag(&mut self, flag: Option<Arc<AtomicBool>>) {
        self.abort_flag = flag;
    }

    pub fn clear_for_root(&mut self) {
        self.bounds_tt.clear();
        self.work_tt.clear();
        self.disproof_dom.clear();
        self.active_and.clear();
    }

    /// Returns `(Some(true), nodes)` if `root_or` is **provably** unable to mate within `max_plies`
    /// (`odd >= 1`), `(Some(false), nodes)` if a bounded no-stall mate path exists, or `(None, nodes)` on timeout.
    pub fn disprove_within(
        &mut self,
        root_or: &FmrsPos,
        max_plies: u16,
        init_interpose_sq: u8,
        budget: u64,
    ) -> (Option<bool>, u64) {
        if max_plies == 0 {
            return (Some(true), 0);
        }
        let max_checks = (max_plies + 1) / 2;
        if self.mode != BoundedMode::DisproveNoStall || self.bounds_tt.len() > 262_144 {
            self.bounds_tt.clear();
        }
        self.work_tt.clear();
        self.active_and.clear();
        self.nodes = 0;
        self.budget = budget;
        self.suspended = false;
        self.mode = BoundedMode::DisproveNoStall;

        let ((pn, dn), _) = self.search_or(
            root_or,
            max_checks,
            init_interpose_sq,
            0,
            0,
            BPN_INF,
            BPN_INF,
        );
        if self.suspended {
            (None, self.nodes)
        } else if dn == 0 {
            (Some(true), self.nodes)
        } else if pn == 0 {
            (Some(false), self.nodes)
        } else {
            (None, self.nodes)
        }
    }

    /// Find the minimum Tsume-Shogi mate plies `<= max_plies` (`odd >= 1`), if one provably exists.
    /// When `require_clean` is `true`, only checkmates with zero leftover attacker hand pieces qualify.
    pub fn prove_tsume_mate_within(
        &mut self,
        root_or: &FmrsPos,
        max_plies: u16,
        init_undef_drop: Option<(u8, u8)>,
        require_clean: bool,
        budget: u64,
    ) -> (Option<u16>, u64) {
        if max_plies == 0 {
            return (None, 0);
        }
        let max_checks = (max_plies + 1) / 2;
        self.bounds_tt.clear();
        self.work_tt.clear();
        self.active_and.clear();
        self.nodes = 0;
        self.budget = budget;
        self.suspended = false;
        self.mode = BoundedMode::ProveTsume { require_clean };

        let (init_sq, init_kt) = init_undef_drop.unwrap_or((255, 0));
        let ((pn, _), _) =
            self.search_or(root_or, max_checks, init_sq, init_kt, 0, BPN_INF, BPN_INF);
        if self.suspended || pn != 0 {
            return (None, self.nodes);
        }
        let mut best_checks = max_checks;
        let mut lo = 1u16;
        let mut hi = max_checks.saturating_sub(1);
        while lo <= hi && !self.suspended {
            let mid = lo + (hi - lo) / 2;
            self.work_tt.clear();
            self.active_and.clear();
            let ((mpn, _), _) =
                self.search_or(root_or, mid, init_sq, init_kt, 0, BPN_INF, BPN_INF);
            if self.suspended {
                break;
            }
            if mpn == 0 {
                best_checks = mid;
                if mid == 1 {
                    break;
                }
                hi = mid - 1;
            } else {
                lo = mid + 1;
            }
        }
        (Some(best_checks * 2 - 1), self.nodes)
    }

    /// Extract a mate witness map `(zkey, atk_mkey) -> check_mv` from the last `ProveTsume` proof DAG.
    pub fn extract_witness_map(
        &mut self,
        root_or: &FmrsPos,
        proved_plies: u16,
        init_undef_drop: Option<(u8, u8)>,
        root_tn: Turn,
        out: &mut HashMap<(u64, MKey), Move>,
    ) {
        let checks = (proved_plies + 1) / 2;
        let (init_sq, init_kt) = init_undef_drop.unwrap_or((255, 0));
        let mut vis = HashSet::new();
        self.extract_witness_or_rec(root_or, checks, init_sq, init_kt, root_tn, out, &mut vis);
    }

    #[allow(clippy::too_many_arguments)]
    fn extract_witness_or_rec(
        &mut self,
        pos: &FmrsPos,
        rem_checks: u16,
        inter_sq: u8,
        inter_drop_ktype: u8,
        root_tn: Turn,
        out: &mut HashMap<(u64, MKey), Move>,
        vis: &mut HashSet<(u64, u64, u64, u16)>,
    ) {
        if out.len() >= 2_048 {
            return;
        }
        let s_mk = pos.mkey(Turn::Sente);
        let g_mk = pos.mkey(Turn::Gote);
        if !vis.insert((pos.zkey(), s_mk.0, g_mk.0, rem_checks)) {
            return;
        }
        let require_clean = matches!(
            self.mode,
            BoundedMode::ProveTsume {
                require_clean: true
            }
        );
        if rem_checks >= 1 {
            let m1_opt = if require_clean {
                pos.find_1ply_mate().filter(|m1| {
                    (m1.mv.is_board_move() && m1.cap_koma == 0 && s_mk.is_empty())
                        || (m1.mv.is_drop() && s_mk.total() == 1)
                })
            } else {
                pos.find_1ply_mate_simple()
            };
            if let Some(m1) = m1_opt {
                out.insert((pos.zkey(), pos.mkey(root_tn)), m1.mv);
                return;
            }
        }
        self.scratch_tb.clear();
        let items = pos.generate_check(&mut self.scratch_tb);
        let mut chosen: Option<(Move, FmrsPos, u16, bool)> = None;
        'find_mv: for it in &items {
            for mv in it.all_moves() {
                let is_recap =
                    inter_sq < 81 && mv.is_board_move() && mv.dest() == inter_sq as usize;
                if is_recap && inter_drop_ktype != 0 {
                    let c_pos = pos
                        .move_forward(mv)
                        .without_attacker_hand_piece(inter_drop_ktype);
                    let ck = (
                        c_pos.zkey(),
                        c_pos.mkey(Turn::Sente).0,
                        c_pos.mkey(Turn::Gote).0,
                        mv.dest() as u8,
                        1,
                        false,
                    );
                    self.scratch_tb.clear();
                    let leaf_ok = c_pos.generate_evasion(&mut self.scratch_tb).0.is_empty()
                        && (!require_clean || c_pos.mkey(Turn::Sente).is_empty());
                    if leaf_ok
                        || self
                            .bounds_tt
                            .get(&ck)
                            .is_some_and(|e| rem_checks >= e.min_proved_checks)
                    {
                        chosen = Some((mv, c_pos, rem_checks, true));
                        break 'find_mv;
                    }
                }
                if rem_checks > 0 {
                    let c_pos = pos.move_forward(mv);
                    let ck = (
                        c_pos.zkey(),
                        c_pos.mkey(Turn::Sente).0,
                        c_pos.mkey(Turn::Gote).0,
                        mv.dest() as u8,
                        0,
                        false,
                    );
                    let c_rem = rem_checks - 1;
                    self.scratch_tb.clear();
                    let leaf_ok = c_pos.generate_evasion(&mut self.scratch_tb).0.is_empty()
                        && (!require_clean || c_pos.mkey(Turn::Sente).is_empty());
                    if leaf_ok
                        || self
                            .bounds_tt
                            .get(&ck)
                            .is_some_and(|e| c_rem >= e.min_proved_checks)
                    {
                        chosen = Some((mv, c_pos, c_rem, false));
                        break 'find_mv;
                    }
                }
            }
        }
        let Some((mv, and_pos, c_rem, was_free_recap)) = chosen else {
            return;
        };
        out.insert((pos.zkey(), pos.mkey(root_tn)), mv);
        self.scratch_tb.clear();
        let (evs, _, _) = and_pos.generate_evasion(&mut self.scratch_tb);
        for ev in &evs {
            for emv in ev.all_moves() {
                if out.len() >= 2_048 {
                    return;
                }
                let is_recap_suteai =
                    was_free_recap && !emv.is_drop() && emv.dest() == mv.dest();
                let next_rem = if is_recap_suteai {
                    c_rem.saturating_sub(1)
                } else {
                    c_rem
                };
                let undef_kt = if emv.is_drop() {
                    emv.hand_ktype()
                } else {
                    0
                };
                let eff_sq = if undef_kt != 0 { emv.dest() as u8 } else { 255 };
                let next_or = and_pos.move_forward(emv);
                self.extract_witness_or_rec(
                    &next_or, next_rem, eff_sq, undef_kt, root_tn, out, vis,
                );
            }
        }
    }

    #[inline]
    fn lookup_child(
        &self,
        key: &(u64, u64, u64, u8, u8, bool),
        s_mk: MKey,
        g_mk: MKey,
        rem_checks: u16,
        init_pn: u32,
        init_dn: u32,
    ) -> (u32, u32) {
        if self.mode == BoundedMode::DisproveNoStall {
            if let Some(cards) = self.disproof_dom.get(&(key.0, key.3, key.5)) {
                for c in cards {
                    if rem_checks <= c.max_checks && c.s_mk.ge_all(s_mk) && g_mk.ge_all(c.g_mk) {
                        return (BPN_INF, 0);
                    }
                }
            }
        }
        if let Some(e) = self.bounds_tt.get(key) {
            if let Some(d_max) = e.disproved_up_to {
                if rem_checks <= d_max {
                    return (BPN_INF, 0);
                }
            }
            if rem_checks >= e.min_proved_checks {
                return (0, BPN_INF);
            }
        }
        let work_key = (key.0, key.1, key.2, key.3, key.4, rem_checks, key.5);
        if let Some(&(wpn, wdn)) = self.work_tt.get(&work_key) {
            return (wpn, wdn);
        }
        (init_pn, init_dn)
    }

    #[inline]
    fn lookup_disproof_info(
        &self,
        key: &(u64, u64, u64, u8, u8, bool),
        s_mk: MKey,
        g_mk: MKey,
        rem_checks: u16,
    ) -> (MKey, MKey, u16) {
        let mut best: Option<(MKey, MKey, u16)> = None;
        if let Some(cards) = self.disproof_dom.get(&(key.0, key.3, key.5)) {
            for c in cards {
                if rem_checks <= c.max_checks && c.s_mk.ge_all(s_mk) && g_mk.ge_all(c.g_mk) {
                    if best.is_none_or(|b| c.max_checks > b.2) {
                        best = Some((c.s_mk, c.g_mk, c.max_checks));
                    }
                }
            }
        }
        if let Some(e) = self.bounds_tt.get(key) {
            if let Some(d_max) = e.disproved_up_to {
                if rem_checks <= d_max && best.is_none_or(|b| d_max > b.2) {
                    best = Some((s_mk, g_mk, d_max));
                }
            }
        }
        best.unwrap_or((s_mk, g_mk, rem_checks))
    }

    #[inline]
    fn store_tt(
        &mut self,
        key: (u64, u64, u64, u8, u8, bool),
        s_mk: MKey,
        g_mk: MKey,
        rem_checks: u16,
        pn: u32,
        dn: u32,
        can_cache_disproof: bool,
    ) {
        if pn == 0 {
            if self.bounds_tt.len() < 524_288 || self.bounds_tt.contains_key(&key) {
                let e = self.bounds_tt.entry(key).or_default();
                e.min_proved_checks = e.min_proved_checks.min(rem_checks);
            }
            return;
        }
        if dn == 0 {
            if !can_cache_disproof {
                return;
            }
            if self.mode == BoundedMode::DisproveNoStall && self.disproof_dom.len() < 262_144 {
                let cards = self
                    .disproof_dom
                    .entry((key.0, key.3, key.5))
                    .or_insert_with(|| Vec::with_capacity(2));
                let mut subsumed = false;
                for c in cards.iter_mut() {
                    if c.s_mk.ge_all(s_mk) && g_mk.ge_all(c.g_mk) && c.max_checks >= rem_checks {
                        subsumed = true;
                        break;
                    }
                    if s_mk.ge_all(c.s_mk) && c.g_mk.ge_all(g_mk) && rem_checks >= c.max_checks {
                        *c = DisproofDomCard {
                            s_mk,
                            g_mk,
                            max_checks: rem_checks,
                        };
                        subsumed = true;
                        break;
                    }
                }
                if !subsumed && cards.len() < 8 {
                    cards.push(DisproofDomCard {
                        s_mk,
                        g_mk,
                        max_checks: rem_checks,
                    });
                }
            }
            if self.bounds_tt.len() < 524_288 || self.bounds_tt.contains_key(&key) {
                let e = self.bounds_tt.entry(key).or_default();
                e.disproved_up_to =
                    Some(e.disproved_up_to.map_or(rem_checks, |v| v.max(rem_checks)));
            }
            return;
        }
        let work_key = (key.0, key.1, key.2, key.3, key.4, rem_checks, key.5);
        self.work_tt.insert(work_key, (pn, dn));
    }

    #[allow(clippy::too_many_arguments)]
    fn search_or(
        &mut self,
        pos: &FmrsPos,
        rem_checks: u16,
        inter_sq: u8,
        inter_drop_ktype: u8,
        depth: u16,
        th_pn: u32,
        th_dn: u32,
    ) -> ((u32, u32), u16) {
        if self.nodes >= self.budget
            || ((self.nodes & 63) == 0
                && self
                    .abort_flag
                    .as_ref()
                    .is_some_and(|f| f.load(Ordering::Relaxed)))
        {
            self.suspended = true;
            return ((1, 1), u16::MAX);
        }
        if depth >= MAX_BOUNDED_DEPTH {
            return match self.mode {
                BoundedMode::DisproveNoStall => ((0, BPN_INF), u16::MAX),
                BoundedMode::ProveTsume { .. } => ((BPN_INF, 0), u16::MAX),
            };
        }
        if rem_checks == 0 && inter_sq >= 81 {
            return ((BPN_INF, 0), u16::MAX);
        }

        let s_mk = pos.mkey(Turn::Sente);
        let g_mk = pos.mkey(Turn::Gote);
        let key = (pos.zkey(), s_mk.0, g_mk.0, inter_sq, inter_drop_ktype, true);
        let (cached_pn, cached_dn) = self.lookup_child(&key, s_mk, g_mk, rem_checks, 1, 1);
        if cached_pn >= th_pn || cached_dn >= th_dn {
            return ((cached_pn, cached_dn), u16::MAX);
        }

        // O(1) bitboard 1-ply Tsume-Shogi mate check (`50ns` lookahead!)
        if rem_checks >= 1 {
            let m1_opt = match self.mode {
                BoundedMode::ProveTsume {
                    require_clean: true,
                } => pos.find_1ply_mate(),
                _ => pos.find_1ply_mate_simple(),
            };
            if let Some(m1) = m1_opt {
                let ok_clean = match self.mode {
                    BoundedMode::DisproveNoStall => true,
                    BoundedMode::ProveTsume { require_clean } => {
                        !require_clean
                            || (m1.mv.is_board_move() && m1.cap_koma == 0 && s_mk.is_empty())
                            || (m1.mv.is_drop() && s_mk.total() == 1)
                    }
                };
                if ok_clean {
                    self.store_tt(key, s_mk, g_mk, rem_checks, 0, BPN_INF, true);
                    return ((0, BPN_INF), u16::MAX);
                }
            }
        }

        self.scratch_tb.clear();
        let raw_items = pos.generate_check(&mut self.scratch_tb);
        let work_key = (key.0, key.1, key.2, key.3, key.4, rem_checks, key.5);
        if !self.work_tt.contains_key(&work_key) {
            self.nodes += 1;
        }

        struct OrChild {
            mv: Move,
            cap_koma: u8,
            strip_kt: u8,
            c_rem: u16,
            c_s_mk: MKey,
            c_g_mk: MKey,
            c_key: (u64, u64, u64, u8, u8, bool),
            init_pn: u32,
            init_dn: u32,
            pos_cache: Option<FmrsPos>,
        }

        let enemy_ou = pos.enemy_ou();
        let mut children: Vec<OrChild> = Vec::with_capacity(raw_items.len() + 2);

        for it in &raw_items {
            for mv in it.all_moves() {
                let dist = chebyshev_dist(mv.dest(), enemy_ou);
                let (init_pn, init_dn) = if dist <= 1 {
                    if mv.is_board_move() {
                        (1, 2)
                    } else {
                        (2, 2)
                    }
                } else {
                    (dist * 2, 1)
                };
                let s_item = single_check_item(pos, mv);
                let cap_koma = s_item.cap_koma;
                let cks = pos.key_forward_item(&s_item);
                let cs = cks.mkey[Turn::Sente.index()];
                let cg = cks.mkey[Turn::Gote.index()];

                let is_recap_on_inter =
                    inter_sq < 81 && mv.is_board_move() && mv.dest() == inter_sq as usize;
                match self.mode {
                    BoundedMode::DisproveNoStall => {
                        if is_recap_on_inter {
                            let c_key = (cks.zkey, cs.0, cg.0, mv.dest() as u8, 0, false);
                            children.push(OrChild {
                                mv,
                                cap_koma,
                                strip_kt: 0,
                                c_rem: rem_checks,
                                c_s_mk: cs,
                                c_g_mk: cg,
                                c_key,
                                init_pn: 1,
                                init_dn: 2,
                                pos_cache: None,
                            });
                        } else if rem_checks > 0 {
                            let c_key = (cks.zkey, cs.0, cg.0, mv.dest() as u8, 0, false);
                            children.push(OrChild {
                                mv,
                                cap_koma,
                                strip_kt: 0,
                                c_rem: rem_checks - 1,
                                c_s_mk: cs,
                                c_g_mk: cg,
                                c_key,
                                init_pn,
                                init_dn,
                                pos_cache: None,
                            });
                        }
                    }
                    BoundedMode::ProveTsume { .. } => {
                        if is_recap_on_inter && inter_drop_ktype != 0 {
                            let mut cs_stripped = cs;
                            if cs_stripped.get(inter_drop_ktype) > 0 {
                                cs_stripped.dec(inter_drop_ktype);
                            }
                            let c_key = (cks.zkey, cs_stripped.0, cg.0, mv.dest() as u8, 1, false);
                            children.push(OrChild {
                                mv,
                                cap_koma,
                                strip_kt: inter_drop_ktype,
                                c_rem: rem_checks,
                                c_s_mk: cs_stripped,
                                c_g_mk: cg,
                                c_key,
                                init_pn: 1,
                                init_dn: 2,
                                pos_cache: None,
                            });
                        }
                        if rem_checks > 0 {
                            let c_key = (cks.zkey, cs.0, cg.0, mv.dest() as u8, 0, false);
                            children.push(OrChild {
                                mv,
                                cap_koma,
                                strip_kt: 0,
                                c_rem: rem_checks - 1,
                                c_s_mk: cs,
                                c_g_mk: cg,
                                c_key,
                                init_pn,
                                init_dn,
                                pos_cache: None,
                            });
                        }
                    }
                }
            }
        }

        if children.is_empty() {
            let (gen_s_mk, valid_rem) = if raw_items.is_empty() {
                let mut gs = s_mk;
                for kt in [HI, KA, KI, GI, KE, KY, FU] {
                    if s_mk.get(kt) > 0 || !pos.drop_check(kt) {
                        gs = mkey_with(gs, kt, max_for_ktype(kt));
                    }
                }
                (gs, u16::MAX)
            } else {
                (max_all_mkey(), 0)
            };
            self.store_tt(key, gen_s_mk, MKey::EMPTY, valid_rem, BPN_INF, 0, true);
            return ((BPN_INF, 0), u16::MAX);
        }

        if children.len() > 1 {
            children.sort_by_key(|c| c.init_pn);
        }

        let mut child_pndn: Vec<(u32, u32)> = children
            .iter()
            .map(|c| {
                if self
                    .active_and
                    .contains_key(&(c.c_key.0, c.c_s_mk.0, c.c_g_mk.0))
                {
                    // Perpetual-check cycle on child AND-node!
                    (BPN_INF, 0)
                } else {
                    self.lookup_child(&c.c_key, c.c_s_mk, c.c_g_mk, c.c_rem, c.init_pn, c.init_dn)
                }
            })
            .collect();

        let mut min_cyc = u16::MAX;
        for (i, c) in children.iter().enumerate() {
            if child_pndn[i].1 == 0 {
                if let Some(&ad) = self.active_and.get(&(c.c_key.0, c.c_s_mk.0, c.c_g_mk.0)) {
                    min_cyc = min_cyc.min(ad);
                }
            }
        }

        loop {
            let mut cur_pn = BPN_INF;
            let mut max_cdn = 0u32;
            let mut active_dn_cnt = 0u32;
            let mut best_i = 0usize;
            let mut pn2 = BPN_INF;

            for (i, &(cpn, cdn)) in child_pndn.iter().enumerate() {
                if cpn < cur_pn {
                    pn2 = cur_pn;
                    cur_pn = cpn;
                    best_i = i;
                } else if cpn < pn2 {
                    pn2 = cpn;
                }
                if cdn > 0 {
                    active_dn_cnt += 1;
                    if cdn > max_cdn {
                        max_cdn = cdn;
                    }
                }
            }
            let dcnt = active_dn_cnt.saturating_sub(1);
            let mut cur_dn = if active_dn_cnt == 0 {
                0
            } else {
                max_cdn.saturating_add(dcnt).min(BPN_INF)
            };
            if cur_pn == 0 {
                cur_dn = BPN_INF;
            } else if cur_dn == 0 {
                cur_pn = BPN_INF;
            }

            if cur_pn >= th_pn || cur_dn >= th_dn || self.suspended {
                if !self.suspended {
                    let (st_s_mk, st_g_mk, st_rem) = if cur_dn == 0
                        && min_cyc >= depth
                        && self.mode == BoundedMode::DisproveNoStall
                    {
                        let mut or_max_s = max_all_mkey();
                        let mut or_need_g = MKey::EMPTY;
                        let mut or_valid_rem = u16::MAX;
                        for c in &children {
                            let (cs, cg, c_max_r) =
                                self.lookup_disproof_info(&c.c_key, c.c_s_mk, c.c_g_mk, c.c_rem);
                            let contrib_r = if c.c_rem < rem_checks {
                                c_max_r.saturating_add(1)
                            } else {
                                c_max_r
                            };
                            or_valid_rem = or_valid_rem.min(contrib_r);
                            or_need_g = or_need_g.max(cg);
                            let mut allowed = cs;
                            if c.mv.is_drop() {
                                let kt = c.mv.hand_ktype();
                                allowed = mkey_with(
                                    allowed,
                                    kt,
                                    (cs.get(kt) + 1).min(max_for_ktype(kt)),
                                );
                            } else if c.cap_koma != 0 && c.strip_kt == 0 {
                                let cap_kt = c.cap_koma & 7;
                                allowed = mkey_with(allowed, cap_kt, cs.get(cap_kt).saturating_sub(1));
                            }
                            for kt in [HI, KA, KI, GI, KE, KY, FU] {
                                or_max_s = mkey_with(or_max_s, kt, or_max_s.get(kt).min(allowed.get(kt)));
                            }
                        }
                        let or_valid_rem = or_valid_rem.max(rem_checks);
                        if or_valid_rem > 0 {
                            for kt in [HI, KA, KI, GI, KE, KY, FU] {
                                if s_mk.get(kt) == 0 && pos.drop_check(kt) {
                                    or_max_s = mkey_with(or_max_s, kt, 0);
                                } else {
                                    or_max_s = mkey_with(or_max_s, kt, or_max_s.get(kt).max(s_mk.get(kt)));
                                }
                            }
                        }
                        (or_max_s, or_need_g, or_valid_rem)
                    } else {
                        (s_mk, g_mk, rem_checks)
                    };
                    self.store_tt(
                        key,
                        st_s_mk,
                        st_g_mk,
                        st_rem,
                        cur_pn,
                        cur_dn,
                        min_cyc >= depth,
                    );
                }
                let out_cyc = if cur_dn == 0 && min_cyc < depth {
                    min_cyc
                } else {
                    u16::MAX
                };
                return ((cur_pn, cur_dn), out_cyc);
            }

            let pn_step = if pn2 >= BPN_INF { 0 } else { 1 + (pn2 >> 2) };
            let c_th_pn = th_pn.min(pn2.saturating_add(pn_step));
            let c_th_dn = if th_dn >= BPN_INF {
                BPN_INF
            } else {
                th_dn.saturating_sub(dcnt).max(1)
            };

            let c = &mut children[best_i];
            if c.pos_cache.is_none() {
                let mut np = pos.move_forward(c.mv);
                if c.strip_kt != 0 {
                    np = np.without_attacker_hand_piece(c.strip_kt);
                }
                c.pos_cache = Some(np);
            }
            let c_pos_ref = c.pos_cache.as_ref().unwrap();
            let cdest = c.mv.dest();
            let c_rem = c.c_rem;
            let free_recap = c.strip_kt != 0;
            let (pndn, c_cyc) =
                self.search_and(c_pos_ref, cdest, c_rem, free_recap, depth + 1, c_th_pn, c_th_dn);
            child_pndn[best_i] = pndn;
            if pndn.1 == 0 {
                min_cyc = min_cyc.min(c_cyc);
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn search_and(
        &mut self,
        and_pos: &FmrsPos,
        checker_dest: usize,
        rem_checks_after: u16,
        was_free_recap: bool,
        depth: u16,
        th_pn: u32,
        th_dn: u32,
    ) -> ((u32, u32), u16) {
        if self.nodes >= self.budget
            || ((self.nodes & 63) == 0
                && self
                    .abort_flag
                    .as_ref()
                    .is_some_and(|f| f.load(Ordering::Relaxed)))
        {
            self.suspended = true;
            return ((1, 1), u16::MAX);
        }
        if depth >= MAX_BOUNDED_DEPTH {
            return match self.mode {
                BoundedMode::DisproveNoStall => ((0, BPN_INF), u16::MAX),
                BoundedMode::ProveTsume { .. } => ((BPN_INF, 0), u16::MAX),
            };
        }

        let s_mk = and_pos.mkey(Turn::Sente);
        let g_mk = and_pos.mkey(Turn::Gote);
        let cyc_key = (and_pos.zkey(), s_mk.0, g_mk.0);
        if let Some(&anc_depth) = self.active_and.get(&cyc_key) {
            // Perpetual check (`連続王手の千日手`): Attacker loses immediately -> Defender escapes!
            return ((BPN_INF, 0), anc_depth);
        }

        let key = (
            and_pos.zkey(),
            s_mk.0,
            g_mk.0,
            checker_dest as u8,
            u8::from(was_free_recap),
            false,
        );
        let (cached_pn, cached_dn) = self.lookup_child(&key, s_mk, g_mk, rem_checks_after, 1, 1);
        if cached_pn >= th_pn || cached_dn >= th_dn {
            return ((cached_pn, cached_dn), u16::MAX);
        }

        self.scratch_tb.clear();
        let (ev_items, _, _) = and_pos.generate_evasion(&mut self.scratch_tb);
        let work_key = (key.0, key.1, key.2, key.3, key.4, rem_checks_after, key.5);
        if !self.work_tt.contains_key(&work_key) {
            self.nodes += 1;
        }

        if ev_items.is_empty() {
            let is_valid_mate = match self.mode {
                BoundedMode::DisproveNoStall => true,
                BoundedMode::ProveTsume { require_clean } => !require_clean || s_mk.is_empty(),
            };
            let (lpn, ldn) = if is_valid_mate {
                (0, BPN_INF)
            } else {
                (BPN_INF, 0)
            };
            self.store_tt(key, s_mk, g_mk, rem_checks_after, lpn, ldn, true);
            return ((lpn, ldn), u16::MAX);
        }

        struct AndChild {
            emv: Move,
            cap_koma: u8,
            eff_sq: u8,
            drop_kt: u8,
            c_rem: u16,
            c_s_mk: MKey,
            c_g_mk: MKey,
            c_key: (u64, u64, u64, u8, u8, bool),
            init_pn: u32,
            init_dn: u32,
            pos_cache: Option<FmrsPos>,
        }

        let self_ou = and_pos.self_ou();
        let mut children: Vec<AndChild> = Vec::with_capacity(ev_items.len() + 4);
        let mut last_drop_sq: usize = usize::MAX;

        for it in &ev_items {
            for emv in it.all_moves() {
                let is_cap_checker = !emv.is_drop() && emv.dest() == checker_dest;
                let is_king = !emv.is_drop() && emv.prev_pos as usize == self_ou;
                let is_king_or_cap = is_cap_checker || is_king;

                // If `was_free_recap && is_cap_checker`, Defender recaptured on the interposition
                // square (`捨て合 / 中合`), so those 2 plies ARE valid Tsume-Shogi defense and
                // consume 1 additional check!
                let suteai_extra = u16::from(was_free_recap && is_cap_checker);
                if is_king_or_cap && rem_checks_after <= suteai_extra {
                    self.store_tt(
                        key,
                        max_all_mkey(),
                        MKey::EMPTY,
                        rem_checks_after,
                        BPN_INF,
                        0,
                        true,
                    );
                    return ((BPN_INF, 0), u16::MAX);
                }
                let child_rem = rem_checks_after - suteai_extra;

                let src_koma = if emv.is_board_move() {
                    and_pos.board_at(emv.prev_pos as usize)
                } else {
                    0
                };
                let cap_koma = if emv.is_board_move() {
                    and_pos.board_at(emv.dest())
                } else {
                    0
                };
                let e_item = MvItem::single_with_hints(emv, src_koma, cap_koma);
                let cks = and_pos.key_forward_item(&e_item);
                let cs = cks.mkey[Turn::Sente.index()];
                let cg = cks.mkey[Turn::Gote.index()];

                if is_king_or_cap {
                    let init_dn = if is_cap_checker || cap_koma != 0 {
                        1
                    } else {
                        2
                    };
                    let c_key = (cks.zkey, cs.0, cg.0, 255, 0, true);
                    children.push(AndChild {
                        emv,
                        cap_koma,
                        eff_sq: 255,
                        drop_kt: 0,
                        c_rem: child_rem,
                        c_s_mk: cs,
                        c_g_mk: cg,
                        c_key,
                        init_pn: 1,
                        init_dn,
                        pos_cache: None,
                    });
                } else {
                    let isq = emv.dest() as u8;
                    let defended = and_pos.self_effect_contains(emv.dest());
                    let init_dn = if !emv.is_drop() {
                        if defended { 2 } else { 32 }
                    } else if defended {
                        match emv.hand_ktype() {
                            KI => 2,
                            HI | GI | KA => 4,
                            _ => 16,
                        }
                    } else if emv.dest() != last_drop_sq {
                        last_drop_sq = emv.dest();
                        256
                    } else {
                        512
                    };
                    match self.mode {
                        BoundedMode::DisproveNoStall => {
                            let c_key = (cks.zkey, cs.0, cg.0, isq, 0, true);
                            children.push(AndChild {
                                emv,
                                cap_koma,
                                eff_sq: isq,
                                drop_kt: 0,
                                c_rem: child_rem,
                                c_s_mk: cs,
                                c_g_mk: cg,
                                c_key,
                                init_pn: 1,
                                init_dn,
                                pos_cache: None,
                            });
                        }
                        BoundedMode::ProveTsume { .. } => {
                            let undef_drop_kt = if emv.is_drop() {
                                emv.hand_ktype()
                            } else {
                                0
                            };
                            if child_rem == 0 && undef_drop_kt == 0 {
                                self.store_tt(key, s_mk, g_mk, 0, BPN_INF, 0, true);
                                return ((BPN_INF, 0), u16::MAX);
                            }
                            let eff_sq = if undef_drop_kt != 0 { isq } else { 255 };
                            let c_key = (cks.zkey, cs.0, cg.0, eff_sq, undef_drop_kt, true);
                            children.push(AndChild {
                                emv,
                                cap_koma,
                                eff_sq,
                                drop_kt: undef_drop_kt,
                                c_rem: child_rem,
                                c_s_mk: cs,
                                c_g_mk: cg,
                                c_key,
                                init_pn: 1,
                                init_dn,
                                pos_cache: None,
                            });
                        }
                    }
                }
            }
        }

        if children.len() > 1 {
            children.sort_by_key(|c| c.init_dn);
        }

        let mut child_pndn: Vec<(u32, u32)> = children
            .iter()
            .map(|c| {
                if c.c_rem == 0 && c.eff_sq == 255 {
                    (BPN_INF, 0)
                } else {
                    self.lookup_child(
                        &c.c_key,
                        c.c_s_mk,
                        c.c_g_mk,
                        c.c_rem,
                        c.init_pn,
                        c.init_dn,
                    )
                }
            })
            .collect();

        self.active_and.insert(cyc_key, depth);
        let mut min_cyc = u16::MAX;

        let res = loop {
            let mut max_cpn = 0u32;
            let mut active_pn_cnt = 0u32;
            let mut cur_dn = BPN_INF;
            let mut best_i = 0usize;
            let mut dn2 = BPN_INF;

            for (i, &(cpn, cdn)) in child_pndn.iter().enumerate() {
                if cpn > 0 {
                    active_pn_cnt += 1;
                    if cpn > max_cpn {
                        max_cpn = cpn;
                    }
                }
                if cdn < cur_dn {
                    dn2 = cur_dn;
                    cur_dn = cdn;
                    best_i = i;
                } else if cdn < dn2 {
                    dn2 = cdn;
                }
            }
            let pcnt = active_pn_cnt.saturating_sub(1);
            let mut cur_pn = if active_pn_cnt == 0 {
                0
            } else {
                max_cpn.saturating_add(pcnt).min(BPN_INF)
            };
            if cur_dn == 0 {
                cur_pn = BPN_INF;
            } else if cur_pn == 0 {
                cur_dn = BPN_INF;
            }

            if cur_pn >= th_pn || cur_dn >= th_dn || self.suspended {
                if !self.suspended {
                    let (st_s_mk, st_g_mk, st_rem) = if cur_dn == 0
                        && min_cyc >= depth
                        && self.mode == BoundedMode::DisproveNoStall
                    {
                        if let Some(esc_i) = child_pndn.iter().position(|&(_, d)| d == 0) {
                            let c = &children[esc_i];
                            if rem_checks_after == 0 && c.eff_sq == 255 {
                                (max_all_mkey(), MKey::EMPTY, 0)
                            } else {
                                let (cs, cg, c_max_r) = self.lookup_disproof_info(
                                    &c.c_key,
                                    c.c_s_mk,
                                    c.c_g_mk,
                                    rem_checks_after,
                                );
                                let mut ng = cg;
                                if c.emv.is_drop() {
                                    let kt = c.emv.hand_ktype();
                                    ng = mkey_with(ng, kt, (ng.get(kt) + 1).min(g_mk.get(kt)));
                                } else if c.cap_koma != 0 {
                                    let cap_kt = c.cap_koma & 7;
                                    if ng.get(cap_kt) > 0 {
                                        ng.dec(cap_kt);
                                    }
                                }
                                (cs, ng, c_max_r.max(rem_checks_after))
                            }
                        } else {
                            (s_mk, g_mk, rem_checks_after)
                        }
                    } else {
                        (s_mk, g_mk, rem_checks_after)
                    };
                    self.store_tt(
                        key,
                        st_s_mk,
                        st_g_mk,
                        st_rem,
                        cur_pn,
                        cur_dn,
                        min_cyc >= depth,
                    );
                }
                let out_cyc = if cur_dn == 0 && min_cyc < depth {
                    min_cyc
                } else {
                    u16::MAX
                };
                break ((cur_pn, cur_dn), out_cyc);
            }

            let dn_step = if dn2 >= BPN_INF { 0 } else { 1 + (dn2 >> 2) };
            let c_th_dn = th_dn.min(dn2.saturating_add(dn_step));
            let c_th_pn = if th_pn >= BPN_INF {
                BPN_INF
            } else {
                th_pn.saturating_sub(pcnt).max(1)
            };

            let c = &mut children[best_i];
            if c.pos_cache.is_none() {
                c.pos_cache = Some(and_pos.move_forward(c.emv));
            }
            let c_pos_ref = c.pos_cache.as_ref().unwrap();
            let eff_sq = c.eff_sq;
            let drop_kt = c.drop_kt;
            let c_rem = c.c_rem;
            let (pndn, c_cyc) = self.search_or(
                c_pos_ref,
                c_rem,
                eff_sq,
                drop_kt,
                depth + 1,
                c_th_pn,
                c_th_dn,
            );
            child_pndn[best_i] = pndn;
            if pndn.1 == 0 {
                min_cyc = min_cyc.min(c_cyc);
            }
        };

        self.active_and.remove(&cyc_key);
        res
    }
}

#[derive(Clone)]
pub struct EvSolved {
    pub emv: Move,
    pub is_king_or_cap: bool,
    pub std_br_sh: u16,
    pub std_red: bool,
    pub min_any_sh: u16,
    pub nouse2: u8,
    pub pv: Vec<Move>,
    pub extra_witnesses: HashMap<(u64, MKey), Move>,
}

pub enum AltResolved {
    Refuted {
        escaping_ev: Option<Move>,
        used_hint: bool,
    },
    Mated {
        std_steps: u16,
        non_redundant: bool,
        any_steps: u16,
        solved_evs: Vec<EvSolved>,
    },
}

fn extract_mate_witness_from_tb(
    tb: &mut TBase,
    pos: &FmrsPos,
    root_tn: Turn,
    out: &mut HashMap<(u64, MKey), Move>,
    vis: &mut HashSet<(u64, MKey, MKey)>,
) {
    if out.len() >= 2_048 {
        return;
    }
    let def_tn = root_tn.flip();
    let fk = (pos.zkey(), pos.mkey(root_tn), pos.mkey(def_tn));
    if !vis.insert(fk) {
        return;
    }
    if let Some(m1) = pos.find_1ply_mate_simple() {
        out.insert((fk.0, fk.1), m1.mv);
        return;
    }
    let checks = pos.generate_check(tb);
    let mut best: Option<((bool, u16), Move)> = None;
    for item in &checks {
        for mv in item.all_moves() {
            let mut s_it = single_check_item(pos, mv);
            let child_ks = pos.key_forward_item(&s_it);
            tb.make_tree_lookup(&child_ks, &mut s_it, root_tn);
            if s_it.tdata.pn == 0 {
                let rank = (!s_it.pr(), s_it.tdata.sh);
                if best.is_none_or(|b| rank < b.0) {
                    best = Some((rank, mv));
                }
            }
        }
    }
    if let Some((_, mv)) = best {
        out.insert((fk.0, fk.1), mv);
        let and_pos = pos.move_forward(mv);
        let mut scratch = TBase::new(1);
        let (evs, _, _) = and_pos.generate_evasion(&mut scratch);
        for ev in &evs {
            for emv in ev.all_moves() {
                if out.len() >= 2_048 {
                    return;
                }
                let next_or = and_pos.move_forward(emv);
                extract_mate_witness_from_tb(tb, &next_or, root_tn, out, vis);
            }
        }
    }
}

/// Count unused attacker hand pieces at the end of `pv`, excluding any pieces gained solely from
/// capturing collapsed futile interpositions (`打無駄合` / `移動無駄合`).
#[inline]
fn unused_atk_pieces_effective(start_or: &FmrsPos, pv: &[Move], _atk_tn: Turn) -> u8 {
    analyze_tsume_pv(start_or, pv).2
}

/// Compute per-ply Tsume-Shogi step weights (`1` for genuine plies, `0` for collapsed futile
/// interposition+recapture pairs) and remaining effective Tsume-Shogi plies `rem_eff[ply]`.
#[inline]
fn compute_pv_effective_rem(root: &FmrsPos, pv: &[Move]) -> (Vec<u8>, Vec<u16>) {
    let (step_delta, rem_eff, _) = analyze_tsume_pv(root, pv);
    (step_delta, rem_eff)
}

/// Deterministically play out any trailing physical `無駄合` (`合い利かず` drops or `移動無駄合`)
/// after Tsume-Shogi checkmate has already been achieved (`generate_evasion.0.is_empty()`),
/// so `out` terminates at a physical Shogi checkmate (`generate_evasion_simple.is_empty()`).
fn all_evasions_constructively_futile(and_pos: &FmrsPos, scratch: &mut TBase) -> bool {
    scratch.clear();
    let tsume_evs = and_pos.generate_evasion(scratch).0;
    if tsume_evs.is_empty() {
        return true;
    }
    let ou = and_pos.self_ou();
    let atk_sq = and_pos.attack_sq();
    let mut rep_evs: Vec<Move> = Vec::new();
    for it in &tsume_evs {
        for em in it.all_moves() {
            if !em.is_drop()
                && ((and_pos.n_oute() == 1 && em.dest() == atk_sq) || em.prev_pos as usize == ou)
            {
                return false;
            }
            if !rep_evs.iter().any(|x| x.is_drop() == em.is_drop() && x.dest() == em.dest() && x.prev_pos == em.prev_pos) {
                if em.is_drop() && rep_evs.iter().any(|x| x.is_drop() && x.dest() == em.dest()) {
                    continue;
                }
                rep_evs.push(em);
            }
        }
    }
    !rep_evs.is_empty()
        && rep_evs
            .into_iter()
            .all(|em| is_constructive_futile_inter_ev(and_pos, em, scratch, 0))
}

fn playout_trailing_futile_evasions(
    cur: &mut FmrsPos,
    out: &mut Vec<Move>,
    scratch: &mut TBase,
) -> bool {
    for _ in 0..24 {
        scratch.clear();
        let phys_evs = cur.generate_evasion_simple(scratch);
        if phys_evs.is_empty() {
            return true;
        }
        if !all_evasions_constructively_futile(cur, scratch) {
            return false;
        }
        let emv = phys_evs[0].head_move();
        let next_or = cur.move_forward(emv);
        scratch.clear();
        let checks = next_or.generate_check(scratch);
        let all_cmvs: Vec<Move> = checks.into_iter().flat_map(|it| it.all_moves()).collect();
        let recap_opt = all_cmvs
            .iter()
            .copied()
            .find(|&cm| {
                cm.is_board_move() && cm.dest() == emv.dest() && {
                    let c2 = next_or.move_forward(cm);
                    scratch.clear();
                    c2.generate_evasion_simple(scratch).is_empty()
                }
            })
            .or_else(|| {
                all_cmvs.iter().copied().find(|&cm| {
                    cm.is_board_move() && cm.dest() == emv.dest() && {
                        let c2 = next_or.move_forward(cm);
                        all_evasions_constructively_futile(&c2, scratch)
                    }
                })
            })
            .or_else(|| {
                all_cmvs.into_iter().find(|&cm| {
                    let c2 = next_or.move_forward(cm);
                    all_evasions_constructively_futile(&c2, scratch)
                })
            });
        let Some(rmv) = recap_opt else {
            return false;
        };
        out.push(emv);
        out.push(rmv);
        *cur = next_or.move_forward(rmv);
    }
    scratch.clear();
    cur.generate_evasion_simple(scratch).is_empty()
}

fn backend_to_sfen<P: PositionBackend>(pos: &P) -> String {
    use crate::futsuu::types::{
        GGI, GHI, GKA, GKE, GKI, GKY, GNG, GNK, GNY, GOU, GRY, GTO, GUM, SGI, SHI, SKA, SKE, SKI,
        SKY, SNG, SNK, SNY, SPC, SRY, STO, SUM,
    };
    let mut s = String::with_capacity(96);
    for r in 0..9 {
        if r > 0 {
            s.push('/');
        }
        let mut empty = 0u8;
        for f in (0..9).rev() {
            let k = pos.board_at(r * 9 + f);
            if k == SPC {
                empty += 1;
                continue;
            }
            if empty > 0 {
                s.push((b'0' + empty) as char);
                empty = 0;
            }
            let tok = match k {
                1 => "P",
                SKY => "L",
                SKE => "N",
                SGI => "S",
                SKI => "G",
                SKA => "B",
                SHI => "R",
                SOU => "K",
                STO => "+P",
                SNY => "+L",
                SNK => "+N",
                SNG => "+S",
                SUM => "+B",
                SRY => "+R",
                17 => "p",
                GKY => "l",
                GKE => "n",
                GGI => "s",
                GKI => "g",
                GKA => "b",
                GHI => "r",
                GOU => "k",
                GTO => "+p",
                GNY => "+l",
                GNK => "+n",
                GNG => "+s",
                GUM => "+b",
                GRY => "+r",
                _ => "?",
            };
            s.push_str(tok);
        }
        if empty > 0 {
            s.push((b'0' + empty) as char);
        }
    }
    s.push_str(if pos.turn() == Turn::Sente { " b " } else { " w " });
    let sm = pos.mkey(Turn::Sente);
    let gm = pos.mkey(Turn::Gote);
    if sm.is_empty() && gm.is_empty() {
        s.push('-');
    } else {
        for &(kt, ch) in &[
            (HI, 'R'),
            (KA, 'B'),
            (KI, 'G'),
            (GI, 'S'),
            (KE, 'N'),
            (KY, 'L'),
            (FU, 'P'),
        ] {
            let cnt = sm.get(kt);
            if cnt > 1 {
                s.push_str(&cnt.to_string());
            }
            if cnt >= 1 {
                s.push(ch);
            }
        }
        for &(kt, ch) in &[
            (HI, 'r'),
            (KA, 'b'),
            (KI, 'g'),
            (GI, 's'),
            (KE, 'n'),
            (KY, 'l'),
            (FU, 'p'),
        ] {
            let cnt = gm.get(kt);
            if cnt > 1 {
                s.push_str(&cnt.to_string());
            }
            if cnt >= 1 {
                s.push(ch);
            }
        }
    }
    s.push_str(" 1");
    s
}

/// Lightweight post-solve refiner invoked ONLY on very short mates (`mate_steps <= 13`,
/// zero cost on any medium/long problem) to resolve `shtsume`'s `super_tsumi` / `not_inc`
/// shadowing of clean short mates and `inc`-skipping of longer Defender evasions.
pub(crate) fn refine_short_tsume_pv_if_needed<P: PositionBackend>(
    root: &P,
    pv: &mut Vec<Move>,
    mate_steps: &mut u16,
    redundant: &mut bool,
    extra_nodes: &mut u64,
) {
    if pv.is_empty()
        || pv.len() % 2 == 0
        || *mate_steps > 13
        || (!*redundant && *mate_steps > 7)
        || root.turn() != Turn::Sente
    {
        return;
    }
    let Ok(froot) = FmrsPos::from_sfen(&backend_to_sfen(root)) else {
        return;
    };
    let root_tn = froot.turn();
    let mut bnd = BoundedDfPn::with_capacity(512);
    let mut scratch = TBase::new(1);

    // 1. If `redundant`, check whether a clean Tsume-Shogi mate exists within `mate_steps + 2`:
    if *redundant {
        let probe_cap = (*mate_steps).saturating_add(2).min(15);
        let (clean_opt, b_nodes) =
            bnd.prove_tsume_mate_within(&froot, probe_cap, None, true, 4_096);
        *extra_nodes += b_nodes;
        if let Some(csh) = clean_opt {
            let mut witnesses = HashMap::new();
            bnd.extract_witness_map(&froot, csh, None, root_tn, &mut witnesses);
            if let Some(w_pv) =
                extract_witness_line(&froot, root_tn, csh as usize, &witnesses, true, &mut scratch)
            {
                if let Some((clean_pv, c_eff)) =
                    validate_and_measure_mate_line(&froot, &w_pv, &mut scratch)
                {
                    *pv = clean_pv;
                    *mate_steps = c_eff as u16;
                    *redundant = false;
                }
            }
        }
    }

    // 2. For `mate_steps <= 7`, verify along the `<= 3` AND-nodes of `pv` that no non-futile
    //    Defender evasion survives strictly longer than `rem_next`:
    for _pass in 0..4 {
        if *mate_steps > 7 || pv.is_empty() || pv.len() % 2 == 0 {
            break;
        }
        let (step_delta, rem_eff, eff_unused) = analyze_tsume_pv(&froot, pv);
        *mate_steps = rem_eff[0];
        *redundant = eff_unused > 0;

        let mut and_nodes: Vec<(usize, FmrsPos, Move, u16, bool)> = Vec::with_capacity(4);
        {
            let mut cur = froot.clone();
            for (ply, &mv) in pv.iter().enumerate() {
                if ply % 2 == 1 {
                    and_nodes.push((
                        ply,
                        cur.clone(),
                        mv,
                        rem_eff[ply].saturating_sub(1),
                        step_delta[ply] == 0,
                    ));
                }
                cur = cur.move_forward(mv);
            }
        }

        let mut spliced = false;
        for &(ply, ref and_pos, pv_ev, rem_next, cur_is_futile) in and_nodes.iter().rev() {
            if cur_is_futile {
                continue;
            }
            let checker_dest = pv[ply - 1].dest();
            scratch.clear();
            let (evs, _, invalid_flag) = and_pos.generate_evasion(&mut scratch);
            let mut best_longer: Option<(u16, bool, Move, Vec<Move>)> = None;

            for it in &evs {
                for emv in it.all_moves() {
                    if emv == pv_ev || (invalid_flag && emv.is_drop()) {
                        continue;
                    }
                    let is_king_or_cap = !emv.is_drop()
                        && (emv.dest() == checker_dest
                            || (and_pos.board_at(emv.prev_pos as usize) & 15) == SOU);
                    let init_ud: Option<(u8, u8)> = None;
                    let next_or = and_pos.move_forward(emv).with_count(0);
                    bnd.clear_for_root();
                    let (within_opt, n1) =
                        bnd.prove_tsume_mate_within(&next_or, rem_next, init_ud, false, 2_048);
                    *extra_nodes += n1;
                    if within_opt.is_some() || bnd.suspended {
                        continue;
                    }
                    // `next_or` is proven NOT to mate within `<= rem_next` plies!
                    let cap_hi = rem_next.saturating_add(6).min(13);
                    let (clean_opt, n2) =
                        bnd.prove_tsume_mate_within(&next_or, cap_hi, init_ud, true, 4_096);
                    *extra_nodes += n2;
                    let (chosen_sh, chosen_clean) = if let Some(csh) = clean_opt {
                        (csh, true)
                    } else {
                        let (any_opt, n3) =
                            bnd.prove_tsume_mate_within(&next_or, cap_hi, init_ud, false, 4_096);
                        *extra_nodes += n3;
                        let Some(ash) = any_opt else {
                            continue;
                        };
                        (ash, false)
                    };
                    let mut witnesses = HashMap::new();
                    bnd.extract_witness_map(&next_or, chosen_sh, init_ud, root_tn, &mut witnesses);
                    let Some(w_pv) = extract_witness_line(
                        &next_or,
                        root_tn,
                        chosen_sh as usize,
                        &witnesses,
                        chosen_clean,
                        &mut scratch,
                    ) else {
                        continue;
                    };
                    let Some((tail_pv, eff_len)) =
                        validate_and_measure_mate_line(&next_or, &w_pv, &mut scratch)
                    else {
                        continue;
                    };
                    let tail_unused = unused_atk_pieces_effective(&next_or, &tail_pv, root_tn);
                    let tail_red = tail_unused > 0;
                    if !is_king_or_cap && tail_red && tail_unused > and_pos.mkey(root_tn).total() {
                        if let Some(&m0) = tail_pv.first() {
                            if m0.is_board_move() && m0.dest() == emv.dest() {
                                let after_recap = next_or.move_forward(m0);
                                scratch.clear();
                                let r_evs = after_recap.generate_evasion_simple(&mut scratch);
                                let recap_defended = r_evs.iter().any(|rit| {
                                    rit.all_moves()
                                        .into_iter()
                                        .any(|rm| rm.is_board_move() && rm.dest() == emv.dest())
                                });
                                if !recap_defended {
                                    continue;
                                }
                            }
                        }
                    }
                    let eff_u16 = eff_len as u16;
                    if eff_u16 > rem_next
                        && best_longer
                            .as_ref()
                            .is_none_or(|b| (eff_u16, tail_red) > (b.0, b.1))
                    {
                        best_longer = Some((eff_u16, tail_red, emv, tail_pv));
                    }
                }
            }

            if let Some((_, _, best_emv, best_tail)) = best_longer {
                pv.truncate(ply);
                pv.push(best_emv);
                pv.extend(best_tail);
                let (_, rem2, un2) = analyze_tsume_pv(&froot, pv);
                *mate_steps = rem2[0];
                *redundant = un2 > 0;
                spliced = true;
                break;
            }
        }
        if !spliced {
            break;
        }
    }
}

/// Sound, uncapped resolver for an alternative check `mv` leading to `child_and` (`rem_sh >= 3`).
///
/// Guarantees:
/// - Never clamps `check_node_limit_per_move` to `12_000`, and scales final budget to at least
///   `root_solve_nodes` so deep compositions are audited with commensurate search effort.
/// - Never treats `PROOF_MAX` (`255`) heuristic PN ceiling as `NoMate` (`Refuted`).
/// - Prevents shadowed shorter mates inside `sub_solver.solve` subtrees via `BoundedDfPn`.
/// - Distinguishes non-interpositions (`King move / checker capture`) from futile interpositions (`無駄合`)
///   so neither `無駄合` masks a dual mate nor `変化駒余り` / `本手順駒余り` is misclassified.
#[allow(clippy::too_many_arguments)]
pub fn resolve_alt_check_sound(
    child_and: &FmrsPos,
    checker_dest: usize,
    rem_sh: u16,
    root_tn: Turn,
    hinted_ev: Option<Move>,
    pv_or_states: &HashSet<(u64, MKey, MKey, u16)>,
    max_b: u64,
    require_exact_disproof: bool,
    sub_solver: &mut Solver,
    bounded: &mut BoundedDfPn,
    scratch_tb: &mut TBase,
    nodes_acc: &mut u64,
) -> Result<AltResolved, ()> {
    scratch_tb.clear();
    let (evasions, _, invalid_flag) = child_and.generate_evasion(scratch_tb);
    if evasions.is_empty() {
        let atk_hand_empty = child_and.mkey(child_and.turn().flip()).is_empty();
        return Ok(AltResolved::Mated {
            std_steps: 1,
            non_redundant: atk_hand_empty,
            any_steps: 1,
            solved_evs: Vec::new(),
        });
    }

    struct EvSlot {
        emv: Move,
        is_king_or_cap: bool,
        next_or: FmrsPos,
        init_inter_sq: u8,
        init_undef_drop: Option<(u8, u8)>,
        from_hint: bool,
        solved: Option<EvSolved>,
    }

    let mut slots: Vec<EvSlot> = Vec::new();
    let push_slot = |emv: Move, from_hint: bool, out: &mut Vec<EvSlot>| {
        let is_king_or_cap = !emv.is_drop()
            && (emv.dest() == checker_dest
                || (child_and.board_at(emv.prev_pos as usize) & 15) == SOU);
        let init_inter_sq = if is_king_or_cap {
            255
        } else {
            emv.dest() as u8
        };
        let init_undef_drop = if emv.is_drop() && !child_and.self_effect_contains(emv.dest()) {
            Some((emv.dest() as u8, emv.hand_ktype()))
        } else {
            None
        };
        let next_or = child_and.move_forward(emv).with_count(0);
        out.push(EvSlot {
            emv,
            is_king_or_cap,
            next_or,
            init_inter_sq,
            init_undef_drop,
            from_hint,
            solved: None,
        });
    };

    if let Some(hmv) = hinted_ev {
        if evasions.iter().any(|ev| ev.all_moves().contains(&hmv)) {
            push_slot(hmv, true, &mut slots);
        }
    }
    let mut rest_evs: Vec<(u32, Move)> = Vec::new();
    for ev in &evasions {
        for emv in ev.all_moves() {
            if Some(emv) == hinted_ev {
                continue;
            }
            let prio = if !emv.is_drop() && emv.dest() == checker_dest {
                0
            } else if !emv.is_drop() && (child_and.board_at(emv.prev_pos as usize) & 15) == SOU {
                let df = ((emv.dest() % 9) as i32 - 4).unsigned_abs();
                let dr = ((emv.dest() / 9) as i32 - 4).unsigned_abs();
                10 + df + dr
            } else if !emv.is_drop() {
                30
            } else if child_and.self_effect_contains(emv.dest()) {
                match emv.hand_ktype() {
                    KI => 25,
                    HI | GI | KA => 28,
                    _ => 40,
                }
            } else {
                100
            };
            rest_evs.push((prio, emv));
        }
    }
    rest_evs.sort_by_key(|&(p, _)| p);
    for (_, emv) in rest_evs {
        push_slot(emv, false, &mut slots);
    }

    let eff_max_b = max_b.max(2_048);
    let mut tiers: Vec<u64> = vec![512];
    if eff_max_b > 8_192 {
        tiers.push(8_192);
    }
    if eff_max_b > *tiers.last().unwrap() {
        tiers.push(eff_max_b);
    }

    // Phase 0: Resolve all non-drop evasions (King moves, captures, board interpositions, plus any hint)
    // across `[512, 8_192, eff_max_b]` BEFORE ever touching drop interpositions (`Phase 1`).
    // In >98% of alternative checks, Defender escapes via a King move or capture, saving all drop solves!
    for drop_phase in [false, true] {
        for &b in &tiers {
            let is_final_tier = b == eff_max_b;
            let mut phase_all_solved = true;

            for slot in &mut slots {
                let in_phase = if slot.emv.is_drop() && !slot.from_hint {
                    drop_phase
                } else {
                    !drop_phase
                };
                if !in_phase || slot.solved.is_some() {
                    continue;
                }

            // Fast exact PV-convergence check (`0`-ply at `rem_sh - 2` or `2`-ply at `rem_sh - 4`):
            // Avoids re-solving the entire remaining main line from scratch on convergent drops (`e.g. R*6b -> R*3b -> 6b3b+`)
            // while still verifying via `BoundedDfPn` that NO shorter `< rem_sh - 2` mate exists!
            let def_tn = root_tn.flip();
            let mut conv_pv_pair: Option<Vec<Move>> = None;
            if pv_or_states.contains(&(
                slot.next_or.zkey(),
                slot.next_or.mkey(root_tn),
                slot.next_or.mkey(def_tn),
                rem_sh - 2,
            )) {
                conv_pv_pair = Some(Vec::new());
            } else if rem_sh >= 5 {
                scratch_tb.clear();
                let c0_list = slot.next_or.generate_check(scratch_tb);
                'find_conv: for it0 in &c0_list {
                    for m0 in it0.all_moves() {
                        let after_m0 = slot.next_or.move_forward(m0);
                        scratch_tb.clear();
                        let (evs2, _, _) = after_m0.generate_evasion(scratch_tb);
                        if !evs2.is_empty() {
                            let mut first_e2 = None;
                            let mut all_in_pv = true;
                            for ev2 in &evs2 {
                                for emv2 in ev2.all_moves() {
                                    if first_e2.is_none() {
                                        first_e2 = Some(emv2);
                                    }
                                    let or4 = after_m0.move_forward(emv2);
                                    if !pv_or_states.contains(&(
                                        or4.zkey(),
                                        or4.mkey(root_tn),
                                        or4.mkey(def_tn),
                                        rem_sh - 4,
                                    )) {
                                        all_in_pv = false;
                                        break;
                                    }
                                }
                                if !all_in_pv {
                                    break;
                                }
                            }
                            if all_in_pv {
                                conv_pv_pair = Some(vec![m0, first_e2.unwrap()]);
                                break 'find_conv;
                            }
                        }
                    }
                }
            }
            if let Some(cpv) = conv_pv_pair {
                if rem_sh < 5 {
                    slot.solved = Some(EvSolved {
                        emv: slot.emv,
                        is_king_or_cap: slot.is_king_or_cap,
                        std_br_sh: rem_sh - 2,
                        std_red: false,
                        min_any_sh: rem_sh - 2,
                        nouse2: 0,
                        pv: cpv,
                        extra_witnesses: HashMap::new(),
                    });
                    continue;
                }
                let (any4_opt, a_nodes) = bounded.prove_tsume_mate_within(
                    &slot.next_or,
                    rem_sh - 4,
                    slot.init_undef_drop,
                    false,
                    eff_max_b.min(8_192),
                );
                *nodes_acc += a_nodes;
                if let Some(ash) = any4_opt {
                    let mut extra_witnesses = HashMap::new();
                    bounded.extract_witness_map(
                        &slot.next_or,
                        ash,
                        slot.init_undef_drop,
                        root_tn,
                        &mut extra_witnesses,
                    );
                    slot.solved = Some(EvSolved {
                        emv: slot.emv,
                        is_king_or_cap: slot.is_king_or_cap,
                        std_br_sh: rem_sh - 2,
                        std_red: false,
                        min_any_sh: ash,
                        nouse2: 0,
                        pv: cpv,
                        extra_witnesses,
                    });
                    continue;
                }
                let ok_conv = if require_exact_disproof {
                    let (dis4, b_nodes) = bounded.disprove_within(
                        &slot.next_or,
                        rem_sh - 4,
                        slot.init_inter_sq,
                        eff_max_b,
                    );
                    *nodes_acc += b_nodes;
                    dis4 == Some(true)
                } else {
                    true
                };
                if ok_conv {
                    slot.solved = Some(EvSolved {
                        emv: slot.emv,
                        is_king_or_cap: slot.is_king_or_cap,
                        std_br_sh: rem_sh - 2,
                        std_red: false,
                        min_any_sh: rem_sh - 2,
                        nouse2: 0,
                        pv: cpv,
                        extra_witnesses: HashMap::new(),
                    });
                    continue;
                }
            }

            // Fast depth-bounded disproof on King moves / checker captures (`never 無駄合`) at small/medium `rem_sh`!
            if slot.is_king_or_cap && rem_sh <= 15 {
                let (dis_opt, b_nodes) = bounded.disprove_within(
                    &slot.next_or,
                    rem_sh - 2,
                    255,
                    b.min(4_096),
                );
                *nodes_acc += b_nodes;
                if dis_opt == Some(true) {
                    return Ok(AltResolved::Refuted {
                        escaping_ev: Some(slot.emv),
                        used_hint: slot.from_hint,
                    });
                }
            }

            sub_solver.set_node_limit(Some(b));
            let mut sub_res = sub_solver.solve_unbounded_pn(&slot.next_or);
            *nodes_acc += sub_res.nodes;
            // Never discard an already-proven `pn == 0` mate just because `make_tree` / `bns_plus`
            // hit `node_limit` right after `bn_search_or` proved mate at the final tier:
            if is_final_tier && sub_res.mate == Some(true) && sub_solver.is_suspended() && !sub_res.error {
                sub_solver.set_node_limit(Some(b.saturating_add((b / 2).max(65_536))));
                sub_res = sub_solver.solve_unbounded_pn(&slot.next_or);
                *nodes_acc += sub_res.nodes;
            }

            match sub_res.mate {
                Some(false) if !sub_solver.is_suspended() && !sub_res.error => {
                    return Ok(AltResolved::Refuted {
                        escaping_ev: Some(slot.emv),
                        used_hint: slot.from_hint,
                    });
                }
                Some(true) if !sub_solver.is_suspended() && !sub_res.error => {
                    let mut std_br_sh = sub_res.mate_steps;
                    let mut std_red = sub_res.redundant;
                    let mut min_any_sh = sub_res.mate_steps;
                    let mut extra_witnesses: HashMap<(u64, MKey), Move> = HashMap::new();
                    if std_br_sh < rem_sh.saturating_sub(2) {
                        if let (Some(ash), a_nodes) = bounded.prove_tsume_mate_within(
                            &slot.next_or,
                            rem_sh - 4,
                            slot.init_undef_drop,
                            false,
                            eff_max_b.min(4_096),
                        ) {
                            *nodes_acc += a_nodes;
                            min_any_sh = min_any_sh.min(ash);
                            bounded.extract_witness_map(
                                &slot.next_or,
                                ash,
                                slot.init_undef_drop,
                                root_tn,
                                &mut extra_witnesses,
                            );
                        } else {
                            let mut vis = HashSet::new();
                            extract_mate_witness_from_tb(
                                sub_solver.tbase_mut(),
                                &slot.next_or,
                                root_tn,
                                &mut extra_witnesses,
                                &mut vis,
                            );
                        }
                    }
                    let nouse2 = if std_red {
                        unused_atk_pieces_effective(&slot.next_or, &sub_res.pv, root_tn).max(1)
                    } else {
                        0
                    };

                    if std_br_sh >= rem_sh - 2 {
                        let shadow_probe_b = eff_max_b.min(8_192);
                        // 1. Check for any shadowed `< rem_sh - 2` (`<= rem_sh - 4`) EarlyShortcut (clean or redundant)!
                        if rem_sh >= 5 {
                            let (any4_opt, a_nodes) = bounded.prove_tsume_mate_within(
                                &slot.next_or,
                                rem_sh - 4,
                                slot.init_undef_drop,
                                false,
                                shadow_probe_b,
                            );
                            *nodes_acc += a_nodes;
                            if let Some(ash) = any4_opt {
                                min_any_sh = ash;
                                bounded.extract_witness_map(
                                    &slot.next_or,
                                    ash,
                                    slot.init_undef_drop,
                                    root_tn,
                                    &mut extra_witnesses,
                                );
                            } else if require_exact_disproof {
                                let (dis4, d4_nodes) = bounded.disprove_within(
                                    &slot.next_or,
                                    rem_sh - 4,
                                    slot.init_inter_sq,
                                    eff_max_b,
                                );
                                *nodes_acc += d4_nodes;
                                if dis4 != Some(true) {
                                    if is_final_tier {
                                        return Err(());
                                    }
                                    phase_all_solved = false;
                                    continue;
                                }
                            }
                        }

                        // 2. If `std_red` OR `std_br_sh >= rem_sh`, check if a shadowed CLEAN mate `<= rem_sh - 2` exists!
                        if (std_red || std_br_sh >= rem_sh) && min_any_sh >= rem_sh - 2 {
                            let (clean2_opt, c2_nodes) = bounded.prove_tsume_mate_within(
                                &slot.next_or,
                                rem_sh - 2,
                                slot.init_undef_drop,
                                true,
                                shadow_probe_b,
                            );
                            *nodes_acc += c2_nodes;
                            if let Some(csh) = clean2_opt {
                                std_br_sh = csh;
                                std_red = false;
                                min_any_sh = min_any_sh.min(csh);
                                bounded.extract_witness_map(
                                    &slot.next_or,
                                    csh,
                                    slot.init_undef_drop,
                                    root_tn,
                                    &mut extra_witnesses,
                                );
                            } else if std_br_sh >= rem_sh && slot.is_king_or_cap {
                                if !require_exact_disproof {
                                    return Ok(AltResolved::Refuted {
                                        escaping_ev: None,
                                        used_hint: slot.from_hint,
                                    });
                                }
                                let (dis2, b2_nodes) = bounded.disprove_within(
                                    &slot.next_or,
                                    rem_sh - 2,
                                    255,
                                    eff_max_b,
                                );
                                *nodes_acc += b2_nodes;
                                if dis2 == Some(true) {
                                    return Ok(AltResolved::Refuted {
                                        escaping_ev: Some(slot.emv),
                                        used_hint: slot.from_hint,
                                    });
                                }
                                if is_final_tier {
                                    return Err(());
                                }
                                phase_all_solved = false;
                                continue;
                            }
                        }
                    }

                    slot.solved = Some(EvSolved {
                        emv: slot.emv,
                        is_king_or_cap: slot.is_king_or_cap,
                        std_br_sh,
                        std_red,
                        min_any_sh,
                        nouse2,
                        pv: sub_res.pv,
                        extra_witnesses,
                    });
                }
                _ => {
                    // `sub_solver` suspended at `b` (or errored): attempt exact depth-bounded disproof
                    // when `rem_sh <= 15` or when `require_exact_disproof` is enabled.
                    if rem_sh <= 15 || require_exact_disproof {
                        let dis_b = if require_exact_disproof {
                            b
                        } else {
                            b.min(4_096)
                        };
                        let (dis_opt, b_nodes) = bounded.disprove_within(
                            &slot.next_or,
                            rem_sh - 2,
                            slot.init_inter_sq,
                            dis_b,
                        );
                        *nodes_acc += b_nodes;
                        if dis_opt == Some(true) {
                            return Ok(AltResolved::Refuted {
                                escaping_ev: Some(slot.emv),
                                used_hint: slot.from_hint,
                            });
                        }
                    }
                    if require_exact_disproof && is_final_tier && rem_sh >= 5 {
                        let (any4, r_nodes) = bounded.prove_tsume_mate_within(
                            &slot.next_or,
                            rem_sh - 4,
                            slot.init_undef_drop,
                            false,
                            b.min(32_768),
                        );
                        *nodes_acc += r_nodes;
                        if let Some(rsh4) = any4 {
                            let mut extra_witnesses = HashMap::new();
                            bounded.extract_witness_map(
                                &slot.next_or,
                                rsh4,
                                slot.init_undef_drop,
                                root_tn,
                                &mut extra_witnesses,
                            );
                            slot.solved = Some(EvSolved {
                                emv: slot.emv,
                                is_king_or_cap: slot.is_king_or_cap,
                                std_br_sh: rsh4,
                                std_red: true,
                                min_any_sh: rsh4,
                                nouse2: 1,
                                pv: Vec::new(),
                                extra_witnesses,
                            });
                            continue;
                        }
                    }
                    if is_final_tier {
                        if require_exact_disproof
                            || bounded
                                .abort_flag
                                .as_ref()
                                .is_some_and(|f| f.load(Ordering::Relaxed))
                        {
                            return Err(());
                        }
                        return Ok(AltResolved::Refuted {
                            escaping_ev: None,
                            used_hint: false,
                        });
                    }
                    phase_all_solved = false;
                }
            }
            }

            if phase_all_solved {
                break;
            }
        }
    }

    if slots.iter().any(|s| s.solved.is_none()) {
        if require_exact_disproof
            || bounded
                .abort_flag
                .as_ref()
                .is_some_and(|f| f.load(Ordering::Relaxed))
        {
            return Err(());
        }
        return Ok(AltResolved::Refuted {
            escaping_ev: None,
            used_hint: false,
        });
    }

    let solved_evs: Vec<EvSolved> = slots.into_iter().map(|s| s.solved.unwrap()).collect();

    // Constructive Tsume-Shogi futile-interposition (`無駄合`) predicate on solved evasions:
    // 1. All-evasions 1-ply collapse (`all_futile`) applies ONLY when every evasion is constructively
    //    `合い利かず` or immediately recaptured without Defender recapture leaving extra unused pieces.
    // 2. When computing resistance across mixed evasions (`std_steps >= 3`), undefended drops
    //    (`!self_effect_contains`) that leave strictly more unused pieces than `min_nouse2`
    //    (`即時無駄合` or `遅れ無駄合`) are also excluded as futile, while defended `捨て合` and
    //    board-move `中合` are preserved unless constructively futile.
    let base_atk_hand = child_and.mkey(root_tn).total();
    let mut constructive_futile: Vec<bool> = Vec::with_capacity(solved_evs.len());
    for s in &solved_evs {
        let futile = is_futile_solved_interposition(child_and, s, base_atk_hand, scratch_tb);
        constructive_futile.push(futile);
    }

    let all_futile = !solved_evs.is_empty() && constructive_futile.iter().all(|&f| f);
    if all_futile
        || (invalid_flag
            && solved_evs
                .iter()
                .zip(constructive_futile.iter())
                .all(|(s, &f)| f || (s.std_red && !s.is_king_or_cap && s.nouse2 > base_atk_hand)))
    {
        let atk_hand_empty = base_atk_hand == 0;
        return Ok(AltResolved::Mated {
            std_steps: 1,
            non_redundant: atk_hand_empty,
            any_steps: 1,
            solved_evs,
        });
    }

    let min_nouse2 = solved_evs.iter().map(|s| s.nouse2).min().unwrap_or(0);
    let is_futile_flags: Vec<bool> = solved_evs
        .iter()
        .zip(constructive_futile.iter())
        .map(|(s, &cf)| {
            cf || is_futile_solved_interposition(child_and, s, min_nouse2, scratch_tb)
                || (s.emv.is_drop()
                    && !child_and.self_effect_contains(s.emv.dest())
                    && s.std_red
                    && s.nouse2 > min_nouse2)
        })
        .collect();

    let max_any_sh = solved_evs
        .iter()
        .zip(is_futile_flags.iter())
        .filter(|(_, &futile)| !futile)
        .map(|(s, _)| s.min_any_sh)
        .max()
        .unwrap_or(0);
    let any_steps = max_any_sh.saturating_add(2);

    let max_std_sh = solved_evs
        .iter()
        .zip(is_futile_flags.iter())
        .filter(|(_, &futile)| !futile)
        .map(|(s, _)| s.std_br_sh)
        .max()
        .unwrap_or(0);
    let std_steps = max_std_sh.saturating_add(2);
    let non_redundant = if max_std_sh <= 3 {
        // Under All-Japan Tsume-Shogi Rules Art. 5 (`最終手付近の同手数変化駒余り`),
        // equal-length dirty sidelines within `max_std_sh <= 3` (`<= 4` plies remaining)
        // are permitted if at least one longest-resistance evasion mates cleanly.
        solved_evs
            .iter()
            .zip(is_futile_flags.iter())
            .filter(|(s, &futile)| !futile && s.std_br_sh == max_std_sh)
            .any(|(s, _)| !s.std_red)
    } else {
        solved_evs
            .iter()
            .zip(is_futile_flags.iter())
            .filter(|(s, &futile)| !futile && s.std_br_sh == max_std_sh)
            .all(|(s, _)| !s.std_red)
    };

    Ok(AltResolved::Mated {
        std_steps,
        non_redundant,
        any_steps,
        solved_evs,
    })
}

fn is_constructive_futile_inter_ev(
    and_pos: &FmrsPos,
    emv: Move,
    scratch_tb: &mut TBase,
    depth: u8,
) -> bool {
    is_constructive_futile_inter_rec(and_pos, emv, None, usize::MAX, scratch_tb, depth)
}

fn is_constructive_futile_inter_rec(
    and_pos: &FmrsPos,
    emv: Move,
    prev_slider_sq: Option<usize>,
    prev_drops_len: usize,
    scratch_tb: &mut TBase,
    depth: u8,
) -> bool {
    let dest = emv.dest();
    let next_or = and_pos.move_forward(emv);
    scratch_tb.clear();
    let checks = next_or.generate_check(scratch_tb);
    for it in &checks {
        for m_rec in it.all_moves() {
            if !m_rec.is_board_move() || m_rec.dest() != dest {
                continue;
            }
            if let Some(ps) = prev_slider_sq {
                if m_rec.prev_pos as usize != ps {
                    continue;
                }
            }
            let after = next_or.move_forward(m_rec);
            scratch_tb.clear();
            let evs = after.generate_evasion_simple(scratch_tb);
            if evs.is_empty() {
                return true;
            }
            if depth >= 5 {
                continue;
            }
            let mut only_drops = true;
            let mut rep_drops: Vec<Move> = Vec::new();
            for eit in &evs {
                for e2 in eit.all_moves() {
                    if !e2.is_drop() {
                        only_drops = false;
                        break;
                    }
                    if !rep_drops.iter().any(|x| x.dest() == e2.dest()) {
                        rep_drops.push(e2);
                    }
                }
                if !only_drops {
                    break;
                }
            }
            let d_len = rep_drops.len();
            if only_drops
                && d_len > 0
                && d_len < prev_drops_len
                && rep_drops.iter().all(|&d_mv| {
                    is_constructive_futile_inter_rec(
                        &after,
                        d_mv,
                        Some(dest),
                        d_len,
                        scratch_tb,
                        depth + 1,
                    )
                })
            {
                return true;
            }
        }
    }
    false
}

fn is_futile_solved_interposition(
    child_and: &FmrsPos,
    s: &EvSolved,
    base_atk_hand: u8,
    scratch_tb: &mut TBase,
) -> bool {
    if s.is_king_or_cap {
        return false;
    }
    if is_constructive_futile_inter_ev(child_and, s.emv, scratch_tb, 0) {
        return true;
    }
    if !s.std_red || s.nouse2 <= base_atk_hand {
        return false;
    }
    let Some(&m0) = s.pv.first() else {
        return false;
    };
    if !m0.is_board_move() || m0.dest() != s.emv.dest() {
        return false;
    }
    let after_recap = child_and.move_forward(s.emv).move_forward(m0);
    scratch_tb.clear();
    let evs = after_recap.generate_evasion_simple(scratch_tb);
    !evs.iter().any(|it| {
        it.all_moves()
            .into_iter()
            .any(|em| em.is_board_move() && em.dest() == s.emv.dest())
    })
}

/// Check whether EVERY Defender-optimal non-futile evasion (`is_valid && !std_red && std_br_sh == rem_sh - 2`)
/// converges back to an OR-state on the intended PV (`in 2 plies, or in 4 plies when `rem_sh >= 5``).
pub fn all_optimal_evasions_converge_to_pv(
    child_and: &FmrsPos,
    solved_evs: &[EvSolved],
    rem_sh: u16,
    root_tn: Turn,
    pv_or_states: &HashSet<(u64, MKey, MKey, u16)>,
) -> bool {
    if rem_sh < 3 {
        return false;
    }
    let def_tn = root_tn.flip();
    let target_sh_2 = rem_sh - 2;
    let mut checked_any = false;

    for ev in solved_evs {
        if ev.std_red || ev.std_br_sh != target_sh_2 {
            continue;
        }
        checked_any = true;
        let next_or = child_and.move_forward(ev.emv);
        if pv_or_states.contains(&(
            next_or.zkey(),
            next_or.mkey(root_tn),
            next_or.mkey(def_tn),
            target_sh_2,
        )) {
            continue;
        }
        // Allow 4-ply endgame transposition (`A -> e1 -> B -> e2` vs `B -> e1 -> A -> e2`) within `endgame_transposition_rem_sh`
        if rem_sh >= 5 && ev.pv.len() >= 2 {
            let after_2 = next_or.move_forward(ev.pv[0]);
            let mut scratch = TBase::new(1);
            let (evs2, _, _) = after_2.generate_evasion(&mut scratch);
            if !evs2.is_empty()
                && evs2.iter().flat_map(|it| it.all_moves()).all(|emv2| {
                    let or4 = after_2.move_forward(emv2);
                    pv_or_states.contains(&(
                        or4.zkey(),
                        or4.mkey(root_tn),
                        or4.mkey(def_tn),
                        rem_sh - 4,
                    ))
                })
            {
                continue;
            }
        }
        return false;
    }

    checked_any
}

/// Stabilize `(mate_steps, pv)` against `shtsume`'s `make_tree_and` `inc`-skipping behavior:
/// when `bn_search_or` refutes a Defender evasion `emv` via a quick redundant (`駒余り`) mate first,
/// `make_tree_and` skips `make_tree_or` on `emv` (`if !item.inc()`), which can miss that `emv`
/// also admits a clean (`!redundant`) mate strictly longer than the extracted `pv` (`> rem_next`),
/// underestimating `res.mate_steps` and `rem_sh` on earlier OR-nodes.
///
/// Walking AND-nodes bottom-up (`from the end of `pv` toward ply 1`) detects any clean longer
/// Defender evasion `emv` (`or NoMate escape`) and splices the true Defender-optimal suffix onto `pv`.
#[allow(clippy::too_many_arguments)]
pub fn stabilize_main_line_pv(
    root: &FmrsPos,
    mut res: SearchResult,
    mut warm_root_tb: Option<&mut TBase>,
    per_check_limit: u64,
    sub_solver: &mut Solver,
    scratch_tb: &mut TBase,
    solve_nodes_acc: &mut u64,
    abort_flag: Option<&AtomicBool>,
    deadline: Option<(Instant, f64)>,
) -> Result<SearchResult, PerfectionStatus> {
    let root_tn = root.turn();
    let def_tn = root_tn.flip();
    let mut lazy_bounded: Option<BoundedDfPn> = None;

    for _pass in 0..16 {
        let mut total_phys = res.pv.len();
        if total_phys == 0 || total_phys % 2 == 0 {
            return Err(PerfectionStatus::Timeout);
        }
        {
            let mut end_cur = root.clone();
            for &mv in &res.pv {
                end_cur = end_cur.move_forward(mv);
            }
            if !playout_trailing_futile_evasions(&mut end_cur, &mut res.pv, scratch_tb) {
                return Err(PerfectionStatus::Timeout);
            }
            total_phys = res.pv.len();
        }

        let (step_delta, rem_eff) = compute_pv_effective_rem(root, &res.pv);
        res.mate_steps = rem_eff[0];
        let pv_nouse = unused_atk_pieces_effective(root, &res.pv, root_tn);
        res.redundant = pv_nouse > 0;

        let mut pv_or_dom: HashMap<u64, Vec<(MKey, MKey, u16)>> =
            HashMap::with_capacity(total_phys / 2 + 1);
        let mut and_nodes: Vec<(usize, FmrsPos, Move, u16, bool)> =
            Vec::with_capacity(total_phys / 2);
        {
            let mut cur = root.clone();
            for (ply, &mv) in res.pv.iter().enumerate() {
                if ply % 2 == 0 {
                    if step_delta[ply] == 1 {
                        pv_or_dom.entry(cur.zkey()).or_default().push((
                            cur.mkey(root_tn),
                            cur.mkey(def_tn),
                            rem_eff[ply],
                        ));
                    }
                } else {
                    let rem_next = rem_eff[ply].saturating_sub(1);
                    let cur_is_futile = step_delta[ply] == 0;
                    and_nodes.push((ply, cur.clone(), mv, rem_next, cur_is_futile));
                }
                cur = cur.move_forward(mv);
            }
        }

        let mut spliced = false;
        for &(ply, ref and_pos, pv_ev, rem_next, cur_is_futile) in and_nodes.iter().rev() {
            let checker_dest = res.pv[ply - 1].dest();
            scratch_tb.clear();
            let (evs, _, invalid_flag) = and_pos.generate_evasion(scratch_tb);
            if evs.is_empty() {
                continue;
            }
            let alt_evs: Vec<Move> = evs
                .iter()
                .flat_map(|it| it.all_moves())
                .filter(|&emv| emv != pv_ev)
                .collect();
            if alt_evs.is_empty() {
                continue;
            }

            let mut best_longer: Option<(u16, bool, Move, Vec<Move>)> = None;
            for emv in alt_evs {
                if abort_flag.is_some_and(|f| f.load(Ordering::Relaxed))
                    || deadline.is_some_and(|(t0, lim)| t0.elapsed().as_secs_f64() >= lim)
                {
                    return Err(PerfectionStatus::Timeout);
                }
                if invalid_flag && emv.is_drop() {
                    continue;
                }
                let is_king_or_cap = !emv.is_drop()
                    && (emv.dest() == checker_dest
                        || (and_pos.board_at(emv.prev_pos as usize) & 15) == SOU);
                if !is_king_or_cap && is_constructive_futile_inter_ev(and_pos, emv, scratch_tb, 0) {
                    continue;
                }
                let next_or_raw = and_pos.move_forward(emv);
                let atk_m = next_or_raw.mkey(root_tn);
                let def_m = next_or_raw.mkey(def_tn);

                // 1. Exact clean PV dominance (`same attacker hand, <= defender hand, <= rem_next`):
                if let Some(entries) = pv_or_dom.get(&next_or_raw.zkey()) {
                    if entries
                        .iter()
                        .any(|&(pa, pd, pr)| pr <= rem_next && atk_m == pa && pd.ge_all(def_m))
                    {
                        continue;
                    }
                }

                // 2. Constructive futile-interposition dominance (`打無駄合` / `移動無駄合`; defended `捨て合` excluded via `any_recap_suteai`):
                let is_undef_drop = emv.is_drop() && !and_pos.self_effect_contains(emv.dest());
                if emv.is_drop() || (!is_king_or_cap && rem_next <= 9) {
                    let stripped_kt = if emv.is_drop() {
                        emv.hand_ktype()
                    } else {
                        and_pos.board_at(emv.prev_pos as usize) & 7
                    };
                    scratch_tb.clear();
                    let recap_checks = next_or_raw.generate_check(scratch_tb);
                    let mut futile_proven = false;
                    for rit in &recap_checks {
                        for m_rec in rit.all_moves() {
                            if !m_rec.is_board_move() || m_rec.dest() != emv.dest() {
                                continue;
                            }
                            let st = next_or_raw
                                .move_forward(m_rec)
                                .without_attacker_hand_piece(stripped_kt);
                            scratch_tb.clear();
                            let (evs3, _, _) = st.generate_evasion(scratch_tb);
                            if evs3.is_empty() {
                                futile_proven = true;
                                break;
                            }
                            let mut has_non_drop = false;
                            let mut any_recap_suteai = false;
                            let mut all_dom = true;
                            for e3_it in &evs3 {
                                for e3 in e3_it.all_moves() {
                                    if !e3.is_drop() && e3.dest() == emv.dest() {
                                        any_recap_suteai = true;
                                        break;
                                    }
                                    if emv.is_drop() {
                                        if !e3.is_drop() {
                                            has_non_drop = true;
                                            let or2 = st.move_forward(e3);
                                            let a2 = or2.mkey(root_tn);
                                            let d2 = or2.mkey(def_tn);
                                            let dom = pv_or_dom.get(&or2.zkey()).is_some_and(
                                                |ents| {
                                                    ents.iter().any(|&(pa, pd, _)| {
                                                        a2.ge_all(pa) && pd.ge_all(d2)
                                                    })
                                                },
                                            );
                                            if !dom {
                                                all_dom = false;
                                                break;
                                            }
                                        }
                                    } else {
                                        if !e3.is_drop() {
                                            has_non_drop = true;
                                        } else if is_constructive_futile_inter_ev(
                                            &st, e3, scratch_tb, 0,
                                        ) {
                                            continue;
                                        }
                                        let or2 = st.move_forward(e3).with_count(2);
                                        let a2 = or2.mkey(root_tn);
                                        let d2 = or2.mkey(def_tn);
                                        let mut dom =
                                            pv_or_dom.get(&or2.zkey()).is_some_and(|ents| {
                                                ents.iter().any(|&(pa, pd, pr)| {
                                                    pr <= rem_next
                                                        && pd.ge_all(d2)
                                                        && if res.redundant {
                                                            a2.ge_all(pa)
                                                        } else {
                                                            a2 == pa
                                                        }
                                                })
                                            });
                                        if !dom {
                                            let bnd =
                                                lazy_bounded.get_or_insert_with(BoundedDfPn::new);
                                            let (m_opt, bn) = bnd.prove_tsume_mate_within(
                                                &or2,
                                                rem_next,
                                                None,
                                                !res.redundant,
                                                1_024,
                                            );
                                            *solve_nodes_acc += bn;
                                            dom = m_opt.is_some();
                                        }
                                        if !dom {
                                            all_dom = false;
                                            break;
                                        }
                                    }
                                }
                                if any_recap_suteai || !all_dom {
                                    break;
                                }
                            }
                            if !any_recap_suteai && has_non_drop && all_dom {
                                futile_proven = true;
                                break;
                            }
                        }
                        if futile_proven {
                            break;
                        }
                    }
                    if futile_proven {
                        continue;
                    }
                }

                // 3. Warm root `TBase` fast skip for deep already-clean `make_tree_or`-expanded siblings:
                if rem_next > 25 {
                    if let Some(tb) = warm_root_tb.as_deref_mut() {
                        let src_koma = if emv.is_board_move() {
                            and_pos.board_at(emv.prev_pos as usize)
                        } else {
                            0
                        };
                        let cap_koma = and_pos.board_at(emv.dest());
                        let mut probe_it = MvItem::single_with_hints(emv, src_koma, cap_koma);
                        let child_ks = and_pos.key_forward_item(&probe_it);
                        tb.make_tree_lookup(&child_ks, &mut probe_it, root_tn);
                        if probe_it.tdata.pn == 0
                            && !probe_it.inc()
                            && probe_it.tdata.sh <= rem_next
                        {
                            continue;
                        }
                    }
                }

                // 4. Solve `next_or` to check for `NoMate` or a strictly longer optimal defense:
                let b_and = if !is_undef_drop {
                    if rem_next <= 15 {
                        per_check_limit.clamp(80_000, 524_288)
                    } else if rem_next <= 35 {
                        per_check_limit.clamp(32_768, 524_288)
                    } else {
                        per_check_limit.clamp(32_768, 131_072)
                    }
                } else if rem_next <= 35 {
                    per_check_limit.clamp(16_384, 131_072)
                } else {
                    16_384
                };
                let next_or = next_or_raw.with_count(2);
                sub_solver.set_node_limit(Some(b_and));
                let mut r2 = sub_solver.solve_unbounded_pn(&next_or);
                *solve_nodes_acc += r2.nodes;
                if r2.mate == Some(true) && sub_solver.is_suspended() && !r2.error {
                    sub_solver.set_node_limit(Some(b_and.saturating_add(65_536)));
                    r2 = sub_solver.solve_unbounded_pn(&next_or);
                    *solve_nodes_acc += r2.nodes;
                }
                if r2.mate == Some(false) && !sub_solver.is_suspended() && !r2.error {
                    return Err(PerfectionStatus::NoMate);
                }
                if r2.mate == Some(true)
                    && !sub_solver.is_suspended()
                    && !r2.error
                    && r2.mate_steps % 2 == 1
                    && !r2.pv.is_empty()
                    && r2.pv.len() % 2 == 1
                {
                    let init_ud: Option<(u8, u8)> = None;
                    if !r2.redundant {
                        let mut true_steps = r2.mate_steps;
                        if true_steps > rem_next && !cur_is_futile && rem_next <= 11 {
                            let bnd = lazy_bounded.get_or_insert_with(BoundedDfPn::new);
                            let (any_le_rem, an) = bnd.prove_tsume_mate_within(
                                &next_or, rem_next, init_ud, false, 4_096,
                            );
                            *solve_nodes_acc += an;
                            if let Some(ash) = any_le_rem {
                                if ash < rem_next || rem_next <= 3 || emv.is_drop() {
                                    continue;
                                }
                                let (cl_le_rem, cn) = bnd.prove_tsume_mate_within(
                                    &next_or, rem_next, init_ud, true, 4_096,
                                );
                                *solve_nodes_acc += cn;
                                if cl_le_rem.is_some() {
                                    continue;
                                }
                            }
                        }
                        let beats =
                            true_steps > rem_next || (cur_is_futile && true_steps >= rem_next);
                        if beats
                            && best_longer
                                .as_ref()
                                .is_none_or(|b| (true_steps, false) > (b.0, b.1))
                        {
                            true_steps = r2.mate_steps;
                            best_longer = Some((true_steps, false, emv, r2.pv));
                        }
                    } else if r2.mate_steps <= 13 && rem_next <= 9 {
                        // `r2.redundant == true`: `shtsume`'s `super_tsumi` may have returned a
                        // shorter dirty mate `<= rem_next` when the true CLEAN mate is `> rem_next`!
                        let r2_nouse =
                            unused_atk_pieces_effective(&next_or, &r2.pv, root_tn).max(1);
                        if is_undef_drop && r2_nouse > pv_nouse {
                            continue;
                        }
                        let bnd = lazy_bounded.get_or_insert_with(BoundedDfPn::new);
                        if !cur_is_futile {
                            let (any_le, an) = bnd.prove_tsume_mate_within(
                                &next_or, rem_next, init_ud, false, 4_096,
                            );
                            *solve_nodes_acc += an;
                            if let Some(ash) = any_le {
                                // By Tsume-Shogi Rules Art. 5:
                                // - Strictly shorter variations (`ash < rem_next`) mate in `ash`
                                //   plies even with surplus pieces (never stretched by piece-dumping).
                                // - Equal-length (`ash == rem_next`) allows surplus pieces at
                                //   `rem_next <= 3` (`最終手付近`) or on drop interpositions.
                                if ash < rem_next || rem_next <= 3 || emv.is_drop() {
                                    continue;
                                }
                                let (cl_le, cn) = bnd.prove_tsume_mate_within(
                                    &next_or, rem_next, init_ud, true, 4_096,
                                );
                                *solve_nodes_acc += cn;
                                if cl_le.is_some() {
                                    continue;
                                }
                            }
                        }
                        let probe_cap = r2.mate_steps.max(rem_next.saturating_add(4)).min(13);
                        let (clean_opt, b_nodes) =
                            bnd.prove_tsume_mate_within(&next_or, probe_cap, init_ud, true, 4_096);
                        *solve_nodes_acc += b_nodes;
                        if let Some(csh) = clean_opt {
                            if csh > rem_next || (cur_is_futile && csh >= rem_next) {
                                let mut witnesses = HashMap::new();
                                bnd.extract_witness_map(
                                    &next_or,
                                    csh,
                                    init_ud,
                                    root_tn,
                                    &mut witnesses,
                                );
                                if let Some(w_pv) = extract_witness_line(
                                    &next_or,
                                    root_tn,
                                    csh as usize,
                                    &witnesses,
                                    true,
                                    scratch_tb,
                                ) {
                                    if let Some((clean_pv, c_eff)) =
                                        validate_and_measure_mate_line(&next_or, &w_pv, scratch_tb)
                                    {
                                        let eff_sh = c_eff as u16;
                                        if (eff_sh > rem_next
                                            || (cur_is_futile && eff_sh >= rem_next))
                                            && best_longer
                                                .as_ref()
                                                .is_none_or(|b| (eff_sh, false) > (b.0, b.1))
                                        {
                                            best_longer = Some((eff_sh, false, emv, clean_pv));
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            if let Some((_, best_red, best_emv, best_pv)) = best_longer {
                res.pv.truncate(ply);
                res.pv.push(best_emv);
                res.pv.extend(best_pv);
                res.mate_steps = effective_tsume_pv_plies(root, &res.pv);
                if best_red {
                    res.redundant = true;
                } else {
                    res.redundant = unused_atk_pieces_effective(root, &res.pv, root_tn) > 0;
                }
                warm_root_tb = None;
                spliced = true;
                break;
            }
        }

        if !spliced {
            break;
        }
    }

    // Phase 2 (short problems `<= 11` plies only, AFTER clean stabilization has converged so no
    // suboptimal Defender branch can shadow a longer clean branch): check whether any King/capture
    // evasion on the final stabilized PV forces a dirty mate (`rem_next > 3` equal-length or strictly longer).
    if !res.redundant && res.mate_steps <= 11 {
        let (step_delta, rem_eff) = compute_pv_effective_rem(root, &res.pv);
        let mut cur = root.clone();
        for (ply, &mv) in res.pv.iter().enumerate() {
            if ply % 2 == 1 && step_delta[ply] == 1 {
                let rem_next = rem_eff[ply].saturating_sub(1);
                let checker_dest = res.pv[ply - 1].dest();
                scratch_tb.clear();
                let (evs, _, _) = cur.generate_evasion(scratch_tb);
                let mut forced_dirty = false;
                for it in &evs {
                    for emv in it.all_moves() {
                        if emv == mv || emv.is_drop() {
                            continue;
                        }
                        let is_king_or_cap = emv.dest() == checker_dest
                            || (cur.board_at(emv.prev_pos as usize) & 15) == SOU;
                        if !is_king_or_cap {
                            continue;
                        }
                        let next_or = cur.move_forward(emv).with_count(2);
                        let bnd = lazy_bounded.get_or_insert_with(BoundedDfPn::new);
                        // If a clean mate `<= rem_next` exists, Defender cannot force dirty here.
                        let (cl_opt, cn) =
                            bnd.prove_tsume_mate_within(&next_or, rem_next, None, true, 4_096);
                        *solve_nodes_acc += cn;
                        if cl_opt.is_some() || bnd.suspended {
                            continue;
                        }
                        let (any_opt, an) =
                            bnd.prove_tsume_mate_within(&next_or, rem_next, None, false, 4_096);
                        *solve_nodes_acc += an;
                        if let Some(ash) = any_opt {
                            if ash < rem_next || rem_next <= 3 {
                                continue;
                            }
                            // `ash == rem_next && rem_next > 3` with NO clean mate `<= rem_next`:
                            // verify no longer clean mate exists within `rem_next + 4` either!
                            let (cl_long, cln) = bnd.prove_tsume_mate_within(
                                &next_or,
                                rem_next.saturating_add(4).min(13),
                                None,
                                true,
                                4_096,
                            );
                            *solve_nodes_acc += cln;
                            if cl_long.is_none() && !bnd.suspended {
                                forced_dirty = true;
                                break;
                            }
                        } else if !bnd.suspended {
                            // Neither clean nor dirty mate within `<= rem_next` (`strictly > rem_next` dirty escape!):
                            let (cl_long, cln) = bnd.prove_tsume_mate_within(
                                &next_or,
                                rem_next.saturating_add(6).min(13),
                                None,
                                true,
                                4_096,
                            );
                            *solve_nodes_acc += cln;
                            if cl_long.is_none() && !bnd.suspended {
                                forced_dirty = true;
                                break;
                            }
                        }
                    }
                    if forced_dirty {
                        break;
                    }
                }
                if forced_dirty {
                    res.redundant = true;
                    break;
                }
            }
            cur = cur.move_forward(mv);
        }
    }

    res.mate_steps = effective_tsume_pv_plies(root, &res.pv);
    if !res.redundant {
        res.redundant = unused_atk_pieces_effective(root, &res.pv, root_tn) > 0;
    }
    if res.redundant && res.mate_steps <= 13 {
        let bnd = lazy_bounded.get_or_insert_with(BoundedDfPn::new);
        bnd.clear_for_root();
        let probe_cap = res.mate_steps.saturating_add(2).min(15);
        let (clean_opt, b_nodes) =
            bnd.prove_tsume_mate_within(root, probe_cap, None, true, 8_192);
        *solve_nodes_acc += b_nodes;
        if let Some(csh) = clean_opt {
            if csh <= res.mate_steps {
                let mut witnesses = HashMap::new();
                bnd.extract_witness_map(root, csh, None, root_tn, &mut witnesses);
                if let Some(w_pv) = extract_witness_line(
                    root,
                    root_tn,
                    csh as usize,
                    &witnesses,
                    true,
                    scratch_tb,
                ) {
                    if let Some((clean_pv, c_eff)) =
                        validate_and_measure_mate_line(root, &w_pv, scratch_tb)
                    {
                        res.pv = clean_pv;
                        res.mate_steps = c_eff as u16;
                        res.redundant = false;
                    }
                }
            }
        }
    }
    if res.redundant {
        return Err(PerfectionStatus::RedundantHand);
    }
    Ok(res)
}

/// Validate a checkmate line `raw_tail` from `start_or`, ensuring the returned sequence terminates
/// at a physical Shogi checkmate (`generate_evasion_simple.is_empty()`, playing out any trailing
/// `合い利かず` drops if `raw_tail` stopped at Tsume-Shogi mate) and returning
/// `(complete_tail, effective_tsume_plies)` where futile interposition+recapture pairs count as `0`.
fn validate_and_measure_mate_line(
    start_or: &FmrsPos,
    raw_tail: &[Move],
    scratch: &mut TBase,
) -> Option<(Vec<Move>, usize)> {
    if raw_tail.is_empty() {
        return None;
    }
    let mut cur = start_or.clone();
    let mut out = Vec::with_capacity(raw_tail.len() + 8);

    for (i, &m) in raw_tail.iter().enumerate() {
        scratch.clear();
        let legal = if i % 2 == 0 {
            cur.generate_check(scratch)
                .into_iter()
                .flat_map(|it| it.all_moves())
                .any(|x| x == m)
        } else {
            cur.generate_evasion_simple(scratch)
                .into_iter()
                .flat_map(|it| it.all_moves())
                .any(|x| x == m)
        };
        if !legal {
            return None;
        }
        out.push(m);
        cur = cur.move_forward(m);
        if i % 2 == 0 {
            scratch.clear();
            if cur.generate_evasion_simple(scratch).is_empty() {
                let eff = effective_tsume_pv_plies(start_or, &out) as usize;
                return Some((out, eff));
            }
        }
    }

    if out.len() % 2 == 1 && playout_trailing_futile_evasions(&mut cur, &mut out, scratch) {
        let eff = effective_tsume_pv_plies(start_or, &out) as usize;
        return Some((out, eff));
    }
    None
}

fn find_1ply_witness_move(
    pos: &FmrsPos,
    root_tn: Turn,
    prefer_clean: bool,
    scratch: &mut TBase,
) -> Option<Move> {
    if prefer_clean {
        let hand = pos.mkey(root_tn);
        if let Some(m1) = pos.find_1ply_mate() {
            let clean = (m1.mv.is_board_move() && m1.cap_koma == 0 && hand.is_empty())
                || (m1.mv.is_drop() && hand.total() == 1);
            if clean {
                return Some(m1.mv);
            }
        }
        scratch.clear();
        for it in pos.generate_check(scratch) {
            for mv in it.all_moves() {
                let c = pos.move_forward(mv);
                scratch.clear();
                if c.mkey(root_tn).is_empty() && c.generate_evasion(scratch).0.is_empty() {
                    return Some(mv);
                }
            }
        }
        None
    } else {
        pos.find_1ply_mate_simple().map(|m1| m1.mv)
    }
}

fn extract_witness_line(
    start_or: &FmrsPos,
    root_tn: Turn,
    max_plies: usize,
    witnesses: &HashMap<(u64, MKey), Move>,
    prefer_clean: bool,
    scratch: &mut TBase,
) -> Option<Vec<Move>> {
    let phys_cap = max_plies.saturating_add(14);
    let mut active = HashSet::new();
    let mut memo: HashMap<(u64, MKey, usize), Option<(u16, Vec<Move>)>> = HashMap::new();
    let mut node_budget = 512usize;
    extract_witness_line_rec(
        start_or,
        root_tn,
        phys_cap,
        witnesses,
        prefer_clean,
        scratch,
        &mut active,
        &mut memo,
        &mut node_budget,
    )
    .map(|(_, line)| line)
}

#[allow(clippy::too_many_arguments)]
fn extract_witness_line_rec(
    cur: &FmrsPos,
    root_tn: Turn,
    rem_phys: usize,
    witnesses: &HashMap<(u64, MKey), Move>,
    prefer_clean: bool,
    scratch: &mut TBase,
    active: &mut HashSet<(u64, MKey)>,
    memo: &mut HashMap<(u64, MKey, usize), Option<(u16, Vec<Move>)>>,
    node_budget: &mut usize,
) -> Option<(u16, Vec<Move>)> {
    if rem_phys == 0 || *node_budget == 0 {
        return None;
    }
    let state_key = (cur.zkey(), cur.mkey(root_tn));
    if let Some(cached) = memo.get(&(state_key.0, state_key.1, rem_phys)) {
        return cached.clone();
    }
    if !active.insert(state_key) {
        return None;
    }
    *node_budget = node_budget.saturating_sub(1);

    let cmv_opt = if let Some(&w) = witnesses.get(&state_key) {
        scratch.clear();
        let ok = cur
            .generate_check(scratch)
            .into_iter()
            .flat_map(|it| it.all_moves())
            .any(|x| x == w);
        if ok {
            Some(w)
        } else {
            find_1ply_witness_move(cur, root_tn, prefer_clean, scratch)
        }
    } else {
        find_1ply_witness_move(cur, root_tn, prefer_clean, scratch)
    };
    let Some(cmv) = cmv_opt else {
        active.remove(&state_key);
        memo.insert((state_key.0, state_key.1, rem_phys), None);
        return None;
    };

    let cand = cur.move_forward(cmv);
    scratch.clear();
    let (evs, _, invalid_flag) = cand.generate_evasion(scratch);
    if evs.is_empty() {
        active.remove(&state_key);
        let res = Some((1u16, vec![cmv]));
        memo.insert((state_key.0, state_key.1, rem_phys), res.clone());
        return res;
    }
    if rem_phys < 3 {
        active.remove(&state_key);
        memo.insert((state_key.0, state_key.1, rem_phys), None);
        return None;
    }

    let all_emvs: Vec<Move> = evs.into_iter().flat_map(|it| it.all_moves()).collect();
    let mut best_non_futile: Option<(u16, bool, Move, Vec<Move>)> = None;
    let mut best_any: Option<(u16, bool, Move, Vec<Move>)> = None;

    for e in all_emvs {
        if invalid_flag && e.is_drop() {
            continue;
        }
        // Deduplicate same-square drops if we already evaluated a drop on `e.dest()`
        if e.is_drop()
            && best_any
                .as_ref()
                .is_some_and(|b| b.2.is_drop() && b.2.dest() == e.dest())
        {
            continue;
        }
        let n = cand.move_forward(e);
        if !(witnesses.contains_key(&(n.zkey(), n.mkey(root_tn)))
            || find_1ply_witness_move(&n, root_tn, prefer_clean, scratch).is_some())
        {
            continue;
        }
        let Some((sub_eff, sub_line)) = extract_witness_line_rec(
            &n,
            root_tn,
            rem_phys - 2,
            witnesses,
            prefer_clean,
            scratch,
            active,
            memo,
            node_budget,
        ) else {
            continue;
        };
        let is_king_or_cap = !e.is_drop()
            && (e.dest() == cmv.dest() || (cand.board_at(e.prev_pos as usize) & 15) == SOU);
        let is_futile = !is_king_or_cap && is_constructive_futile_inter_ev(&cand, e, scratch, 0);
        let step_eff = if is_futile {
            sub_eff.saturating_sub(1).max(1)
        } else {
            sub_eff.saturating_add(2)
        };
        let non_drop = !e.is_drop();
        if !is_futile
            && best_non_futile
                .as_ref()
                .is_none_or(|b| (step_eff, non_drop) > (b.0, b.1))
        {
            best_non_futile = Some((step_eff, non_drop, e, sub_line.clone()));
        }
        if best_any
            .as_ref()
            .is_none_or(|b| (step_eff, non_drop) > (b.0, b.1))
        {
            best_any = Some((step_eff, non_drop, e, sub_line));
        }
    }

    active.remove(&state_key);
    let chosen = best_non_futile.or(best_any);
    let res = chosen.map(|(eff, _, ev, mut tail)| {
        let mut line = Vec::with_capacity(2 + tail.len());
        line.push(cmv);
        line.push(ev);
        line.append(&mut tail);
        (eff, line)
    });
    memo.insert((state_key.0, state_key.1, rem_phys), res.clone());
    res
}

/// Reconstruct a complete legal checkmating move sequence (`pv[..first_ply] + [alt_move, ...]`)
/// demonstrating a detected `Shortcut` for KIF fork visualization.
#[allow(clippy::too_many_arguments)]
pub fn build_shortcut_pv(
    root: &FmrsPos,
    main_pv: &[Move],
    first_ply: u16,
    alt_move: Move,
    kind: FlawKind,
    rem_sh: u16,
    solved_evs: &[EvSolved],
    scratch_tb: &mut TBase,
) -> Vec<Move> {
    let fp = first_ply as usize;
    if fp > main_pv.len() {
        return Vec::new();
    }
    let root_tn = root.turn();
    let mut spv: Vec<Move> = main_pv[..fp].to_vec();
    spv.push(alt_move);

    let mut child_and = root.clone();
    for &m in &spv {
        child_and = child_and.move_forward(m);
    }
    scratch_tb.clear();
    let (raw_evs, _, _) = child_and.generate_evasion(scratch_tb);
    if raw_evs.is_empty() {
        let _ = playout_trailing_futile_evasions(&mut child_and, &mut spv, scratch_tb);
        return spv;
    }

    if solved_evs.is_empty() {
        return Vec::new();
    }

    let base_atk_hand = child_and.mkey(root_tn).total();
    let constructive_futile: Vec<bool> = solved_evs
        .iter()
        .map(|s| is_futile_solved_interposition(&child_and, s, base_atk_hand, scratch_tb))
        .collect();
    if constructive_futile.iter().all(|&f| f) {
        if playout_trailing_futile_evasions(&mut child_and, &mut spv, scratch_tb) {
            return spv;
        }
    }
    let min_nouse2 = solved_evs.iter().map(|s| s.nouse2).min().unwrap_or(0);
    let valid_flags: Vec<bool> = solved_evs
        .iter()
        .zip(constructive_futile.iter())
        .map(|(s, &cf)| {
            !(cf || is_futile_solved_interposition(&child_and, s, min_nouse2, scratch_tb)
                || (s.emv.is_drop()
                    && !child_and.self_effect_contains(s.emv.dest())
                    && s.std_red
                    && s.nouse2 > min_nouse2))
        })
        .collect();

    let best_ev: &EvSolved = match kind {
        FlawKind::EqualLengthShortcut => solved_evs
            .iter()
            .zip(valid_flags.iter())
            .filter(|(s, &v)| v && !s.std_red)
            .max_by_key(|(s, _)| (s.std_br_sh, s.is_king_or_cap))
            .map(|(s, _)| s)
            .unwrap_or(&solved_evs[0]),
        FlawKind::EarlyShortcut => {
            let max_std = solved_evs
                .iter()
                .zip(valid_flags.iter())
                .filter(|(_, &v)| v)
                .map(|(s, _)| s.std_br_sh)
                .max()
                .unwrap_or(0);
            if max_std.saturating_add(2) < rem_sh {
                solved_evs
                    .iter()
                    .zip(valid_flags.iter())
                    .filter(|(_, &v)| v)
                    .max_by_key(|(s, _)| (s.std_br_sh, s.is_king_or_cap))
                    .map(|(s, _)| s)
                    .unwrap_or(&solved_evs[0])
            } else {
                solved_evs
                    .iter()
                    .zip(valid_flags.iter())
                    .filter(|(_, &v)| v)
                    .max_by_key(|(s, _)| (s.min_any_sh, s.is_king_or_cap))
                    .map(|(s, _)| s)
                    .unwrap_or(&solved_evs[0])
            }
        }
    };

    spv.push(best_ev.emv);
    let next_or = child_and.move_forward(best_ev.emv).with_count(2);

    let max_tail = match kind {
        FlawKind::EarlyShortcut => rem_sh.saturating_sub(3) as usize,
        FlawKind::EqualLengthShortcut => rem_sh.saturating_sub(2) as usize,
    };

    // 1. Direct `best_ev.pv` if already a complete mate within the effective Tsume-Shogi bound:
    if let Some((clean_tail, eff_len)) =
        validate_and_measure_mate_line(&next_or, &best_ev.pv, scratch_tb)
    {
        if eff_len <= max_tail {
            spv.extend(clean_tail);
            return spv;
        }
    }

    // 2. Convergent `conv_pv_pair` (`0` or `2` plies onto `main_pv`):
    if best_ev.pv.len() % 2 == 0 && best_ev.pv.len() <= max_tail {
        let mut conv_or = next_or.clone();
        let mut legal_pair = true;
        for (idx, &m) in best_ev.pv.iter().enumerate() {
            scratch_tb.clear();
            let ok = if idx % 2 == 0 {
                conv_or
                    .generate_check(scratch_tb)
                    .into_iter()
                    .flat_map(|it| it.all_moves())
                    .any(|x| x == m)
            } else {
                conv_or
                    .generate_evasion_simple(scratch_tb)
                    .into_iter()
                    .flat_map(|it| it.all_moves())
                    .any(|x| x == m)
            };
            if !ok {
                legal_pair = false;
                break;
            }
            conv_or = conv_or.move_forward(m);
        }
        if legal_pair {
            let mut mcur = root.clone();
            for (k, &mm) in main_pv.iter().enumerate() {
                if k % 2 == 0 && mcur.zkey() == conv_or.zkey() {
                    let mut combined = best_ev.pv.clone();
                    combined.extend_from_slice(&main_pv[k..]);
                    if let Some((clean_tail, eff_len)) =
                        validate_and_measure_mate_line(&next_or, &combined, scratch_tb)
                    {
                        if eff_len <= max_tail {
                            spv.extend(clean_tail);
                            return spv;
                        }
                    }
                }
                mcur = mcur.move_forward(mm);
            }
        }
    }

    // 3. Shadowed `BoundedDfPn` witness map (`extra_witnesses`):
    if !best_ev.extra_witnesses.is_empty() {
        let prefer_clean = kind == FlawKind::EqualLengthShortcut;
        if let Some(wtail) = extract_witness_line(
            &next_or,
            root_tn,
            max_tail,
            &best_ev.extra_witnesses,
            prefer_clean,
            scratch_tb,
        ) {
            if let Some((clean_tail, eff_len)) =
                validate_and_measure_mate_line(&next_or, &wtail, scratch_tb)
            {
                if eff_len <= max_tail {
                    spv.extend(clean_tail);
                    return spv;
                }
            }
        }
    }

    // 4. Fallback `simple_mate` / standard solver on `next_or`:
    for try_simple in [kind == FlawKind::EarlyShortcut, kind != FlawKind::EarlyShortcut] {
        let mut fb_solver = Solver::new(SearchConfig {
            hash_mb: 8,
            node_limit: Some(65_536),
            simple_mate: try_simple,
            search_level: 1,
            use_ml_ordering: true,
            ..Default::default()
        });
        let fb_res = fb_solver.solve_unbounded_pn(&next_or);
        if fb_res.mate == Some(true) {
            if let Some((clean_tail, eff_len)) =
                validate_and_measure_mate_line(&next_or, &fb_res.pv, scratch_tb)
            {
                if eff_len <= max_tail {
                    spv.extend(clean_tail);
                    return spv;
                }
            }
        }
    }

    Vec::new()
}

/// Format a `futsuu::Move` at `pos_at_ply` into unambiguous Japanese KIF-style notation
/// (e.g. `▲２二成銀(33)`, `▲２四桂打`, `▲４一飛不成(44)`, `▲４六金(57)`).
pub fn format_futsuu_move_ja(pos_at_ply: &PositionAux, mv: Move) -> String {
    const ZEN_FILES: [&str; 9] = ["１", "２", "３", "４", "５", "６", "７", "８", "９"];
    const KAN_RANKS: [&str; 9] = ["一", "二", "三", "四", "五", "六", "七", "八", "九"];

    fn kind_ja(k: Kind) -> &'static str {
        match k {
            Kind::Pawn => "歩",
            Kind::Lance => "香",
            Kind::Knight => "桂",
            Kind::Silver => "銀",
            Kind::Gold => "金",
            Kind::Bishop => "角",
            Kind::Rook => "飛",
            Kind::King => "玉",
            Kind::ProPawn => "と",
            Kind::ProLance => "成香",
            Kind::ProKnight => "成桂",
            Kind::ProSilver => "成銀",
            Kind::ProBishop => "馬",
            Kind::ProRook => "竜",
        }
    }

    let is_black = pos_at_ply.turn() == Color::BLACK;
    let mark = if is_black { "▲" } else { "▽" };
    match move_to_movement(mv) {
        Movement::Drop(dest, kind) => format!(
            "{}{}{}{}打",
            mark,
            ZEN_FILES[dest.col()],
            KAN_RANKS[dest.row()],
            kind_ja(kind)
        ),
        Movement::Move {
            source,
            dest,
            promote,
            ..
        } => {
            let (_, k) = pos_at_ply.get(source).unwrap_or((Color::BLACK, Kind::Pawn));
            let in_prom_zone = if is_black {
                source.row() < 3 || dest.row() < 3
            } else {
                source.row() >= 6 || dest.row() >= 6
            };
            let can_prom = matches!(
                k,
                Kind::Pawn
                    | Kind::Lance
                    | Kind::Knight
                    | Kind::Silver
                    | Kind::Bishop
                    | Kind::Rook
            ) && in_prom_zone;
            let prom_str = if promote {
                "成"
            } else if can_prom {
                "不成"
            } else {
                ""
            };
            format!(
                "{}{}{}{}{}{}({}{})",
                mark,
                ZEN_FILES[dest.col()],
                KAN_RANKS[dest.row()],
                kind_ja(k),
                prom_str,
                "",
                source.col() + 1,
                source.row() + 1
            )
        }
    }
}

/// Exact O(1) backward-transposition refutation:
/// If Defender has a valid King move or checker capture (`is_king_or_cap`, never `無駄合`)
/// leading to an OR-position `q` dominated by any earlier/current PV OR-state `P_k` (`k <= ply`),
/// then `alt` cannot be a shortest/equal-length shortcut (`>= rem_sh + 2`), or else `P_k` itself
/// admits a strictly earlier/shorter shortcut caught at `k <= ply`.
pub fn is_refuted_by_earlier_pv(
    child_and: &FmrsPos,
    checker_dest: usize,
    root_tn: Turn,
    def_tn: Turn,
    ply: u16,
    pv_or_dom: &HashMap<u64, Vec<(MKey, MKey, u16)>>,
    scratch_tb: &mut TBase,
) -> bool {
    scratch_tb.clear();
    let (evs, _, _) = child_and.generate_evasion(scratch_tb);
    for ev in &evs {
        for emv in ev.all_moves() {
            let is_king_or_cap = !emv.is_drop()
                && (emv.dest() == checker_dest
                    || (child_and.board_at(emv.prev_pos as usize) & 15) == SOU);
            if !is_king_or_cap {
                continue;
            }
            let q = child_and.move_forward(emv);
            if let Some(entries) = pv_or_dom.get(&q.zkey()) {
                let qa = q.mkey(root_tn);
                let qd = q.mkey(def_tn);
                if entries
                    .iter()
                    .any(|&(pa, pd, p_ply)| p_ply <= ply && pa.ge_all(qa) && qd.ge_all(pd))
                {
                    return true;
                }
            }
        }
    }
    false
}

/// Recover a short (`<= 13`-ply) clean Tsume-Shogi mate at `root` when `res.redundant` was set
/// due to `shtsume`'s `super_tsumi` shadowing or King-adjacent futile interpositions.
pub fn recover_short_clean_root_if_needed(
    root: &FmrsPos,
    res: &mut SearchResult,
    warm_tbase: &mut bool,
    sub_solver: &mut Solver,
    bounded: &mut BoundedDfPn,
    scratch_tb: &mut TBase,
    nodes_acc: &mut u64,
) {
    let root_tn = root.turn();
    if !res.pv.is_empty() && res.pv.len() % 2 == 1 {
        res.mate_steps = effective_tsume_pv_plies(root, &res.pv);
        res.redundant = unused_atk_pieces_effective(root, &res.pv, root_tn) > 0;
    }

    if res.redundant && res.mate_steps <= 13 {
        bounded.clear_for_root();
        let probe_cap = res.mate_steps.saturating_add(2).min(15);
        let (clean_opt, b_nodes) =
            bounded.prove_tsume_mate_within(root, probe_cap, None, true, 8_192);
        *nodes_acc += b_nodes;
        if let Some(csh) = clean_opt {
            let mut witnesses = HashMap::new();
            bounded.extract_witness_map(root, csh, None, root_tn, &mut witnesses);
            if let Some(w_pv) =
                extract_witness_line(root, root_tn, csh as usize, &witnesses, true, scratch_tb)
            {
                if let Some((clean_pv, c_eff)) =
                    validate_and_measure_mate_line(root, &w_pv, scratch_tb)
                {
                    res.pv = clean_pv;
                    res.mate_steps = c_eff as u16;
                    res.redundant = false;
                    *warm_tbase = false;
                }
            }
        }
    }

    if res.redundant && res.mate_steps <= 13 {
        scratch_tb.clear();
        let root_checks = root.generate_check(scratch_tb);
        let empty_states = HashSet::new();
        let mut best_clean_root: Option<(u16, Vec<Move>)> = None;
        for item in &root_checks {
            for mv in item.all_moves() {
                let child_and = root.move_forward(mv);
                if let Ok(AltResolved::Mated {
                    std_steps,
                    non_redundant: true,
                    any_steps,
                    solved_evs,
                }) = resolve_alt_check_sound(
                    &child_and,
                    mv.dest(),
                    res.mate_steps.saturating_add(2),
                    root_tn,
                    None,
                    &empty_states,
                    32_768,
                    false,
                    sub_solver,
                    bounded,
                    scratch_tb,
                    nodes_acc,
                ) {
                    if std_steps <= res.mate_steps
                        && any_steps >= std_steps
                        && best_clean_root
                            .as_ref()
                            .is_none_or(|b| std_steps < b.0)
                    {
                        let sc_pv = build_shortcut_pv(
                            root,
                            &res.pv,
                            0,
                            mv,
                            FlawKind::EqualLengthShortcut,
                            std_steps,
                            &solved_evs,
                            scratch_tb,
                        );
                        if !sc_pv.is_empty()
                            && sc_pv.len() % 2 == 1
                            && unused_atk_pieces_effective(root, &sc_pv, root_tn) == 0
                        {
                            best_clean_root = Some((std_steps, sc_pv));
                        }
                    }
                }
            }
        }
        if let Some((c_steps, c_pv)) = best_clean_root {
            res.pv = c_pv;
            res.mate_steps = c_steps;
            res.redundant = false;
            *warm_tbase = false;
        }
    }
}

pub struct ConventionalVerifier {
    cfg: PerfectionConfig,
    require_exact_disproof: bool,
    threads: usize,
    time_limit_secs: Option<f64>,
    solve_solver: Solver,
    check_solver: Solver,
    sub_solver: Solver,
    bounded: BoundedDfPn,
    scratch_tb: TBase,
    check_solver_nodes_acc: u64,
    abort_flag: Option<Arc<AtomicBool>>,
    progress_cb: Option<Arc<dyn Fn(u64) + Send + Sync>>,
}

impl ConventionalVerifier {
    pub fn new(cfg: PerfectionConfig) -> Self {
        let solve_solver = Solver::new(SearchConfig {
            hash_mb: cfg.hash_mb,
            node_limit: Some(cfg.solve_node_limit),
            search_level: cfg.search_level,
            use_ml_ordering: true,
            ..Default::default()
        });
        let check_solver = Solver::new(SearchConfig {
            hash_mb: cfg.hash_mb,
            node_limit: Some(2_048),
            search_level: 0,
            use_ml_ordering: true,
            ..Default::default()
        });
        let sub_solver = Solver::new(SearchConfig {
            hash_mb: (cfg.hash_mb / 2).max(16),
            node_limit: Some(cfg.check_node_limit_per_move.max(2_048)),
            search_level: cfg.search_level.max(1),
            use_ml_ordering: true,
            ..Default::default()
        });
        Self {
            cfg,
            require_exact_disproof: false,
            threads: 1,
            time_limit_secs: None,
            solve_solver,
            check_solver,
            sub_solver,
            bounded: BoundedDfPn::new(),
            scratch_tb: TBase::new(1),
            check_solver_nodes_acc: 0,
            abort_flag: None,
            progress_cb: None,
        }
    }

    pub fn new_strict_formal(cfg: PerfectionConfig) -> Self {
        let mut v = Self::new(cfg);
        v.require_exact_disproof = true;
        v
    }

    pub fn set_require_exact_disproof(&mut self, strict: bool) {
        self.require_exact_disproof = strict;
    }

    pub fn set_threads(&mut self, threads: usize) {
        self.threads = threads.max(1);
    }

    pub fn set_fail_fast(&mut self, fail_fast: bool) {
        self.cfg.fail_fast = fail_fast;
    }

    pub fn set_time_limit_secs(&mut self, secs: Option<f64>) {
        self.time_limit_secs = secs;
    }

    pub fn set_abort_flag(&mut self, flag: Option<Arc<AtomicBool>>) {
        self.solve_solver.set_abort_flag(flag.clone());
        self.check_solver.set_abort_flag(flag.clone());
        self.sub_solver.set_abort_flag(flag.clone());
        self.bounded.set_abort_flag(flag.clone());
        self.abort_flag = flag;
    }

    pub fn set_progress_callback(&mut self, cb: Option<Arc<dyn Fn(u64) + Send + Sync>>) {
        self.solve_solver.set_progress_callback(cb.clone());
        self.progress_cb = cb;
    }

    #[inline]
    fn is_cancelled(&self, start: Instant) -> bool {
        if self
            .abort_flag
            .as_ref()
            .is_some_and(|f| f.load(Ordering::Relaxed))
        {
            return true;
        }
        if let Some(lim) = self.time_limit_secs {
            if start.elapsed().as_secs_f64() >= lim {
                return true;
            }
        }
        false
    }

    #[inline]
    fn report_progress(&self, base_nodes: u64) {
        if let Some(cb) = &self.progress_cb {
            cb(base_nodes + self.check_solver_nodes_acc);
        }
    }

    /// Solve `root` and verify conventional perfection along the principal variation.
    pub fn evaluate(&mut self, root: &FmrsPos) -> PerfectionReport {
        let res = self.solve_solver.solve_unbounded_pn(root);
        self.verify_solved_inner(root, res, true)
    }

    /// Verify perfection given an already computed `SearchResult`.
    pub fn verify_solved(&mut self, root: &FmrsPos, res: SearchResult) -> PerfectionReport {
        self.verify_solved_inner(root, res, false)
    }

    fn verify_solved_inner(
        &mut self,
        root: &FmrsPos,
        mut res: SearchResult,
        mut warm_tbase: bool,
    ) -> PerfectionReport {
        let verify_start = Instant::now();
        self.check_solver_nodes_acc = 0;
        let mut solve_nodes = res.nodes;
        match res.mate {
            Some(false) => {
                return PerfectionReport {
                    status: PerfectionStatus::NoMate,
                    mate_steps: 0,
                    pv: Vec::new(),
                    shortcut_pv: Vec::new(),
                    solve_nodes,
                    check_nodes: 0,
                    shortcut_or_count: 0,
                    minor_flaw_count: 0,
                };
            }
            None => {
                return PerfectionReport {
                    status: PerfectionStatus::Timeout,
                    mate_steps: 0,
                    pv: Vec::new(),
                    shortcut_pv: Vec::new(),
                    solve_nodes,
                    check_nodes: 0,
                    shortcut_or_count: 0,
                    minor_flaw_count: 0,
                };
            }
            Some(true) => {}
        }

        let root_tn: Turn = root.turn();
        let def_tn: Turn = root_tn.flip();

        recover_short_clean_root_if_needed(
            root,
            &mut res,
            &mut warm_tbase,
            &mut self.sub_solver,
            &mut self.bounded,
            &mut self.scratch_tb,
            &mut self.check_solver_nodes_acc,
        );

        if res.redundant {
            return PerfectionReport {
                status: PerfectionStatus::RedundantHand,
                mate_steps: res.mate_steps,
                pv: res.pv,
                shortcut_pv: Vec::new(),
                solve_nodes,
                check_nodes: self.check_solver_nodes_acc,
                shortcut_or_count: 0,
                minor_flaw_count: 0,
            };
        }

        if res.pv.is_empty() || res.pv.len() % 2 == 0 {
            return PerfectionReport {
                status: PerfectionStatus::Timeout,
                mate_steps: res.mate_steps,
                pv: res.pv,
                shortcut_pv: Vec::new(),
                solve_nodes,
                check_nodes: self.check_solver_nodes_acc,
                shortcut_or_count: 0,
                minor_flaw_count: 0,
            };
        }

        let init_per_check_limit = if self.require_exact_disproof {
            self.cfg.check_node_limit_per_move.max(solve_nodes)
        } else {
            self.cfg
                .check_node_limit_per_move
                .max(solve_nodes.min(2_000_000))
        };
        let warm_tb_opt = if warm_tbase {
            Some(self.solve_solver.tbase_mut())
        } else {
            None
        };
        let deadline = self.time_limit_secs.map(|lim| (verify_start, lim));
        res = match stabilize_main_line_pv(
            root,
            res,
            warm_tb_opt,
            init_per_check_limit,
            &mut self.sub_solver,
            &mut self.scratch_tb,
            &mut solve_nodes,
            self.abort_flag.as_deref(),
            deadline,
        ) {
            Ok(r) => r,
            Err(st) => {
                return PerfectionReport {
                    status: st,
                    mate_steps: 0,
                    pv: Vec::new(),
                    shortcut_pv: Vec::new(),
                    solve_nodes,
                    check_nodes: self.check_solver_nodes_acc,
                    shortcut_or_count: 0,
                    minor_flaw_count: 0,
                };
            }
        };
        self.report_progress(solve_nodes);

        if res.redundant {
            return PerfectionReport {
                status: PerfectionStatus::RedundantHand,
                mate_steps: res.mate_steps,
                pv: res.pv,
                shortcut_pv: Vec::new(),
                solve_nodes,
                check_nodes: self.check_solver_nodes_acc,
                shortcut_or_count: 0,
                minor_flaw_count: 0,
            };
        }

        let per_check_limit = if self.require_exact_disproof {
            self.cfg.check_node_limit_per_move.max(solve_nodes)
        } else {
            self.cfg
                .check_node_limit_per_move
                .max(solve_nodes.min(2_000_000))
        };

        '_re_stab: for re_stab_iter in 0..4usize {
            let (step_delta, rem_eff) = compute_pv_effective_rem(root, &res.pv);
            res.mate_steps = rem_eff[0];
            let total_sh = res.mate_steps as usize;

            let mut pv_ors: Vec<(u16, FmrsPos, Move, u16)> = Vec::with_capacity(total_sh / 2 + 1);
            let mut pv_or_states: HashSet<(u64, MKey, MKey, u16)> =
                HashSet::with_capacity(total_sh / 2 + 1);
            let mut pv_or_dom: HashMap<u64, Vec<(MKey, MKey, u16)>> =
                HashMap::with_capacity(total_sh / 2 + 1);
            {
                let mut cur = root.clone();
                for (ply, &mv) in res.pv.iter().enumerate() {
                    if ply % 2 == 0 && step_delta[ply] == 1 {
                        let rem_sh = rem_eff[ply];
                        let am = cur.mkey(root_tn);
                        let dm = cur.mkey(def_tn);
                        pv_or_states.insert((cur.zkey(), am, dm, rem_sh));
                        pv_or_dom
                            .entry(cur.zkey())
                            .or_default()
                            .push((am, dm, ply as u16));
                        pv_ors.push((ply as u16, cur.clone(), mv, rem_sh));
                    }
                    cur = cur.move_forward(mv);
                }
                self.scratch_tb.clear();
                if !cur.generate_evasion(&mut self.scratch_tb).0.is_empty() {
                    return PerfectionReport {
                        status: PerfectionStatus::Timeout,
                        mate_steps: res.mate_steps,
                        pv: res.pv,
                        shortcut_pv: Vec::new(),
                        solve_nodes,
                        check_nodes: self.check_solver_nodes_acc,
                        shortcut_or_count: 0,
                        minor_flaw_count: 0,
                    };
                }
            }

            #[cfg(not(target_arch = "wasm32"))]
            if self.threads > 1 && pv_ors.len() >= 4 {
                let mut minor_flaw_init = 0u16;
                let mut tasks: Vec<(usize, u16, Move, Move, u16)> = Vec::new();
                for (or_idx, &(ply, ref or_pos, intended_mv, rem_sh)) in pv_ors.iter().enumerate()
                {
                    self.scratch_tb.clear();
                    let checks = or_pos.generate_check(&mut self.scratch_tb);
                    if rem_sh == 1 {
                        for item in &checks {
                            for mv in item.all_moves() {
                                if mv == intended_mv {
                                    continue;
                                }
                                let child = or_pos.move_forward(mv);
                                self.scratch_tb.clear();
                                if child.generate_evasion(&mut self.scratch_tb).0.is_empty()
                                    && child.mkey(root_tn).is_empty()
                                {
                                    minor_flaw_init = minor_flaw_init.saturating_add(1);
                                }
                            }
                        }
                        continue;
                    }
                    for item in &checks {
                        for mv in item.all_moves() {
                            if mv == intended_mv {
                                continue;
                            }
                            let child_and = or_pos.move_forward(mv);
                            if is_refuted_by_earlier_pv(
                                &child_and,
                                mv.dest(),
                                root_tn,
                                def_tn,
                                ply,
                                &pv_or_dom,
                                &mut self.scratch_tb,
                            ) {
                                continue;
                            }
                            tasks.push((or_idx, ply, mv, intended_mv, rem_sh));
                        }
                    }
                }

                if tasks.len() > 2 {
                    let n_workers = self.threads.min(tasks.len());
                    let worker_hash_mb = (self.cfg.hash_mb / 2).clamp(16, 64);
                    let next_idx = AtomicUsize::new(0);
                    let found_ply = AtomicUsize::new(usize::MAX);
                    let timed_out = AtomicBool::new(false);
                    let total_check_nodes = AtomicU64::new(0);
                    let minor_flaw_total = AtomicU64::new(minor_flaw_init as u64);
                    let best_flaw: Mutex<Option<(u16, Move, Move, FlawKind, u16, Vec<EvSolved>)>> =
                        Mutex::new(None);
                    let shortcut_plies: Mutex<HashSet<u16>> = Mutex::new(HashSet::new());

                    let cfg_ref = &self.cfg;
                    let pv_ors_ref = &pv_ors;
                    let pv_or_states_ref = &pv_or_states;
                    let tasks_ref = &tasks;
                    let req_exact = self.require_exact_disproof;
                    let time_limit = self.time_limit_secs;
                    let ext_abort = self.abort_flag.as_ref();
                    let prog_cb = self.progress_cb.as_ref();
                    let base_check_acc = self.check_solver_nodes_acc;

                    std::thread::scope(|scope| {
                        for _ in 0..n_workers {
                            scope.spawn(|| {
                                let mut check_solver = Solver::new(SearchConfig {
                                    hash_mb: worker_hash_mb,
                                    threads: 1,
                                    node_limit: Some(2_048),
                                    search_level: 0,
                                    use_ml_ordering: true,
                                    ..Default::default()
                                });
                                let mut sub_solver = Solver::new(SearchConfig {
                                    hash_mb: worker_hash_mb,
                                    threads: 1,
                                    node_limit: Some(per_check_limit.max(2_048)),
                                    search_level: cfg_ref.search_level.max(1),
                                    use_ml_ordering: true,
                                    ..Default::default()
                                });
                                let mut bounded = BoundedDfPn::new();
                                let mut scratch_tb = TBase::new(1);
                                if let Some(flag) = ext_abort {
                                    check_solver.set_abort_flag(Some(Arc::clone(flag)));
                                    sub_solver.set_abort_flag(Some(Arc::clone(flag)));
                                    bounded.set_abort_flag(Some(Arc::clone(flag)));
                                }

                                loop {
                                    if timed_out.load(Ordering::Acquire) {
                                        break;
                                    }
                                    if ext_abort.is_some_and(|f| f.load(Ordering::Relaxed))
                                        || time_limit.is_some_and(|lim| {
                                            verify_start.elapsed().as_secs_f64() >= lim
                                        })
                                    {
                                        timed_out.store(true, Ordering::Release);
                                        break;
                                    }
                                    let idx = next_idx.fetch_add(1, Ordering::Relaxed);
                                    if idx >= tasks_ref.len() {
                                        break;
                                    }
                                    let (or_idx, ply, mv, intended_mv, rem_sh) = tasks_ref[idx];
                                    if cfg_ref.fail_fast
                                        && (ply as usize) >= found_ply.load(Ordering::Acquire)
                                    {
                                        break;
                                    }

                                    let or_pos = &pv_ors_ref[or_idx].1;
                                    let child_and = or_pos.move_forward(mv);

                                    let mut probe_item = single_check_item(or_pos, mv);
                                    check_solver.probe_and_after_check(
                                        &child_and,
                                        TSUME_MAX_DEPTH.saturating_sub(child_and.count() + 1),
                                        &mut probe_item,
                                    );
                                    let pnodes = check_solver.nodes();
                                    let cur_tot = total_check_nodes
                                        .fetch_add(pnodes, Ordering::Relaxed)
                                        + pnodes;
                                    if let Some(cb) = prog_cb {
                                        cb(solve_nodes + base_check_acc + cur_tot);
                                    }
                                    if probe_item.tdata.dn == 0 {
                                        continue;
                                    }

                                    let rem_time = time_limit.map(|lim| {
                                        (lim - verify_start.elapsed().as_secs_f64()).max(0.001)
                                    });
                                    sub_solver.set_time_limit_secs(rem_time);

                                    let mut local_acc = 0u64;
                                    if let Some(cb) = prog_cb {
                                        let base = solve_nodes
                                            + base_check_acc
                                            + total_check_nodes.load(Ordering::Relaxed);
                                        let cb_clone = Arc::clone(cb);
                                        sub_solver.set_progress_callback(Some(Arc::new(
                                            move |n: u64| {
                                                cb_clone(base + n);
                                            },
                                        )));
                                    }
                                    let resolved = resolve_alt_check_sound(
                                        &child_and,
                                        mv.dest(),
                                        rem_sh,
                                        root_tn,
                                        None,
                                        pv_or_states_ref,
                                        per_check_limit,
                                        req_exact,
                                        &mut sub_solver,
                                        &mut bounded,
                                        &mut scratch_tb,
                                        &mut local_acc,
                                    );
                                    if prog_cb.is_some() {
                                        sub_solver.set_progress_callback(None);
                                    }
                                    let cur_tot = total_check_nodes
                                        .fetch_add(local_acc, Ordering::Relaxed)
                                        + local_acc;
                                    if let Some(cb) = prog_cb {
                                        cb(solve_nodes + base_check_acc + cur_tot);
                                    }

                                    if ext_abort.is_some_and(|f| f.load(Ordering::Relaxed))
                                        || time_limit.is_some_and(|lim| {
                                            verify_start.elapsed().as_secs_f64() >= lim
                                        })
                                    {
                                        timed_out.store(true, Ordering::Release);
                                        break;
                                    }

                                    match resolved {
                                        Ok(AltResolved::Refuted { .. }) => continue,
                                        Err(()) => {
                                            timed_out.store(true, Ordering::Release);
                                            break;
                                        }
                                        Ok(AltResolved::Mated {
                                            std_steps,
                                            non_redundant,
                                            any_steps,
                                            solved_evs,
                                        }) => {
                                            if any_steps < rem_sh || std_steps < rem_sh {
                                                {
                                                    let mut g = best_flaw.lock().unwrap();
                                                    if g.as_ref().is_none_or(|cur| ply < cur.0) {
                                                        *g = Some((
                                                            ply,
                                                            mv,
                                                            intended_mv,
                                                            FlawKind::EarlyShortcut,
                                                            rem_sh,
                                                            solved_evs,
                                                        ));
                                                    }
                                                }
                                                shortcut_plies.lock().unwrap().insert(ply);
                                                if cfg_ref.fail_fast {
                                                    found_ply
                                                        .fetch_min(ply as usize, Ordering::AcqRel);
                                                }
                                                continue;
                                            }

                                            if std_steps == rem_sh && non_redundant {
                                                let is_same_src_dst_prom = mv.prev_pos
                                                    == intended_mv.prev_pos
                                                    && mv.dest() == intended_mv.dest();
                                                if is_same_src_dst_prom {
                                                    minor_flaw_total
                                                        .fetch_add(1, Ordering::Relaxed);
                                                    continue;
                                                }
                                                if (rem_sh <= cfg_ref.endgame_transposition_rem_sh
                                                    || is_same_ray_sliding_drop(
                                                        or_pos,
                                                        mv,
                                                        intended_mv,
                                                    ))
                                                    && rem_sh >= 3
                                                    && all_optimal_evasions_converge_to_pv(
                                                        &child_and,
                                                        &solved_evs,
                                                        rem_sh,
                                                        root_tn,
                                                        pv_or_states_ref,
                                                    )
                                                {
                                                    minor_flaw_total
                                                        .fetch_add(1, Ordering::Relaxed);
                                                    continue;
                                                }
                                                {
                                                    let mut g = best_flaw.lock().unwrap();
                                                    if g.as_ref().is_none_or(|cur| ply < cur.0) {
                                                        *g = Some((
                                                            ply,
                                                            mv,
                                                            intended_mv,
                                                            FlawKind::EqualLengthShortcut,
                                                            rem_sh,
                                                            solved_evs,
                                                        ));
                                                    }
                                                }
                                                shortcut_plies.lock().unwrap().insert(ply);
                                                if cfg_ref.fail_fast {
                                                    found_ply
                                                        .fetch_min(ply as usize, Ordering::AcqRel);
                                                }
                                            }
                                        }
                                    }
                                }
                            });
                        }
                    });

                    self.check_solver_nodes_acc += total_check_nodes.load(Ordering::Relaxed);
                    let minor_flaw_count =
                        minor_flaw_total.load(Ordering::Relaxed).min(u16::MAX as u64) as u16;
                    let flaw_opt = best_flaw.into_inner().unwrap();
                    let shortcut_or_count = shortcut_plies.into_inner().unwrap().len() as u16;

                    if let Some((first_ply, alt_move, intended_move, kind, f_rem_sh, f_evs)) =
                        flaw_opt
                    {
                        let shortcut_pv = build_shortcut_pv(
                            root,
                            &res.pv,
                            first_ply,
                            alt_move,
                            kind,
                            f_rem_sh,
                            &f_evs,
                            &mut self.scratch_tb,
                        );
                        if kind == FlawKind::EarlyShortcut
                            && first_ply > 0
                            && res.mate_steps <= 15
                            && re_stab_iter < 3
                            && !shortcut_pv.is_empty()
                            && shortcut_pv.len() % 2 == 1
                        {
                            let sc_clean =
                                unused_atk_pieces_effective(root, &shortcut_pv, root_tn) == 0;
                            if sc_clean {
                                let mut cand = res.clone();
                                cand.pv = shortcut_pv.clone();
                                cand.mate_steps = effective_tsume_pv_plies(root, &shortcut_pv);
                                cand.redundant = false;
                                if let Ok(new_res) = stabilize_main_line_pv(
                                    root,
                                    cand,
                                    None,
                                    init_per_check_limit,
                                    &mut self.sub_solver,
                                    &mut self.scratch_tb,
                                    &mut solve_nodes,
                                    self.abort_flag.as_deref(),
                                    deadline,
                                ) {
                                    let fp_cmp = (first_ply as usize).min(new_res.pv.len());
                                    let def_switched = new_res.pv[..fp_cmp] != res.pv[..fp_cmp];
                                    if !new_res.redundant
                                        && (def_switched || new_res.mate_steps < res.mate_steps)
                                    {
                                        res = new_res;
                                        continue '_re_stab;
                                    }
                                }
                            }
                        }
                        return PerfectionReport {
                            status: PerfectionStatus::Shortcut {
                                first_ply,
                                alt_move,
                                intended_move,
                                kind,
                            },
                            mate_steps: res.mate_steps,
                            pv: res.pv,
                            shortcut_pv,
                            solve_nodes,
                            check_nodes: self.check_solver_nodes_acc,
                            shortcut_or_count: shortcut_or_count.max(1),
                            minor_flaw_count,
                        };
                    }
                    if timed_out.load(Ordering::Acquire) || self.is_cancelled(verify_start) {
                        return PerfectionReport {
                            status: PerfectionStatus::Timeout,
                            mate_steps: res.mate_steps,
                            pv: res.pv,
                            shortcut_pv: Vec::new(),
                            solve_nodes,
                            check_nodes: self.check_solver_nodes_acc,
                            shortcut_or_count: 0,
                            minor_flaw_count,
                        };
                    }
                    return PerfectionReport {
                        status: PerfectionStatus::Perfect,
                        mate_steps: res.mate_steps,
                        pv: res.pv,
                        shortcut_pv: Vec::new(),
                        solve_nodes,
                        check_nodes: self.check_solver_nodes_acc,
                        shortcut_or_count: 0,
                        minor_flaw_count,
                    };
                }
            }

            // Reuse a single sound `check_solver` TBase and `BoundedDfPn` dominance table across the PV!
            self.check_solver.tbase_mut().clear();
            self.bounded.clear_for_root();

            let mut shortcut_or_count = 0u16;
            let mut minor_flaw_count = 0u16;
            let mut first_flaw: Option<(u16, Move, Move, FlawKind, u16, Vec<EvSolved>)> = None;

            for &(ply, ref or_pos, intended_mv, rem_sh) in &pv_ors {
                if self.is_cancelled(verify_start) {
                    return PerfectionReport {
                        status: PerfectionStatus::Timeout,
                        mate_steps: res.mate_steps,
                        pv: res.pv,
                        shortcut_pv: Vec::new(),
                        solve_nodes,
                        check_nodes: self.check_solver_nodes_acc,
                        shortcut_or_count,
                        minor_flaw_count,
                    };
                }
                self.scratch_tb.clear();
                let checks = or_pos.generate_check(&mut self.scratch_tb);

                if rem_sh == 1 {
                    for item in &checks {
                        for mv in item.all_moves() {
                            if mv == intended_mv {
                                continue;
                            }
                            let child = or_pos.move_forward(mv);
                            self.scratch_tb.clear();
                            if child.generate_evasion(&mut self.scratch_tb).0.is_empty()
                                && child.mkey(root_tn).is_empty()
                            {
                                minor_flaw_count = minor_flaw_count.saturating_add(1);
                            }
                        }
                    }
                    continue;
                }

                let mut pos_has_shortcut = false;

                'mv_loop: for item in &checks {
                    for mv in item.all_moves() {
                        if mv == intended_mv {
                            continue;
                        }
                        if self.is_cancelled(verify_start) {
                            return PerfectionReport {
                                status: PerfectionStatus::Timeout,
                                mate_steps: res.mate_steps,
                                pv: res.pv,
                                shortcut_pv: Vec::new(),
                                solve_nodes,
                                check_nodes: self.check_solver_nodes_acc,
                                shortcut_or_count,
                                minor_flaw_count,
                            };
                        }
                        let child_and = or_pos.move_forward(mv);

                        // Exact O(1) backward-transposition refutation against earlier/current PV OR-states.
                        if is_refuted_by_earlier_pv(
                            &child_and,
                            mv.dest(),
                            root_tn,
                            def_tn,
                            ply,
                            &pv_or_dom,
                            &mut self.scratch_tb,
                        ) {
                            continue;
                        }

                        // Stage 1: Fast shared-TBase infinite-depth disproof probe (`<= 2,048` nodes).
                        // Soundly skip ONLY when `dn == 0` (never skip on `pn >= PROOF_MAX - 1`!).
                        let mut probe_item = single_check_item(or_pos, mv);
                        self.check_solver.probe_and_after_check(
                            &child_and,
                            TSUME_MAX_DEPTH.saturating_sub(child_and.count() + 1),
                            &mut probe_item,
                        );
                        self.check_solver_nodes_acc += self.check_solver.nodes();
                        self.report_progress(solve_nodes);

                        if probe_item.tdata.dn == 0 {
                            continue;
                        }

                        // Forward cumulative progress callback and remaining time limit into `sub_solver`.
                        let rem_time = self
                            .time_limit_secs
                            .map(|lim| (lim - verify_start.elapsed().as_secs_f64()).max(0.001));
                        self.sub_solver.set_time_limit_secs(rem_time);
                        if let Some(cb) = &self.progress_cb {
                            let base = solve_nodes + self.check_solver_nodes_acc;
                            let cb_clone = Arc::clone(cb);
                            self.sub_solver.set_progress_callback(Some(Arc::new(move |n: u64| {
                                cb_clone(base + n);
                            })));
                        }

                        // Stage 2: Exact uncapped resolution (`Solver::solve_unbounded_pn` + `BoundedDfPn`).
                        let resolved = resolve_alt_check_sound(
                            &child_and,
                            mv.dest(),
                            rem_sh,
                            root_tn,
                            None,
                            &pv_or_states,
                            per_check_limit,
                            self.require_exact_disproof,
                            &mut self.sub_solver,
                            &mut self.bounded,
                            &mut self.scratch_tb,
                            &mut self.check_solver_nodes_acc,
                        );
                        self.sub_solver.set_time_limit_secs(None);
                        if self.progress_cb.is_some() {
                            self.sub_solver.set_progress_callback(None);
                            self.report_progress(solve_nodes);
                        }
                        if self.is_cancelled(verify_start) {
                            return PerfectionReport {
                                status: PerfectionStatus::Timeout,
                                mate_steps: res.mate_steps,
                                pv: res.pv,
                                shortcut_pv: Vec::new(),
                                solve_nodes,
                                check_nodes: self.check_solver_nodes_acc,
                                shortcut_or_count,
                                minor_flaw_count,
                            };
                        }
                        match resolved {
                            Ok(AltResolved::Refuted { .. }) => continue,
                            Err(()) => {
                                return PerfectionReport {
                                    status: PerfectionStatus::Timeout,
                                    mate_steps: res.mate_steps,
                                    pv: res.pv,
                                    shortcut_pv: Vec::new(),
                                    solve_nodes,
                                    check_nodes: self.check_solver_nodes_acc,
                                    shortcut_or_count,
                                    minor_flaw_count,
                                };
                            }
                            Ok(AltResolved::Mated {
                                std_steps,
                                non_redundant,
                                any_steps,
                                solved_evs,
                            }) => {
                                // 1. Strictly faster early mate (`< rem_sh`), even with unused pieces!
                                if any_steps < rem_sh || std_steps < rem_sh {
                                    pos_has_shortcut = true;
                                    if self.cfg.fail_fast {
                                        let shortcut_pv = build_shortcut_pv(
                                            root,
                                            &res.pv,
                                            ply,
                                            mv,
                                            FlawKind::EarlyShortcut,
                                            rem_sh,
                                            &solved_evs,
                                            &mut self.scratch_tb,
                                        );
                                        if ply > 0
                                            && res.mate_steps <= 15
                                            && re_stab_iter < 3
                                            && !shortcut_pv.is_empty()
                                            && shortcut_pv.len() % 2 == 1
                                        {
                                            let sc_clean = unused_atk_pieces_effective(
                                                root,
                                                &shortcut_pv,
                                                root_tn,
                                            ) == 0;
                                            if sc_clean {
                                                let mut cand = res.clone();
                                                cand.pv = shortcut_pv.clone();
                                                cand.mate_steps =
                                                    effective_tsume_pv_plies(root, &shortcut_pv);
                                                cand.redundant = false;
                                                if let Ok(new_res) = stabilize_main_line_pv(
                                                    root,
                                                    cand,
                                                    None,
                                                    init_per_check_limit,
                                                    &mut self.sub_solver,
                                                    &mut self.scratch_tb,
                                                    &mut solve_nodes,
                                                    self.abort_flag.as_deref(),
                                                    deadline,
                                                ) {
                                                    let fp_cmp =
                                                        (ply as usize).min(new_res.pv.len());
                                                    let def_switched =
                                                        new_res.pv[..fp_cmp] != res.pv[..fp_cmp];
                                                    if !new_res.redundant
                                                        && (def_switched
                                                            || new_res.mate_steps < res.mate_steps)
                                                    {
                                                        res = new_res;
                                                        continue '_re_stab;
                                                    }
                                                }
                                            }
                                        }
                                        return PerfectionReport {
                                            status: PerfectionStatus::Shortcut {
                                                first_ply: ply,
                                                alt_move: mv,
                                                intended_move: intended_mv,
                                                kind: FlawKind::EarlyShortcut,
                                            },
                                            mate_steps: res.mate_steps,
                                            pv: res.pv,
                                            shortcut_pv,
                                            solve_nodes,
                                            check_nodes: self.check_solver_nodes_acc,
                                            shortcut_or_count: 1,
                                            minor_flaw_count,
                                        };
                                    }
                                    if first_flaw.is_none() {
                                        first_flaw = Some((
                                            ply,
                                            mv,
                                            intended_mv,
                                            FlawKind::EarlyShortcut,
                                            rem_sh,
                                            solved_evs,
                                        ));
                                    }
                                    break 'mv_loop;
                                }

                                // 2. Equal-length non-redundant mate (`== rem_sh`)
                                if std_steps == rem_sh && non_redundant {
                                    let is_same_src_dst_prom = mv.prev_pos == intended_mv.prev_pos
                                        && mv.dest() == intended_mv.dest();
                                    if is_same_src_dst_prom {
                                        minor_flaw_count = minor_flaw_count.saturating_add(1);
                                        continue;
                                    }

                                    if (rem_sh <= self.cfg.endgame_transposition_rem_sh
                                        || is_same_ray_sliding_drop(or_pos, mv, intended_mv))
                                        && rem_sh >= 3
                                        && all_optimal_evasions_converge_to_pv(
                                            &child_and,
                                            &solved_evs,
                                            rem_sh,
                                            root_tn,
                                            &pv_or_states,
                                        )
                                    {
                                        minor_flaw_count = minor_flaw_count.saturating_add(1);
                                        continue;
                                    }

                                    pos_has_shortcut = true;
                                    if self.cfg.fail_fast {
                                        let shortcut_pv = build_shortcut_pv(
                                            root,
                                            &res.pv,
                                            ply,
                                            mv,
                                            FlawKind::EqualLengthShortcut,
                                            rem_sh,
                                            &solved_evs,
                                            &mut self.scratch_tb,
                                        );
                                        return PerfectionReport {
                                            status: PerfectionStatus::Shortcut {
                                                first_ply: ply,
                                                alt_move: mv,
                                                intended_move: intended_mv,
                                                kind: FlawKind::EqualLengthShortcut,
                                            },
                                            mate_steps: res.mate_steps,
                                            pv: res.pv,
                                            shortcut_pv,
                                            solve_nodes,
                                            check_nodes: self.check_solver_nodes_acc,
                                            shortcut_or_count: 1,
                                            minor_flaw_count,
                                        };
                                    }
                                    if first_flaw.is_none() {
                                        first_flaw = Some((
                                            ply,
                                            mv,
                                            intended_mv,
                                            FlawKind::EqualLengthShortcut,
                                            rem_sh,
                                            solved_evs,
                                        ));
                                    }
                                    break 'mv_loop;
                                }
                            }
                        }
                    }
                }

                if pos_has_shortcut {
                    shortcut_or_count = shortcut_or_count.saturating_add(1);
                }
            }

            if let Some((first_ply, alt_move, intended_move, kind, f_rem_sh, f_evs)) = first_flaw {
                let shortcut_pv = build_shortcut_pv(
                    root,
                    &res.pv,
                    first_ply,
                    alt_move,
                    kind,
                    f_rem_sh,
                    &f_evs,
                    &mut self.scratch_tb,
                );
                if kind == FlawKind::EarlyShortcut
                    && first_ply > 0
                    && res.mate_steps <= 15
                    && re_stab_iter < 3
                    && !shortcut_pv.is_empty()
                    && shortcut_pv.len() % 2 == 1
                {
                    let sc_clean = unused_atk_pieces_effective(root, &shortcut_pv, root_tn) == 0;
                    if sc_clean {
                        let mut cand = res.clone();
                        cand.pv = shortcut_pv.clone();
                        cand.mate_steps = effective_tsume_pv_plies(root, &shortcut_pv);
                        cand.redundant = false;
                        if let Ok(new_res) = stabilize_main_line_pv(
                            root,
                            cand,
                            None,
                            init_per_check_limit,
                            &mut self.sub_solver,
                            &mut self.scratch_tb,
                            &mut solve_nodes,
                            self.abort_flag.as_deref(),
                            deadline,
                        ) {
                            let fp_cmp = (first_ply as usize).min(new_res.pv.len());
                            let def_switched = new_res.pv[..fp_cmp] != res.pv[..fp_cmp];
                            if !new_res.redundant
                                && (def_switched || new_res.mate_steps < res.mate_steps)
                            {
                                res = new_res;
                                continue '_re_stab;
                            }
                        }
                    }
                }
                return PerfectionReport {
                    status: PerfectionStatus::Shortcut {
                        first_ply,
                        alt_move,
                        intended_move,
                        kind,
                    },
                    mate_steps: res.mate_steps,
                    pv: res.pv,
                    shortcut_pv,
                    solve_nodes,
                    check_nodes: self.check_solver_nodes_acc,
                    shortcut_or_count,
                    minor_flaw_count,
                };
            }

            return PerfectionReport {
                status: PerfectionStatus::Perfect,
                mate_steps: res.mate_steps,
                pv: res.pv,
                shortcut_pv: Vec::new(),
                solve_nodes,
                check_nodes: self.check_solver_nodes_acc,
                shortcut_or_count: 0,
                minor_flaw_count,
            };
        }

        PerfectionReport {
            status: PerfectionStatus::Timeout,
            mate_steps: res.mate_steps,
            pv: res.pv,
            shortcut_pv: Vec::new(),
            solve_nodes,
            check_nodes: self.check_solver_nodes_acc,
            shortcut_or_count: 0,
            minor_flaw_count: 0,
        }
    }
}
