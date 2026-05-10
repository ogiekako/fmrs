use crate::position::{
    advance::options::{AdvanceError, AdvanceResult},
    bitboard::{
        bishop_reachable, king_then_king_or_night_power, knight_power, lance_reachable,
        rook_reachable,
    },
    checked,
    position::PositionAux,
    rule::{is_legal_drop, is_legal_move, is_movable, promotable},
};

use crate::{
    piece::{Color, Kind},
    position::{
        bitboard::{self, power, rule::king_power, BitBoard},
        Movement, Square,
    },
};

use super::{
    common,
    pinned::{pinned_into, Pinned},
    AdvanceOptions,
};

// #[inline(never)]
pub(super) fn attack_preventing_movements<'a>(
    position: &'a mut PositionAux,
    should_return_check: bool,
    options: &'a AdvanceOptions,
    attacker_hint: Option<Attacker>,
    result: &'a mut Vec<Movement>,
) -> AdvanceResult</* is legal mate */ bool> {
    let mut ctx = Context::new(
        position,
        should_return_check,
        options,
        attacker_hint,
        result,
    )?;
    ctx.advance()?;
    Ok(ctx.is_mate && !position.pawn_drop())
}

struct Context<'a> {
    position: &'a mut PositionAux,
    occupied_without_king: BitBoard,
    /// Lazy: pinned は `add_movements_to` (capture/block) でしか参照されない。
    /// `king_move` が早期に脱出を見つけて Err 返すと参照されないので、
    /// 計算 (~6% CPU) を回避できる。
    /// Inlined Pinned + flag avoids the return-by-value memcpy of `pinned()`.
    pinned: Pinned,
    pinned_initialized: bool,
    /// Lazy: own-color leap pieces grouped per leap_kind. Computed once and
    /// reused across capture/block dest iterations to avoid redundant
    /// `bitboard(turn, kind)` lookups.
    leap_state: Option<LeapState>,
    attacker: Attacker,
    pawn_mask: Option<usize>,
    should_return_check: bool,
    // Mutable fields
    result: &'a mut Vec<Movement>,
    is_mate: bool,
    num_branches_without_pawn_drop: usize,

    options: &'a AdvanceOptions,
}

#[derive(Clone, Copy)]
struct LeapState {
    /// Own raw Lance pieces (ProLance is gold-like, not in this set).
    lance: BitBoard,
    /// Own raw Knight pieces (ProKnight is gold-like).
    knight: BitBoard,
    /// Own Bishop | ProBishop (both line pieces sharing diagonal motion).
    bishopish: BitBoard,
    /// Own Rook | ProRook (both line pieces sharing straight motion).
    rookish: BitBoard,
}

impl<'a> Context<'a> {
    // #[inline(never)]
    fn new(
        position: &'a mut PositionAux,
        should_return_check: bool,
        options: &'a AdvanceOptions,
        attacker_hint: Option<Attacker>,
        result: &'a mut Vec<Movement>,
    ) -> AdvanceResult<Self> {
        let turn = position.turn();
        let attacker = match attacker_hint {
            Some(attacker) => attacker,
            None => attacker(position, turn, false).ok_or(AdvanceError::NoAttacker)?,
        };

        let mut occupied_without_king = position.occupied_bb();
        occupied_without_king.unset(position.must_king_pos(turn));

        Ok(Self {
            position,
            occupied_without_king,
            pinned: Pinned::default(),
            pinned_initialized: false,
            leap_state: None,
            attacker,
            pawn_mask: None, // TODO: move to PositionAux
            should_return_check,
            result,
            is_mate: true,
            num_branches_without_pawn_drop: 0,
            options,
        })
    }

    fn leap_state(&mut self) -> LeapState {
        if let Some(s) = self.leap_state {
            return s;
        }
        let turn = self.position.turn();
        let lance = self.position.bitboard(turn, Kind::Lance);
        let knight = self.position.bitboard(turn, Kind::Knight);
        let bishop = self.position.bitboard(turn, Kind::Bishop);
        let pro_bishop = self.position.bitboard(turn, Kind::ProBishop);
        let rook = self.position.bitboard(turn, Kind::Rook);
        let pro_rook = self.position.bitboard(turn, Kind::ProRook);
        let s = LeapState {
            lance,
            knight,
            bishopish: bishop | pro_bishop,
            rookish: rook | pro_rook,
        };
        self.leap_state = Some(s);
        s
    }

    fn pawn_mask(&mut self) -> usize {
        *self.pawn_mask.get_or_insert_with(|| {
            let mut mask = Default::default();
            for pos in self.position.bitboard(self.position.turn(), Kind::Pawn) {
                mask |= 1 << pos.col()
            }
            mask
        })
    }

    fn pinned(&mut self) -> &Pinned {
        if !self.pinned_initialized {
            let turn = self.position.turn();
            pinned_into(self.position, turn, turn, &mut self.pinned);
            self.pinned_initialized = true;
        }
        &self.pinned
    }

    // #[inline(never)]
    fn advance(&mut self) -> AdvanceResult<()> {
        self.king_move()?;

        if self.attacker.double_check.is_none() {
            self.capture(self.attacker.pos)?;
            self.block(self.attacker.pos, self.attacker.kind)?;
        }

        Ok(())
    }

    // #[inline(never)]
    fn block(&mut self, attacker_pos: Square, attacker_kind: Kind) -> AdvanceResult<()> {
        if attacker_kind.is_line_piece() {
            let blockable = self.blockable_squares(attacker_pos, attacker_kind);
            for dest in blockable {
                self.add_movements_to(dest, true)?;
            }
        }
        Ok(())
    }

    // #[inline(never)]
    fn capture(&mut self, attacker_pos: Square) -> AdvanceResult<()> {
        self.add_movements_to(attacker_pos, false)
    }

    // #[inline(never)]
    fn king_move(&mut self) -> AdvanceResult<()> {
        let king_color = self.position.turn();
        let attacker_color = king_color.opposite();
        let attacker_color_bb = self.position.capturable_by(king_color);

        let mut king_reachable = king_power(self.position.must_turn_king_pos())
            .and_not(self.position.color_bb_and_stone(king_color));
        if king_reachable.is_empty() {
            return Ok(());
        }
        let mut seen_cands = BitBoard::default();
        let non_line_cands =
            king_then_king_or_night_power(king_color, self.position.must_turn_king_pos())
                & attacker_color_bb;
        for attacker_pos in non_line_cands {
            let attacker_kind = self.position.must_get_kind(attacker_pos);
            let attacker_reach = match attacker_kind {
                Kind::Lance | Kind::Bishop | Kind::Rook => continue,
                Kind::ProBishop | Kind::ProRook => king_power(attacker_pos),
                _ => {
                    seen_cands.set(attacker_pos);
                    power(attacker_color, attacker_pos, attacker_kind)
                }
            };

            king_reachable = king_reachable.and_not(attacker_reach);
            if king_reachable.is_empty() {
                return Ok(());
            }
        }

        let lances = self.position.bitboard(attacker_color, Kind::Lance);
        let bishipish = self.position.bishopish() & attacker_color_bb;
        let rookish = self.position.rookish() & attacker_color_bb;

        // Per-attacker magic lookup is cheaper than per-dest (line pieces are
        // typically 1-2 while king dests can be up to 8). Build the union of all
        // squares attacked by enemy line pieces, considering the board with the
        // king removed (so the king can't shield itself).
        let mut attacked_by_line = BitBoard::default();
        for lance_pos in lances {
            attacked_by_line |=
                lance_reachable(self.occupied_without_king, attacker_color, lance_pos);
        }
        for bishop_pos in bishipish {
            attacked_by_line |= bishop_reachable(self.occupied_without_king, bishop_pos);
        }
        for rook_pos in rookish {
            attacked_by_line |= rook_reachable(self.occupied_without_king, rook_pos);
        }

        for dest in king_reachable {
            if attacked_by_line.contains(dest) {
                continue;
            }
            let capture_kind = self.position.get_kind(dest);
            let king_pos = self.position.must_turn_king_pos();
            self.maybe_add_move(
                Movement::move_with_hint(king_pos, Kind::King, dest, false, capture_kind),
                Kind::King,
            )?;
        }
        Ok(())
    }

    fn add_movements_to(&mut self, dest: Square, include_drop: bool) -> AdvanceResult<()> {
        let turn = self.position.turn();
        let opposite = turn.opposite();

        // Drop
        if include_drop {
            for kind in self.position.hands().kinds(turn) {
                let pawn_mask = (kind == Kind::Pawn).then(|| self.pawn_mask()).unwrap_or(0);
                if is_legal_drop(turn, dest, kind, pawn_mask) {
                    self.maybe_add_move(Movement::Drop(dest, kind), kind)?;
                }
            }
        }

        let capture_kind = if include_drop {
            None
        } else {
            self.position.get_kind(dest)
        };

        // Around-dest moves: pieces of any kind in king_power(dest) that can step to dest.
        // Line-piece "step" moves are handled by the leap loop below; this loop skips them
        // via `around_dest_move_is_generated_by_leap`.
        // Exclude turn-side king up-front so we don't evaluate it as a source. King moves
        // are emitted by `king_move()`; including the king here only adds a per-source
        // dispatch + branch.
        let king_excl = BitBoard::EMPTY.with(self.position.must_turn_king_pos());
        let around_dest =
            (king_power(dest) & self.position.capturable_by(opposite)).and_not(king_excl);
        for source_pos in around_dest {
            let source_kind = self.position.must_get_kind(source_pos);
            let source_power = self
                .pinned()
                .pinned_area(source_pos)
                .unwrap_or_else(|| bitboard::power(turn, source_pos, source_kind));
            if source_power.contains(dest) {
                if around_dest_move_is_generated_by_leap(source_pos, dest, source_kind, turn) {
                    continue;
                }
                for promote in [false, true] {
                    if promote && source_kind.promote().is_none() {
                        continue;
                    }
                    if !is_legal_move(turn, source_pos, dest, source_kind, promote) {
                        continue;
                    }
                    let movement = Movement::move_with_hint(
                        source_pos,
                        source_kind,
                        dest,
                        promote,
                        capture_kind,
                    );
                    self.maybe_add_move(movement, source_kind)?;
                }
            }
        }

        // Leap pieces: cached `LeapState` avoids 4× repeated `bitboard(turn, kind)`
        // calls per dest. Lance/Knight specialize on a known `source_kind` so the
        // inner `must_get_kind` lookup disappears.
        let leap = self.leap_state();
        if !leap.lance.is_empty() {
            self.add_leap_simple(
                dest,
                capture_kind,
                leap.lance,
                Kind::Lance,
                turn,
                opposite,
            )?;
        }
        if !leap.knight.is_empty() {
            self.add_leap_simple(
                dest,
                capture_kind,
                leap.knight,
                Kind::Knight,
                turn,
                opposite,
            )?;
        }
        if !leap.bishopish.is_empty() {
            self.add_leap_promotable(
                dest,
                capture_kind,
                leap.bishopish,
                Kind::Bishop,
                turn,
                opposite,
            )?;
        }
        if !leap.rookish.is_empty() {
            self.add_leap_promotable(
                dest,
                capture_kind,
                leap.rookish,
                Kind::Rook,
                turn,
                opposite,
            )?;
        }
        Ok(())
    }

    /// Leap path for kinds whose promoted form is not a line piece (Lance, Knight).
    /// `source_kind` is statically known to equal `kind`, so `must_get_kind` is
    /// skipped. is_legal_move を per-source 関数呼び出しせず、不成り合法性は
    /// (turn, dest, kind) にのみ依存する事実を使ってループ前に決定し、
    /// 成り合法性は dest_promotable を事前計算した上で source 側のみ確認する。
    fn add_leap_simple(
        &mut self,
        dest: Square,
        capture_kind: Option<Kind>,
        on_board: BitBoard,
        kind: Kind,
        turn: Color,
        opposite: Color,
    ) -> AdvanceResult<()> {
        let sources = bitboard::reachable(self.position, opposite, dest, kind, false) & on_board;
        let movable_unpromoted = is_movable(turn, dest, kind);
        let dest_promotable = promotable(dest, turn);
        for source_pos in sources {
            if self.pinned().is_unpin_move(source_pos, dest) {
                continue;
            }
            if movable_unpromoted {
                self.maybe_add_move(
                    Movement::move_with_hint(source_pos, kind, dest, false, capture_kind),
                    kind,
                )?;
            }
            // Lance/Knight は can_promote == true (常に成れる)、不成りで届く
            // ところには成りでも届くので、合法性は source/dest のいずれかが
            // 敵陣にあるかだけで決まる。
            if dest_promotable || promotable(source_pos, turn) {
                self.maybe_add_move(
                    Movement::move_with_hint(source_pos, kind, dest, true, capture_kind),
                    kind,
                )?;
            }
        }
        Ok(())
    }

    /// Leap path for line-piece kinds whose promoted form is also a line piece
    /// (Bishop/ProBishop, Rook/ProRook). The actual `source_kind` must be looked
    /// up because `on_board` mixes raw and promoted variants. Bishop/Rook 系は
    /// is_movable が常に true (Pawn/Lance/Knight 制限を受けない) なので不成り
    /// は無条件で合法。成りは can_promote && (source か dest が敵陣) で判定。
    fn add_leap_promotable(
        &mut self,
        dest: Square,
        capture_kind: Option<Kind>,
        on_board: BitBoard,
        kind: Kind,
        turn: Color,
        opposite: Color,
    ) -> AdvanceResult<()> {
        let sources = bitboard::reachable(self.position, opposite, dest, kind, false) & on_board;
        let dest_promotable = promotable(dest, turn);
        for source_pos in sources {
            if self.pinned().is_unpin_move(source_pos, dest) {
                continue;
            }
            let source_kind = self.position.must_get_kind(source_pos);
            // 不成り: 常に合法。
            self.maybe_add_move(
                Movement::move_with_hint(source_pos, source_kind, dest, false, capture_kind),
                source_kind,
            )?;
            // 成り: 既に成った駒は can_promote == false でスキップ。
            if source_kind.can_promote() && (dest_promotable || promotable(source_pos, turn)) {
                self.maybe_add_move(
                    Movement::move_with_hint(source_pos, source_kind, dest, true, capture_kind),
                    source_kind,
                )?;
            }
        }
        Ok(())
    }
}

// Helper methods
impl Context<'_> {
    // #[inline(never)]
    fn is_return_check(&self, movement: &Movement) -> bool {
        let mut np = self.position.clone();
        np.do_move(movement);
        checked(&mut np, self.position.turn().opposite())
    }

    fn maybe_add_move(&mut self, movement: Movement, kind: Kind) -> AdvanceResult<()> {
        let is_king_move = kind == Kind::King;

        // TODO: check the second attacker
        if !is_king_move && self.attacker.double_check.is_some() {
            let mut np = self.position.clone();
            np.do_move(&movement);
            if checked(&mut np, self.position.turn()) {
                return Ok(());
            }
        }

        if self.should_return_check && !self.is_return_check(&movement) {
            return Ok(());
        }

        self.is_mate = false;
        if !movement.is_pawn_drop() {
            self.num_branches_without_pawn_drop += 1;
            self.options
                .check_allowed_branches(self.num_branches_without_pawn_drop)?;
        }

        debug_assert!(
            {
                let mut np = self.position.clone();
                np.do_move(&movement);
                !common::checked(&mut np, self.position.turn())
            },
            "{:?} king checked: posision={:?} movement={:?} next={:?}",
            self.position.turn(),
            self.position,
            movement,
            {
                let mut np = self.position.clone();
                np.do_move(&movement);
                np
            }
        );

        self.result.push(movement);

        Ok(())
    }

    fn blockable_squares(&mut self, attacker_pos: Square, attacker_kind: Kind) -> BitBoard {
        let king_pos = self.position.must_turn_king_pos();
        if king_power(king_pos).contains(attacker_pos) {
            return BitBoard::default();
        }
        bitboard::reachable(
            self.position,
            self.position.turn(),
            king_pos,
            attacker_kind.maybe_unpromote(),
            false,
        ) & bitboard::reachable(
            self.position,
            self.position.turn().opposite(),
            attacker_pos,
            attacker_kind.maybe_unpromote(),
            true,
        )
    }
}

fn around_dest_move_is_generated_by_leap(
    source: Square,
    dest: Square,
    source_kind: Kind,
    turn: Color,
) -> bool {
    let dcol = source.col().abs_diff(dest.col());
    let drow = source.row().abs_diff(dest.row());

    match source_kind {
        Kind::Bishop | Kind::ProBishop => dcol == 1 && drow == 1,
        Kind::Rook | Kind::ProRook => dcol + drow == 1,
        Kind::Lance => {
            dcol == 0
                && ((turn.is_black() && source.row() == dest.row() + 1)
                    || (turn.is_white() && dest.row() == source.row() + 1))
        }
        _ => false,
    }
}

#[derive(Clone, Debug)]
pub struct Attacker {
    pub pos: Square,
    pub kind: Kind,
    pub double_check: Option<(Square, Kind)>,
}

impl Attacker {
    fn new(pos: Square, kind: Kind, double_check: Option<(Square, Kind)>) -> Self {
        Self {
            pos,
            kind,
            double_check,
        }
    }
}

pub fn attacker(
    position: &mut PositionAux,
    king_color: Color,
    early_return: bool,
) -> Option<Attacker> {
    let king_pos = position.king_pos(king_color)?;

    let mut attacker: Option<Attacker> = None;

    let mut opponent_bb = position.capturable_by(king_color);

    let king_power_area = (king_power(king_pos) | knight_power(king_color, king_pos)) & opponent_bb;

    for pos in king_power_area {
        let kind = position.must_get_kind(pos);
        if power(king_color, king_pos, kind).contains(pos)
            && update_attacker(&mut attacker, pos, kind, early_return)
        {
            return attacker;
        }
    }
    opponent_bb = opponent_bb.and_not(king_power_area);

    let occupied = position.occupied_bb();

    // Lance
    let attacking_lances = lance_reachable(occupied, king_color, king_pos) & opponent_bb;
    if !attacking_lances.is_empty() {
        let attacker_pos = attacking_lances.singleton();
        let kind = position.must_get_kind(attacker_pos);
        if matches!(kind, Kind::Lance | Kind::Rook | Kind::ProRook)
            && update_attacker(&mut attacker, attacker_pos, kind, early_return)
        {
            return attacker;
        }

        opponent_bb.unset(attacker_pos);
    }

    for bishop in [false, true] {
        let mut attacker_cands = if bishop {
            position.bishopish()
        } else {
            position.rookish()
        } & opponent_bb;

        if attacker_cands.is_empty() {
            continue;
        }
        attacker_cands &= if bishop {
            bishop_reachable(occupied, king_pos)
        } else {
            rook_reachable(occupied, king_pos)
        };
        if attacker_cands.is_empty() {
            continue;
        }
        for attacker_pos in attacker_cands {
            if update_attacker(
                &mut attacker,
                attacker_pos,
                position.must_get_kind(attacker_pos),
                early_return,
            ) {
                return attacker;
            }
        }
    }

    attacker
}

fn update_attacker(
    attacker: &mut Option<Attacker>,
    pos: Square,
    kind: Kind,
    early_return: bool,
) -> bool {
    if let Some(a) = attacker {
        a.double_check = (pos, kind).into();
        true
    } else {
        *attacker = Some(Attacker::new(pos, kind, None));
        early_return
    }
}
