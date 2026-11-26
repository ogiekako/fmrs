use crate::position::{
    advance::options::{AdvanceError, AdvanceResult},
    bitboard::{
        bishop_power, bishop_reachable, king_then_king_or_night_power, knight_power, lance_power,
        lance_reachable, rook_power, rook_reachable,
    },
    checked,
    position::PositionAux,
    rule::{is_legal_move, is_movable, promotable},
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

/// Squares strictly between `a` (king) and `b` (line attacker), ordered closest-to-`a` first.
/// Layout per entry: `[len, sq_0, sq_1, ..., sq_6]`.
static ORDERED_BETWEEN: [[[u8; 8]; 81]; 81] = {
    let mut res = [[[0u8; 8]; 81]; 81];
    let mut a = 0;
    while a < 81 {
        let ac = (a / 9) as i32;
        let ar = (a % 9) as i32;
        let mut b = 0;
        while b < 81 {
            if a != b {
                let bc = (b / 9) as i32;
                let br = (b % 9) as i32;
                let dc = bc - ac;
                let dr = br - ar;
                if dc == 0 || dr == 0 || dc.abs() == dr.abs() {
                    let sc = dc.signum();
                    let sr = dr.signum();
                    let mut c = ac + sc;
                    let mut r = ar + sr;
                    let mut len = 0usize;
                    while c != bc || r != br {
                        res[a][b][1 + len] = (c * 9 + r) as u8;
                        len += 1;
                        c += sc;
                        r += sr;
                    }
                    res[a][b][0] = len as u8;
                }
            }
            b += 1;
        }
        a += 1;
    }
    res
};

// #[inline(never)]
pub(super) fn attack_preventing_movements<'a>(
    position: &'a PositionAux,
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
    position: &'a PositionAux,
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
        position: &'a PositionAux,
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
            should_return_check,
            result,
            is_mate: true,
            num_branches_without_pawn_drop: 0,
            options,
        })
    }

    #[inline]
    fn leap_state(&mut self) -> LeapState {
        if let Some(s) = self.leap_state {
            return s;
        }
        let turn = self.position.turn();
        let turn_bb = self.position.color_bb(turn);
        let s = LeapState {
            lance: self.position.kind_bb(Kind::Lance) & turn_bb,
            knight: self.position.kind_bb(Kind::Knight) & turn_bb,
            bishopish: self.position.bishopish() & turn_bb,
            rookish: self.position.rookish() & turn_bb,
        };
        self.leap_state = Some(s);
        s
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
            let entry = &ORDERED_BETWEEN[self.position.must_turn_king_pos().index()]
                [attacker_pos.index()];
            let len = entry[0] as usize;
            for i in 0..len {
                self.add_movements_to(Square::from_index(entry[1 + i] as usize), true)?;
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
                if kind == Kind::Pawn && self.position.col_has_pawn(turn, dest.col()) {
                    continue;
                }
                if is_movable(turn, dest, kind) {
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
                for promote in [true, false] {
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

        // Leap pieces: cached `LeapState` + direct magic/power queries without `reachable` exclude mask.
        let leap = self.leap_state();
        let occ = self.position.occupied_bb();
        if !leap.lance.is_empty() {
            let sources = lance_reachable(occ, opposite, dest) & leap.lance;
            if !sources.is_empty() {
                self.add_leap_simple(dest, capture_kind, sources, Kind::Lance, turn)?;
            }
        }
        if !leap.knight.is_empty() {
            let sources = knight_power(opposite, dest) & leap.knight;
            if !sources.is_empty() {
                self.add_leap_simple(dest, capture_kind, sources, Kind::Knight, turn)?;
            }
        }
        if !leap.bishopish.is_empty() {
            let sources = bishop_reachable(occ, dest) & leap.bishopish;
            if !sources.is_empty() {
                self.add_leap_promotable(dest, capture_kind, sources, turn)?;
            }
        }
        if !leap.rookish.is_empty() {
            let sources = rook_reachable(occ, dest) & leap.rookish;
            if !sources.is_empty() {
                self.add_leap_promotable(dest, capture_kind, sources, turn)?;
            }
        }
        Ok(())
    }

    fn add_leap_simple(
        &mut self,
        dest: Square,
        capture_kind: Option<Kind>,
        sources: BitBoard,
        kind: Kind,
        turn: Color,
    ) -> AdvanceResult<()> {
        let movable_unpromoted = is_movable(turn, dest, kind);
        let dest_promotable = promotable(dest, turn);
        for source_pos in sources {
            if self.pinned().is_unpin_move(source_pos, dest) {
                continue;
            }
            if dest_promotable || promotable(source_pos, turn) {
                self.maybe_add_move(
                    Movement::move_with_hint(source_pos, kind, dest, true, capture_kind),
                    kind,
                )?;
            }
            if movable_unpromoted {
                self.maybe_add_move(
                    Movement::move_with_hint(source_pos, kind, dest, false, capture_kind),
                    kind,
                )?;
            }
        }
        Ok(())
    }

    fn add_leap_promotable(
        &mut self,
        dest: Square,
        capture_kind: Option<Kind>,
        sources: BitBoard,
        turn: Color,
    ) -> AdvanceResult<()> {
        let dest_promotable = promotable(dest, turn);
        for source_pos in sources {
            if self.pinned().is_unpin_move(source_pos, dest) {
                continue;
            }
            let source_kind = self.position.must_get_kind(source_pos);
            if source_kind.can_promote() && (dest_promotable || promotable(source_pos, turn)) {
                self.maybe_add_move(
                    Movement::move_with_hint(source_pos, source_kind, dest, true, capture_kind),
                    source_kind,
                )?;
            }
            self.maybe_add_move(
                Movement::move_with_hint(source_pos, source_kind, dest, false, capture_kind),
                source_kind,
            )?;
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
        checked(&np, self.position.turn().opposite())
    }

    fn maybe_add_move(&mut self, movement: Movement, kind: Kind) -> AdvanceResult<()> {
        let is_king_move = kind == Kind::King;

        // TODO: check the second attacker
        if !is_king_move && self.attacker.double_check.is_some() {
            let mut np = self.position.clone();
            np.do_move(&movement);
            if checked(&np, self.position.turn()) {
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
                !common::checked(&np, self.position.turn())
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

#[derive(Clone, Copy, Debug)]
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
    position: &PositionAux,
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
    if opponent_bb.is_empty() {
        return attacker;
    }

    let occupied = position.occupied_bb();

    // Lance: pre-filter by enemy lances on the forward ray before computing `lance_reachable`.
    let lance_cands = position.kind_bb(Kind::Lance) & opponent_bb & lance_power(king_color, king_pos);
    if !lance_cands.is_empty() {
        let attacking_lances = lance_reachable(occupied, king_color, king_pos) & lance_cands;
        if !attacking_lances.is_empty() {
            let attacker_pos = attacking_lances.singleton();
            if update_attacker(&mut attacker, attacker_pos, Kind::Lance, early_return) {
                return attacker;
            }
        }
    }

    // Rook/ProRook: pre-filter by `rook_power(king_pos)` before magic lookup.
    let rook_cands = position.rookish() & opponent_bb & rook_power(king_pos);
    if !rook_cands.is_empty() {
        let attacking_rooks = rook_cands & rook_reachable(occupied, king_pos);
        for attacker_pos in attacking_rooks {
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

    // Bishop/ProBishop: pre-filter by `bishop_power(king_pos)` before magic lookup.
    let bishop_cands = position.bishopish() & opponent_bb & bishop_power(king_pos);
    if !bishop_cands.is_empty() {
        let attacking_bishops = bishop_cands & bishop_reachable(occupied, king_pos);
        for attacker_pos in attacking_bishops {
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
