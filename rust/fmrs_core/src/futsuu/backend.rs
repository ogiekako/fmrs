//! Pluggable position / move-generation backend trait for the shtsume solver.

use crate::futsuu::ni_cache::BoardPlanes;
use crate::futsuu::tbase::TBase;
use crate::futsuu::types::{KeyState, MKey, Move, MvItem, Turn};

/// Exact 1-ply checkmate witness returned by [`PositionBackend::find_1ply_mate`].
#[derive(Copy, Clone, Debug)]
pub struct OnePlyMate {
    pub mv: Move,
    pub src_koma: u8,
    pub cap_koma: u8,
    pub final_atk_koma: u8,
    pub is_double_check: bool,
}

/// Oracle interface queried during futile-interposition filtering (`is_futile_drop_square`)
/// to test whether capturing a dropped interposition reaches an already-proven mate in `TBase`.
pub trait HsOracle {
    fn hs_lookup<P: PositionBackend>(&mut self, pos: &P, tn: Turn) -> bool;
}

/// Abstraction over the board representation and check/evasion move generator.
///
/// Both the ported shtsume backend (`ShtsumePos`) and the `fmrs` core backend
/// (`FmrsPos`) implement this trait so the exact same df-pn (`bn_search`) solver
/// and transposition table (`TBase` / `SharedTBase`) can run on either backend.
pub trait PositionBackend: Clone + Send + Sync {
    /// Decode a position from an SFEN string.
    fn from_sfen(sfen: &str) -> anyhow::Result<Self>;

    /// Current side to move.
    fn turn(&self) -> Turn;

    /// Ply count from the root position.
    fn count(&self) -> u16;

    /// Board + turn Zobrist hash (MUST exclude hands so dominance lookup in TBase works).
    fn zkey(&self) -> u64;

    /// Packed hand counts for `side`.
    fn mkey(&self, side: Turn) -> MKey;

    /// Lightweight snapshot of `(zkey, mkey, turn, count)`.
    #[inline]
    fn key_state(&self) -> KeyState {
        KeyState {
            zkey: self.zkey(),
            mkey: [self.mkey(Turn::Sente), self.mkey(Turn::Gote)],
            turn: self.turn(),
            count: self.count(),
        }
    }

    /// Fast key-only transition after applying `mv`, for TT lookup without cloning the full position.
    fn key_forward(&self, mv: Move) -> KeyState;

    /// Fast key-only transition using precomputed `(src_koma, cap_koma)` hints on `MvItem`.
    #[inline(always)]
    fn key_forward_item(&self, item: &MvItem) -> KeyState {
        self.key_forward(item.head_move())
    }

    /// Full move execution returning the child position.
    fn move_forward(&self, mv: Move) -> Self;

    /// Piece on square `sq` (0..81, shtsume indexing: `rank * 9 + file`, 0 = 1一).
    fn board_at(&self, sq: usize) -> u8;

    /// Defender's (opponent's) king square.
    fn enemy_ou(&self) -> usize;

    /// Side-to-move's king square (valid when king is on board; 0..81).
    fn self_ou(&self) -> usize;

    /// Number of pieces currently giving check to the side-to-move's king (0, 1, or 2).
    fn n_oute(&self) -> u8;

    /// Square of the primary checking piece (valid when `n_oute() >= 1`).
    fn attack_sq(&self) -> usize;

    /// Whether the side-to-move has any piece attacking `sq`.
    fn self_effect_contains(&self, sq: usize) -> bool;

    /// Whether the position is horizontally symmetric (used at root to halve search space).
    fn symmetry_check(&self) -> bool;

    /// Whether the side-to-move can give check by dropping a piece of type `ktype` (FU..=HI).
    fn drop_check(&self, ktype: u8) -> bool;

    /// Default AND-node `1 + (dn >> k)` threshold shift when `SearchConfig::use_ml_ordering` is true.
    const DEFAULT_EPS_DN_SHIFT: u8 = 0;

    /// Fast bitboard check for an immediate 1-ply checkmate at an OR node.
    /// Must be 100% sound (every `Some` witness is a legal 1-ply mate respecting `inc` priority);
    /// returning `None` simply falls back to full check generation.
    #[inline(always)]
    fn find_1ply_mate(&self) -> Option<OnePlyMate> {
        None
    }

    /// Fast bitboard check for an immediate 1-ply checkmate at an OR node without `inc` (駒余り) constraints.
    #[inline(always)]
    fn find_1ply_mate_simple(&self) -> Option<OnePlyMate> {
        self.find_1ply_mate()
    }

    /// Default backend-specific AND-node refutation scoring (no-op on `ShtsumePos`;
    /// runs the L1-resident `4.7 KB` `AndTacV2` lookup table on `FmrsPos`).
    #[inline(always)]
    fn default_score_and_moves(&self, _list: &mut [MvItem]) {}

    /// Append all legal checking moves (OR node) into `out` without needing `TBase`.
    fn generate_check_fast(&self, out: &mut Vec<MvItem>);

    /// Append all legal checking moves (OR node) into `out` without allocating a new `Vec`.
    #[inline]
    fn generate_check_into(&self, _tbase: &mut TBase, out: &mut Vec<MvItem>) {
        self.generate_check_fast(out);
    }

    /// Generate all legal checking moves (OR node).
    #[inline]
    fn generate_check(&self, _tbase: &mut TBase) -> Vec<MvItem> {
        let mut out = Vec::with_capacity(32);
        self.generate_check_fast(&mut out);
        out
    }

    /// Append all legal evasion moves (AND node) into `out` using an [`HsOracle`] (`TBase` or `WorkerTBase`).
    fn generate_evasion_with<O: HsOracle>(&self, oracle: &mut O, out: &mut Vec<MvItem>) -> (bool, bool);

    /// Append all legal evasion moves (AND node) into `out`, returning `(invalid_drops, invalid_moves)`.
    #[inline]
    fn generate_evasion_into(&self, tbase: &mut TBase, out: &mut Vec<MvItem>) -> (bool, bool) {
        self.generate_evasion_with(tbase, out)
    }

    /// Append all legal evasion moves (AND node) into `out` WITHOUT futile-drop (無駄合) filtering.
    fn generate_evasion_simple_fast(&self, out: &mut Vec<MvItem>);

    /// Append all legal evasion moves (AND node) into `out` WITHOUT futile-drop (無駄合) filtering.
    #[inline]
    fn generate_evasion_simple_into(&self, _tbase: &mut TBase, out: &mut Vec<MvItem>) {
        self.generate_evasion_simple_fast(out);
    }

    /// Generate all legal evasion moves (AND node) without futile-drop filtering.
    #[inline]
    fn generate_evasion_simple(&self, _tbase: &mut TBase) -> Vec<MvItem> {
        let mut out = Vec::with_capacity(16);
        self.generate_evasion_simple_fast(&mut out);
        out
    }

    /// Generate all legal evasion moves (AND node).
    /// Returns `(moves, invalid_drops, invalid_moves)`.
    #[inline]
    fn generate_evasion(&self, tbase: &mut TBase) -> (Vec<MvItem>, bool, bool) {
        let mut out = Vec::with_capacity(16);
        let (invalid_drops, invalid_moves) = self.generate_evasion_into(tbase, &mut out);
        (out, invalid_drops, invalid_moves)
    }

    // ---- Exact 81-bit Non-Interference (`NiCache`) proof footprint hooks ----

    const SUPPORTS_NI: bool = false;

    #[inline(always)]
    fn ni_ring_hash(&self) -> u64 {
        0
    }

    #[inline(always)]
    fn ni_planes(&self) -> BoardPlanes {
        BoardPlanes::default()
    }

    #[inline(always)]
    fn ni_planes_and_ring_hash(&self) -> (BoardPlanes, u64) {
        (self.ni_planes(), self.ni_ring_hash())
    }

    #[inline(always)]
    fn ni_proof_leaf(&self) -> Option<(u128, MKey)> {
        None
    }

    #[inline(always)]
    fn ni_proof_and_local(&self, _evasions: &[MvItem]) -> Option<(u128, MKey)> {
        None
    }

    #[inline(always)]
    fn ni_proof_or_move_mask(&self, _mv: Move, _child_sh: u16) -> Option<u128> {
        None
    }
}
