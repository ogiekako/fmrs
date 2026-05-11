use anyhow::bail;

use crate::direction::Direction;
use crate::piece::*;

#[derive(Clone, PartialEq, Eq, Ord, PartialOrd, Default)]
pub struct Position {
    black_bb: BitBoard,     // 16 bytes
    kind_bb: KindBitBoard,  // 64 bytes (no per-square cache; lives on PositionAux)
    hands: Hands,           // 8 bytes
    pub(super) digest: u64, // 8 bytes
}

use crate::position::rule::is_movable;
use crate::position::zobrist::zobrist_stone;
use crate::sfen;
use std::fmt;
use std::fmt::Debug;

impl fmt::Debug for Position {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", PositionAux::new(self.clone(), None).sfen_url())
    }
}

use super::advance::attack_prevent::attacker;
use super::bitboard::kind_bitboard::{
    apply_movement_to_cache, build_kind_cache, encode_kind, kind_from_cache, must_kind_from_cache,
    write_kind_idx, KindCache, EMPTY_KIND_CACHE,
};
use super::bitboard::reachable_sub;
use super::bitboard::BitBoard;
use super::bitboard::KindBitBoard;
use super::hands::Hands;
use super::zobrist::zobrist;
use super::Movement;
use super::PositionExt as _;
use super::Square;
use super::UndoMove;

impl Position {
    pub fn turn(&self) -> Color {
        self.hands.turn()
    }
    pub fn set_turn(&mut self, c: Color) {
        self.hands.set_turn(c);
    }
    /// Flip the turn bit; cheaper than `set_turn(c.opposite())` when toggling.
    #[inline(always)]
    pub fn toggle_turn(&mut self) {
        self.hands.toggle_turn();
    }
    pub fn hands(&self) -> Hands {
        self.hands
    }
    pub fn hands_mut(&mut self) -> &mut Hands {
        &mut self.hands
    }
    pub fn pawn_drop(&self) -> bool {
        self.hands.pawn_drop()
    }
    pub fn set_pawn_drop(&mut self, x: bool) {
        self.hands.set_pawn_drop(x)
    }

    pub fn black(&self) -> BitBoard {
        self.black_bb
    }

    /// Returns a bitboard of pieces of the specified color and kind.
    #[inline(always)]
    pub fn bitboard(&self, color: Color, kind: Kind) -> BitBoard {
        if color.is_black() {
            self.kind_bb.bitboard(kind) & self.black_bb
        } else {
            self.kind_bb.bitboard(kind).and_not(self.black_bb)
        }
    }
    pub fn kind_bb(&self) -> &KindBitBoard {
        &self.kind_bb
    }
    #[inline(always)]
    pub fn get(&self, pos: Square) -> Option<(Color, Kind)> {
        let kind = self.kind_bb.get(pos)?;
        Some(if self.black().contains(pos) {
            (Color::BLACK, kind)
        } else {
            (Color::WHITE, kind)
        })
    }
    #[inline(always)]
    pub fn set(&mut self, pos: Square, c: Color, k: Kind) {
        debug_assert_eq!(self.get(pos), None);

        if c.is_black() {
            self.black_bb.set(pos);
        }
        self.kind_bb.set(pos, k);

        // c, k は引数で既知なので、`hash_at` 経由 (kind_bb.must_get
        // + black_bb.contains の二重 lookup) ではなく zobrist 直呼び。
        self.digest ^= zobrist(c, pos, k);
    }
    #[inline(always)]
    pub fn unset(&mut self, pos: Square, c: Color, k: Kind) {
        debug_assert_eq!(self.get(pos), Some((c, k)));

        self.digest ^= zobrist(c, pos, k);

        if c.is_black() {
            self.black_bb.unset(pos);
        }
        self.kind_bb.unset(pos, k);
    }

    /// Same as `unset(pos, c, old)` followed by `set(pos, c, new)` but skips the
    /// black_bb cancel-pair (color unchanged) and merges the digest XOR into one.
    /// Caller must ensure `old != Kind::King` and `new != Kind::King`.
    #[inline(always)]
    pub fn change_kind(&mut self, pos: Square, c: Color, old: Kind, new: Kind) {
        debug_assert_eq!(self.get(pos), Some((c, old)));
        debug_assert_ne!(old, Kind::King);
        debug_assert_ne!(new, Kind::King);
        self.digest ^= zobrist(c, pos, old) ^ zobrist(c, pos, new);
        self.kind_bb.change_kind(pos, old, new);
    }

    /// Move a piece (same color, same kind) from `src` to `dst`. Equivalent to
    /// `unset(src, c, k); set(dst, c, k)` but uses XOR-toggle on layer bitboards
    /// (one op per layer instead of two) and merges the two digest XORs.
    #[inline(always)]
    pub fn move_piece(&mut self, src: Square, dst: Square, c: Color, k: Kind) {
        debug_assert_eq!(self.get(src), Some((c, k)));
        debug_assert_eq!(self.get(dst), None);
        if c.is_black() {
            let mask = (1u128 << src.index()) | (1u128 << dst.index());
            self.black_bb.toggle_mask(mask);
        }
        self.kind_bb.move_piece(src, dst, k);
        self.digest ^= zobrist(c, src, k) ^ zobrist(c, dst, k);
    }

    pub fn shift(&mut self, dir: Direction) {
        self.black_bb.shift(dir);
        self.kind_bb.shift(dir);
        self.digest = 0;
        for pos in self.kind_bb.occupied() {
            self.digest ^= self.hash_at(pos);
        }
    }

    pub(super) fn hash_at(&self, pos: Square) -> u64 {
        let color = if self.black_bb.contains(pos) {
            Color::BLACK
        } else {
            Color::WHITE
        };
        let kind = self.kind_bb.must_get(pos);
        zobrist(color, pos, kind)
    }

    // #[inline(never)]
    pub fn digest(&self) -> u64 {
        self.digest ^ self.hands.x
    }

    /// Serialize to 88 bytes (little-endian): black_bb, promote, kind0..2, hands.x.
    /// `digest` is derived on deserialization.
    pub fn to_bytes(&self) -> [u8; 88] {
        let mut buf = [0u8; 88];
        buf[0..16].copy_from_slice(&self.black_bb.u128().to_le_bytes());
        let (promote, kind0, kind1, kind2) = self.kind_bb.raw_parts();
        buf[16..32].copy_from_slice(&promote.u128().to_le_bytes());
        buf[32..48].copy_from_slice(&kind0.u128().to_le_bytes());
        buf[48..64].copy_from_slice(&kind1.u128().to_le_bytes());
        buf[64..80].copy_from_slice(&kind2.u128().to_le_bytes());
        buf[80..88].copy_from_slice(&self.hands.x.to_le_bytes());
        buf
    }

    /// Deserialize from 88 bytes produced by `to_bytes`. Recomputes `digest`;
    /// no allocation.
    pub fn from_bytes(bytes: &[u8; 88]) -> Self {
        let black_bb = BitBoard::from_u128(u128::from_le_bytes(bytes[0..16].try_into().unwrap()));
        let promote = BitBoard::from_u128(u128::from_le_bytes(bytes[16..32].try_into().unwrap()));
        let kind0 = BitBoard::from_u128(u128::from_le_bytes(bytes[32..48].try_into().unwrap()));
        let kind1 = BitBoard::from_u128(u128::from_le_bytes(bytes[48..64].try_into().unwrap()));
        let kind2 = BitBoard::from_u128(u128::from_le_bytes(bytes[64..80].try_into().unwrap()));
        let hands_x = u64::from_le_bytes(bytes[80..88].try_into().unwrap());
        let kind_bb = KindBitBoard::from_raw_parts(promote, kind0, kind1, kind2);
        let mut result = Self {
            black_bb,
            kind_bb,
            hands: Hands { x: hands_x },
            digest: 0,
        };
        for pos in result.kind_bb.occupied() {
            result.digest ^= result.hash_at(pos);
        }
        result
    }

    pub fn sfen(&self) -> String {
        PositionAux::new(self.clone(), None).sfen()
    }

    pub fn try_set(&mut self, pos: Square, color: Color, kind: Kind) -> anyhow::Result<()> {
        self.check_can_set(pos, color, kind)?;
        self.set(pos, color, kind);
        Ok(())
    }

    pub fn check_can_set(&self, pos: Square, color: Color, kind: Kind) -> anyhow::Result<()> {
        if self.get(pos).is_some() {
            bail!("already occupied");
        }
        if self.used_kind_count(kind) >= kind.max_count() {
            bail!("too many pieces");
        }
        if kind == Kind::Pawn && self.col_has_pawn(color, pos.col()) {
            bail!("double pawns");
        }
        if !is_movable(color, pos, kind) {
            bail!("unmovable");
        }
        Ok(())
    }

    fn used_kind_count(&self, kind: Kind) -> u32 {
        let kind = kind.maybe_unpromote();
        let mut res = Color::iter()
            .map(|c| self.hands().count(c, kind))
            .sum::<usize>() as u32;
        res += self.kind_bb().bitboard(kind).count_ones();
        if let Some(kind) = kind.promote() {
            res += self.kind_bb().bitboard(kind).count_ones();
        }
        res
    }

    fn col_has_pawn(&self, color: Color, col: usize) -> bool {
        let pawn_bb = self.bitboard(color, Kind::Pawn).u128();
        let mask = (1 << (col * 9 + 9)) - (1 << (col * 9));
        pawn_bb & mask != 0
    }
}

#[derive(Clone)]
pub struct PositionAux {
    core: Position,
    occupied: BitBoard,
    white_bb: BitBoard,
    white_king_pos: Option<Square>,
    black_king_pos: Option<Option<Square>>,
    stone: Option<BitBoard>,
    stone_digest: u64,
    /// Per-square 4-bit kind cache. Lives here (not on `Position`) so stored
    /// `Position` instances stay slim. Built lazily in `PositionAux::new`,
    /// maintained incrementally on `set`/`unset`/`change_kind`/`move_piece`.
    kind_at: KindCache,
}

impl Default for PositionAux {
    fn default() -> Self {
        Self {
            core: Position::default(),
            occupied: BitBoard::default(),
            white_bb: BitBoard::default(),
            white_king_pos: None,
            black_king_pos: None,
            stone: None,
            stone_digest: 0,
            kind_at: EMPTY_KIND_CACHE,
        }
    }
}

impl PartialEq for PositionAux {
    fn eq(&self, other: &Self) -> bool {
        self.core == other.core && self.stone == other.stone
    }
}

impl Eq for PositionAux {}

impl Debug for PositionAux {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.clone().sfen_url())
    }
}

impl PositionAux {
    pub fn new(core: Position, stone: Option<BitBoard>) -> Self {
        let kind_at = build_kind_cache(core.kind_bb());
        Self::from_parts(core, kind_at, stone)
    }

    /// Build a `PositionAux` from a pre-computed kind cache. Skips the
    /// O(occupied) cache rebuild in `new` — caller is responsible for keeping
    /// `kind_at` in sync with `core.kind_bb()`. Used by low-memory solvers that
    /// maintain the cache alongside `Position` to avoid per-conversion rebuilds.
    pub fn from_parts(core: Position, kind_at: KindCache, stone: Option<BitBoard>) -> Self {
        let occupied = core.kind_bb().occupied();
        let white_bb = occupied.and_not(core.black());
        let mut res = Self {
            core,
            white_bb,
            occupied,
            white_king_pos: None,
            black_king_pos: None,
            stone: None,
            stone_digest: 0,
            kind_at,
        };
        if let Some(stone) = stone {
            res.set_stone(stone);
        }
        res
    }

    /// Read-only access to the per-square kind cache. Callers maintaining
    /// `(Position, KindCache)` storage use this to snapshot the cache for
    /// inheritance into descendants.
    #[inline(always)]
    pub fn kind_at(&self) -> &KindCache {
        &self.kind_at
    }

    #[inline]
    pub(crate) fn moved_digest(&self, movement: &Movement) -> u64 {
        self.core.moved_digest(movement) ^ self.stone_digest
    }

    pub fn kind_bb(&self, kind: Kind) -> BitBoard {
        self.core.kind_bb().bitboard(kind)
    }

    #[inline(always)]
    pub fn bitboard(&self, color: Color, kind: Kind) -> BitBoard {
        self.kind_bb(kind) & self.color_bb(color)
    }

    pub fn occupied_bb(&self) -> BitBoard {
        self.occupied
    }

    pub(crate) fn capturable_by(&self, color: Color) -> BitBoard {
        if color.is_black() {
            self.white_bb()
        } else {
            self.core.black()
        }
    }

    /// Returns `color's pieces ∪ stones`. `occupied` already carries stone bits
    /// (see set_stone), so this can be expressed as `occupied & ~other_color`,
    /// a single bitboard op — replaces the previous "color_bb (| stone if Some)"
    /// pattern that required reading the `stone` field on every call.
    #[inline(always)]
    pub(crate) fn color_bb_and_stone(&self, color: Color) -> BitBoard {
        let other_color_bb = if color.is_black() {
            self.white_bb()
        } else {
            self.core.black()
        };
        self.occupied.and_not(other_color_bb)
    }

    pub fn black_bb(&self) -> BitBoard {
        self.core.black()
    }

    pub fn white_bb(&self) -> BitBoard {
        self.white_bb
    }

    pub fn color_bb(&self, color: Color) -> BitBoard {
        if color.is_black() {
            self.core.black()
        } else {
            self.white_bb()
        }
    }

    pub fn hands(&self) -> Hands {
        self.core.hands()
    }

    #[inline(always)]
    pub(crate) fn must_get_kind(&self, pos: Square) -> Kind {
        must_kind_from_cache(&self.kind_at, pos)
    }

    #[inline(always)]
    pub(crate) fn get_kind(&self, dest: Square) -> Option<Kind> {
        kind_from_cache(&self.kind_at, dest)
    }

    #[inline(always)]
    pub fn get(&self, pos: Square) -> Option<(Color, Kind)> {
        if self.has_stone(pos) {
            return None;
        }
        if !self.occupied_bb().contains(pos) {
            return None;
        }
        Some((
            Color::from_is_black(self.black_bb().contains(pos)),
            self.must_get_kind(pos),
        ))
    }

    pub fn turn(&self) -> Color {
        self.core.turn()
    }

    pub(crate) fn pawn_silver_goldish(&self) -> BitBoard {
        self.core.kind_bb().pawn_silver_goldish()
    }

    pub(crate) fn bishopish(&self) -> BitBoard {
        self.core.kind_bb.bishopish()
    }

    pub(crate) fn rookish(&self) -> BitBoard {
        self.core.kind_bb.rookish()
    }

    /// Promote-layer bitboard accessor; splits `bishopish() & black_bb` into
    /// raw and promoted variants without two `bitboard(BLACK, kind)` lookups.
    #[inline(always)]
    pub(crate) fn kind_bb_promote_layer(&self) -> BitBoard {
        self.core.kind_bb.promote_layer()
    }

    pub fn pawn_drop(&self) -> bool {
        self.core.pawn_drop()
    }

    pub fn checked_slow(&mut self, king_color: Color) -> bool {
        attacker(self, king_color, true).is_some()
    }

    pub(crate) fn white_king_pos(&mut self) -> Square {
        if self.white_king_pos.is_none() {
            self.white_king_pos = Some((self.kind_bb(Kind::King) & self.white_bb()).singleton());
        }
        self.white_king_pos.unwrap()
    }

    pub(crate) fn black_king_pos(&mut self) -> Option<Square> {
        if self.black_king_pos.is_none() {
            self.black_king_pos = Some((self.kind_bb(Kind::King) & self.black_bb()).next());
        }
        self.black_king_pos.unwrap()
    }

    pub fn do_move(&mut self, movement: &Movement) {
        let turn = self.turn();

        match movement {
            Movement::Move {
                source,
                source_kind_hint,
                dest,
                promote,
                capture_kind_hint,
            } => {
                let source_kind = source_kind_hint.unwrap_or_else(|| self.must_get_kind(*source));
                let capture_kind = capture_kind_hint.unwrap_or_else(|| self.get_kind(*dest));
                if let Some(capture_kind) = capture_kind {
                    self.unset(*dest, turn.opposite(), capture_kind);
                    self.hands_mut().add(turn, capture_kind.maybe_unpromote());
                }
                if *promote {
                    let dest_kind = source_kind.promote().unwrap();
                    self.unset(*source, turn, source_kind);
                    self.set(*dest, turn, dest_kind);
                } else {
                    // Common path (~70% of moves): no promotion, no kind change.
                    // `move_piece` fuses unset(src) + set(dst) into a single
                    // XOR-toggle on each layer bitboard.
                    self.move_piece(*source, *dest, turn, source_kind);
                }

                self.core.set_pawn_drop(false);
                self.core.toggle_turn();
            }
            Movement::Drop(pos, kind) => {
                self.set(*pos, turn, *kind);
                self.hands_mut().remove(turn, *kind);

                self.core.set_pawn_drop(*kind == Kind::Pawn);
                self.core.toggle_turn();
            }
        }
    }

    pub fn digest(&self) -> u64 {
        self.core.digest() ^ self.stone_digest
    }

    #[inline(always)]
    pub fn unset(&mut self, pos: Square, color: Color, kind: Kind) {
        self.occupied.unset(pos);
        if color.is_white() {
            self.white_bb.unset(pos);
        }

        if kind == Kind::King {
            if color.is_black() {
                self.black_king_pos = None;
            } else {
                self.white_king_pos = None;
            }
        }

        self.core.unset(pos, color, kind);
        write_kind_idx(&mut self.kind_at, pos.index(), 0);
    }

    #[inline(always)]
    pub fn set(&mut self, pos: Square, color: Color, kind: Kind) {
        debug_assert!(!self.has_stone(pos));
        self.occupied.set(pos);
        if color.is_white() {
            self.white_bb.set(pos);
        }

        if kind == Kind::King {
            if color.is_black() {
                self.black_king_pos = Some(Some(pos));
            } else {
                self.white_king_pos = Some(pos);
            }
        }

        self.core.set(pos, color, kind);
        write_kind_idx(&mut self.kind_at, pos.index(), encode_kind(kind));
    }

    /// Replace the kind at `pos` (same color, neither old nor new is King).
    /// Skips occupied / white_bb / King tracking mutations that would cancel.
    #[inline(always)]
    pub fn change_kind(&mut self, pos: Square, color: Color, old: Kind, new: Kind) {
        debug_assert_ne!(old, Kind::King);
        debug_assert_ne!(new, Kind::King);
        self.core.change_kind(pos, color, old, new);
        write_kind_idx(&mut self.kind_at, pos.index(), encode_kind(new));
    }

    /// Move a piece from `src` to `dst` (same color, same kind). Faster than
    /// `unset(src) + set(dst)` due to XOR-toggle on bitboards (one op per
    /// layer/color bitboard instead of two) and a single combined digest XOR.
    #[inline(always)]
    pub fn move_piece(&mut self, src: Square, dst: Square, color: Color, kind: Kind) {
        debug_assert!(!self.has_stone(dst));
        let mask = (1u128 << src.index()) | (1u128 << dst.index());
        self.occupied.toggle_mask(mask);
        if color.is_white() {
            self.white_bb.toggle_mask(mask);
        }
        if kind == Kind::King {
            if color.is_black() {
                self.black_king_pos = Some(Some(dst));
            } else {
                self.white_king_pos = Some(dst);
            }
        }
        self.core.move_piece(src, dst, color, kind);
        let encoded = encode_kind(kind);
        write_kind_idx(&mut self.kind_at, src.index(), 0);
        write_kind_idx(&mut self.kind_at, dst.index(), encoded);
    }

    pub fn hands_mut(&mut self) -> &mut Hands {
        self.core.hands_mut()
    }

    pub fn set_turn(&mut self, color: Color) {
        self.core.set_turn(color);
    }

    pub fn shift(&mut self, dir: Direction) {
        if let Some(stone) = self.stone.as_mut() {
            stone.shift(dir)
        }
        self.occupied.shift(dir);
        self.white_bb.shift(dir);
        if let Some(pos) = self.white_king_pos.as_mut() {
            pos.shift(dir)
        }
        self.black_king_pos
            .as_mut()
            .map(|pos| pos.as_mut().map(|pos| pos.shift(dir)));

        self.core.shift(dir);
        // Per-square cache cannot be shifted directly; rebuild from bitboards.
        self.kind_at = build_kind_cache(self.core.kind_bb());
    }

    pub fn must_king_pos(&mut self, king_color: Color) -> Square {
        if king_color.is_black() {
            self.black_king_pos().unwrap()
        } else {
            self.white_king_pos()
        }
    }

    pub(crate) fn must_turn_king_pos(&mut self) -> Square {
        if self.turn().is_black() {
            self.black_king_pos().unwrap()
        } else {
            self.white_king_pos()
        }
    }

    pub fn white_king_attack_squares(&mut self, kind: Kind) -> BitBoard {
        let white_king_pos = self.white_king_pos();
        reachable_sub(self, Color::WHITE, white_king_pos, kind)
    }

    pub(crate) fn core(&self) -> &Position {
        &self.core
    }

    pub fn from_sfen(s: &str) -> anyhow::Result<Self> {
        sfen::decode_position(s)
    }

    pub fn sfen(&self) -> String {
        sfen::encode_position(self)
    }

    pub fn sfen_url(&self) -> String {
        sfen::sfen_to_image_url(&self.sfen())
    }

    pub fn set_pawn_drop(&mut self, pawn_drop: bool) {
        self.core.set_pawn_drop(pawn_drop);
    }

    pub fn stone(&self) -> &Option<BitBoard> {
        &self.stone
    }

    /// Serialize to 105 bytes: `core.to_bytes()` (88) + stone_flag (1) + stone u128 LE (16).
    /// stone_flag=0 → no stone; stone_flag=1 → stone present in the following 16 bytes.
    pub fn to_bytes(&self) -> [u8; 105] {
        let mut buf = [0u8; 105];
        buf[0..88].copy_from_slice(&self.core.to_bytes());
        if let Some(s) = self.stone {
            buf[88] = 1;
            buf[89..105].copy_from_slice(&s.u128().to_le_bytes());
        }
        buf
    }

    /// Deserialize from 105 bytes produced by `to_bytes`.
    pub fn from_bytes(bytes: &[u8; 105]) -> Self {
        let core = Position::from_bytes(bytes[0..88].try_into().unwrap());
        let stone = if bytes[88] == 0 {
            None
        } else {
            Some(BitBoard::from_u128(u128::from_le_bytes(
                bytes[89..105].try_into().unwrap(),
            )))
        };
        Self::new(core, stone)
    }

    fn has_stone(&self, pos: Square) -> bool {
        self.stone
            .as_ref()
            .map_or(false, |stone| stone.contains(pos))
    }

    pub fn undo_move(&mut self, token: &super::UndoMove) -> Movement {
        let mut core = self.core.clone();
        let res = core.undo_move(token);
        *self = Self::new(core, self.stone);
        res
    }

    pub fn col_has_pawn(&self, color: Color, col: usize) -> bool {
        self.core.col_has_pawn(color, col)
    }

    pub fn flipped(&self) -> Self {
        let mut core = Position::default();
        let mut stone = BitBoard::default();
        for pos in Square::iter() {
            if let Some((color, kind)) = self.get(pos) {
                core.set(pos.flipped(), color.opposite(), kind);
            }
            if self.stone().map_or(false, |stone| stone.contains(pos)) {
                stone.set(pos.flipped());
            }
        }
        core.set_turn(self.turn().opposite());
        for k in KINDS[0..NUM_HAND_KIND].iter().copied() {
            core.hands_mut()
                .add_n(Color::WHITE, k, self.hands().count(Color::BLACK, k));
            core.hands_mut()
                .add_n(Color::BLACK, k, self.hands().count(Color::WHITE, k));
        }

        Self::new(core, stone.into())
    }

    pub fn set_stone(&mut self, stone: BitBoard) {
        assert_eq!(self.stone, None);
        self.stone = Some(stone);
        self.occupied |= stone;

        for pos in stone {
            self.stone_digest ^= zobrist_stone(pos);
        }
    }

    pub fn try_set(&mut self, pos: Square, color: Color, kind: Kind) -> anyhow::Result<()> {
        self.core.try_set(pos, color, kind)
    }

    pub fn can_set(&self, pos: Square, color: Color, kind: Kind) -> anyhow::Result<()> {
        self.core.check_can_set(pos, color, kind)
    }

    pub fn settable_bb(&self, color: Color, kind: Kind) -> BitBoard {
        if self.core.used_kind_count(kind) >= kind.max_count() {
            return BitBoard::EMPTY;
        }

        let occupied = self.occupied_bb();
        let unmovable = kind.unmovable_bb(color);
        if kind == Kind::Pawn {
            let pawn_mask = self.bitboard(color, Kind::Pawn).col_mask_bb();
            return BitBoard::FULL.and_not(occupied | unmovable | pawn_mask);
        }
        BitBoard::FULL.and_not(occupied | unmovable)
    }

    pub fn undo_digest(&self, token: &UndoMove) -> u64 {
        self.core.undo_digest(token) ^ self.stone_digest
    }

    pub fn is_illegal_initial_position(&self) -> bool {
        for c in Color::iter() {
            let pawns = self.bitboard(c, Kind::Pawn).u128();
            for i in 0..9 {
                if (pawns >> (i * 9) & 0x1FF).count_ones() > 1 {
                    return true;
                }
            }
            for k in [Kind::Pawn, Kind::Lance, Kind::Knight] {
                if !(k.unmovable_bb(c) & self.bitboard(c, k)).is_empty() {
                    return true;
                }
            }
        }
        false
    }

    pub(crate) fn king_pos(&mut self, king_color: Color) -> Option<Square> {
        if king_color.is_black() {
            self.black_king_pos()
        } else {
            self.white_king_pos().into()
        }
    }

    // TODO: remember attackers
}

/// Compact frontier-storage form of a position: `Position` plus the per-square
/// `KindCache` that `PositionAux` would otherwise rebuild on every conversion.
///
/// Algorithms that store many positions in `Vec` use this instead of bare
/// `Position` so loading back into `PositionAux` is O(1) (via `from_parts`)
/// rather than O(occupied) (via `PositionAux::new`'s `build_kind_cache`).
/// The cache is propagated to descendants incrementally via `apply_movement`,
/// avoiding any full rebuild across the search tree.
///
/// Memory: `Position` (96 B) + `KindCache` (41 B, ~48 B aligned) ≈ 144 B/entry.
#[derive(Clone)]
pub struct CachedPosition {
    core: Position,
    kind_at: KindCache,
}

impl CachedPosition {
    /// Snapshot the core+cache of `aux`. The cache is consumed by reference
    /// (it's `[u8; 41]`, cheap to copy).
    #[inline]
    pub fn from_aux(aux: &PositionAux) -> Self {
        Self {
            core: aux.core.clone(),
            kind_at: aux.kind_at,
        }
    }

    /// Build from a `Position` without an existing cache — rebuilds the cache
    /// from bitboards (O(occupied)). Use only for entry points (initial seed
    /// loading, checkpoint resume) where no cache is available.
    pub fn from_position(core: Position) -> Self {
        let kind_at = build_kind_cache(core.kind_bb());
        Self { core, kind_at }
    }

    /// Reconstruct a `PositionAux` without rescanning bitboards for the cache.
    #[inline]
    pub fn to_aux(&self, stone: Option<BitBoard>) -> PositionAux {
        PositionAux::from_parts(self.core.clone(), self.kind_at, stone)
    }

    #[inline]
    pub fn core(&self) -> &Position {
        &self.core
    }

    /// Apply `m` to both the core position and the kind cache. The cache
    /// update is O(1) per movement (touches at most 2 squares), so this is
    /// only marginally more expensive than `Position::do_move` alone.
    #[inline]
    pub fn apply_movement(&mut self, m: &Movement) {
        apply_movement_to_cache(&mut self.kind_at, m);
        self.core.do_move(m);
    }

    /// Functional variant: clone, apply movement, return.
    #[inline]
    pub fn after_movement(&self, m: &Movement) -> Self {
        // Avoid `self.clone()` (which would copy then we mutate) by building
        // the descendant's fields directly. Saves one redundant memcpy on the
        // hot push path (`CachedPosition::from_aux(&aux).after_movement(m)`
        // becomes equivalent to building the descendant from `aux` in one shot).
        let mut kind_at = self.kind_at;
        apply_movement_to_cache(&mut kind_at, m);
        let mut core = self.core.clone();
        core.do_move(m);
        Self { core, kind_at }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        direction::Direction,
        position::{position::PositionAux, BitBoard, Square},
    };

    use super::{Color, Kind, Position};

    #[test]
    fn test_shift() {
        let mut position = Position::default();
        position.set(Square::new(0, 0), Color::BLACK, Kind::Pawn);
        position.shift(Direction::Down);

        assert_eq!(position.digest(), {
            let mut position = Position::default();
            position.set(Square::new(0, 1), Color::BLACK, Kind::Pawn);
            position.digest()
        });
    }

    #[test]
    fn digest_no_collision() {
        for (pos1, pos2) in [
            (
                "9/9/5B3/4kb3/9/5+R3/9/9/2+R6 b 4g4s4n4l18p",
                "9/9/5B3/4kb3/9/5+R3/2R6/9/9 b 4g4s4n4l18p",
            ),
            (
                "9/9/5B3/4kb3/2R6/5+R3/9/9/9 b 4g4s4n4l18p",
                "9/9/5B3/4kb3/9/5+R3/2R6/9/9 b 4g4s4n4l18p",
            ),
        ] {
            let mut pos1 = PositionAux::from_sfen(pos1).unwrap();
            let mut pos2 = PositionAux::from_sfen(pos2).unwrap();

            for pawn_drop1 in [true, false] {
                for pawn_drop2 in [true, false] {
                    pos1.set_pawn_drop(pawn_drop1);
                    pos2.set_pawn_drop(pawn_drop2);

                    assert_ne!(pos1, pos2);
                    assert_ne!(pos1.digest(), pos2.digest(), "{:?} {:?}", pos1, pos2);

                    pretty_assertions::assert_eq!(
                        PositionAux::from_sfen(&pos1.sfen()).unwrap(),
                        pos1
                    );
                    pretty_assertions::assert_eq!(
                        PositionAux::from_sfen(&pos2.sfen()).unwrap(),
                        pos2
                    );
                }
            }
        }
    }

    #[test]
    fn position_size() {
        // 16 (black_bb) + 64 (kind_bb) + 8 (hands) + 8 (digest) = 96.
        assert_eq!(std::mem::size_of::<Position>(), 96);
    }

    #[test]
    fn test_stone() {
        use crate::position::Position;

        let mut stone = BitBoard::default();
        stone.set(Square::new(0, 0));
        let position = PositionAux::new(Position::default(), stone.into());

        assert_eq!(position.sfen(), "8O/9/9/9/9/9/9/9/9 b - 1");
    }
}
