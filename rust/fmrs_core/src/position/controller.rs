use crate::{
    piece::{BoardPiece, Color, Kind, KindEffect, Kindish, Kinds, NUM_KINDISH, NUM_KIND_EFFECTS},
    position::bitboard::reachable_cont_sub,
};

use super::{
    bitboard::{
        bitboard::{between, outside},
        gold_power, king_power, knight_power, lance_reachable,
        magic::{bishop_reachable, probishop_reachable, prorook_reachable, rook_reachable},
        pawn_power, silver_power,
    },
    position::PositionAux,
    zobrist::zobrist_stone,
    BitBoard, Hands, Movement, Position, PositionExt, Square,
};

#[derive(Default)]
pub struct PositionController {
    stone: BitBoard,
    stone_digest: u64,

    history: Vec<Position>,
    core: Position,

    king: [Option<Square>; 2],

    white: BitBoard,
    black_or_stone: BitBoard,
    occupied: BitBoard,

    pieces: Vec<Option<BoardPiece>>,

    // kind_bb: [BitBoard; NUM_KIND],
    kindish_bb: [BitBoard; NUM_KINDISH],

    king_attack_squares: [[BitBoard; NUM_KIND_EFFECTS]; 2],
    pinning: [BitBoard; 2],
    attackable: [BitBoard; NUM_KINDISH],

    need_update: u64,
    delta_update: u64,
}

const KING_ATTACK_SQUARES_BASE: u64 = 1;
const PINNING_BLACK: u64 = KING_ATTACK_SQUARES_BASE << 2 * NUM_KIND_EFFECTS;
const PINNING_WHITE: u64 = PINNING_BLACK << 1;
const ATTACKABLE_BASE: u64 = PINNING_WHITE << 1;

fn attackable_flag(kind: Kindish) -> u64 {
    ATTACKABLE_BASE << kind.index()
}

fn king_attack_squares_flag(king_color: Color, kind: KindEffect) -> u64 {
    let shift = king_color.index() * NUM_KIND_EFFECTS + kind.index();
    KING_ATTACK_SQUARES_BASE << shift
}

impl std::fmt::Debug for PositionController {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{:?}",
            PositionAux::new(self.core.clone(), self.stone.into())
        )
    }
}

impl PositionController {
    pub fn new(core: Position, stone: Option<BitBoard>) -> Self {
        let stone = stone.unwrap_or_default();
        let mut stone_digest = 0;
        for pos in stone {
            stone_digest ^= zobrist_stone(pos);
        }

        let mut res = PositionController {
            stone,
            stone_digest,
            black_or_stone: stone,
            occupied: stone,
            pieces: vec![None; 81],
            ..Default::default()
        };

        *res.core.hands_mut() = core.hands();

        for pos in Square::iter() {
            if let Some(p) = core.get(pos) {
                res.set(pos, p);
            }
        }
        res
    }

    pub fn push(&mut self) {
        self.history.push(self.core.clone());
    }

    pub fn pop(&mut self) {
        let core = self.history.pop().unwrap();
        self.set_core(&core);
    }

    pub fn turn(&self) -> Color {
        self.core.turn()
    }

    pub fn get(&self, pos: Square) -> &Option<BoardPiece> {
        &self.pieces[pos.index()]
    }

    pub fn must_get_kind(&self, pos: Square) -> Kind {
        self.get(pos).unwrap().kind()
    }

    pub fn get_kind(&self, pos: Square) -> Option<Kind> {
        self.get(pos).map(|p| p.kind())
    }

    fn set_delta(&mut self, delta: u64) {
        self.delta_update = delta;
    }

    pub fn set(&mut self, pos: Square, p: BoardPiece) {
        debug_assert!(!self.stone.contains(pos), "set: {:?} {:?}", self, pos);
        self.pieces[pos.index()] = Some(p);
        self.occupied.set(pos);
        if p.is_black() {
            self.black_or_stone.set(pos);
        } else {
            self.white.set(pos);
        }
        if p.is_king() {
            self.king[p.color().index()] = pos.into();
        }
        self.kindish_bb[p.kind().ish().index()].set(pos);
        self.core.set(pos, p);

        self.need_update = !0;
        // FIXME
        // self.delta_update(pos, p, true);

        // #[cfg(debug_assertions)]
        // self.dcheck_pinning(pos, p, true);
    }

    pub fn unset(&mut self, pos: Square, p: BoardPiece) {
        debug_assert!(!self.stone.contains(pos), "set: {:?} {:?}", self, pos);
        debug_assert_eq!(
            self.pieces[pos.index()],
            Some(p),
            "unset: {:?} {:?} {:?}",
            self,
            pos,
            p
        );
        self.pieces[pos.index()] = None;
        self.occupied.unset(pos);
        if p.is_black() {
            self.black_or_stone.unset(pos);
        } else {
            self.white.unset(pos);
        }
        if p.is_king() {
            self.king[p.color().index()] = None;
        }
        self.kindish_bb[p.kind().ish().index()].unset(pos);
        self.core.unset(pos, p);

        self.need_update = !0;
        // FIXME
        // self.delta_update(pos, p, false);

        // #[cfg(debug_assertions)]
        // self.dcheck_pinning(pos, p, false);
    }

    // fn can_delta_update(&self, flags: u64) -> bool {
    //     let can_delta_update = self.delta_update & !self.need_update;
    //     can_delta_update & flags == flags
    // }

    // fn delta_update(&mut self, pos: Square, p: BoardPiece, set: bool) {
    //     if p.is_king() {
    //         self.need_update |= KING_ATTACK_SQUARES_BLACK | KING_ATTACK_SQUARES_WHITE;
    //     }
    //     if self.can_delta_update(KING_ATTACK_SQUARES_BLACK) {
    //         self.update_king_attack_squares(pos, p, set, Color::BLACK);
    //     } else {
    //         self.need_update |= KING_ATTACK_SQUARES_BLACK;
    //     }
    //     if self.can_delta_update(KING_ATTACK_SQUARES_WHITE) {
    //         self.update_king_attack_squares(pos, p, set, Color::WHITE);
    //     } else {
    //         self.need_update |= KING_ATTACK_SQUARES_WHITE;
    //     }
    //     if self.can_delta_update(PINNING_BLACK | KING_ATTACK_SQUARES_BLACK) {
    //         self.update_pinning(pos, p, set, Color::BLACK);
    //     } else {
    //         self.need_update |= PINNING_BLACK;
    //     }
    //     if self.can_delta_update(PINNING_WHITE | KING_ATTACK_SQUARES_WHITE) {
    //         self.update_pinning(pos, p, set, Color::WHITE);
    //     } else {
    //         self.need_update |= PINNING_WHITE;
    //     }
    // }

    fn update_king_attack_squares(
        &mut self,
        pos: Square,
        p: BoardPiece,
        set: bool,
        king_color: Color,
    ) {
        if p.is_king() && p.color() == king_color {
            if set {
                self.king_attack_squares[king_color.index()][KindEffect::Pawn.index()] =
                    pawn_power(king_color, pos);
                self.king_attack_squares[king_color.index()][KindEffect::Lance.index()] =
                    lance_reachable(self.occupied, king_color, pos);
                self.king_attack_squares[king_color.index()][KindEffect::Knight.index()] =
                    knight_power(king_color, pos);
                self.king_attack_squares[king_color.index()][KindEffect::Silver.index()] =
                    silver_power(king_color, pos);
                self.king_attack_squares[king_color.index()][KindEffect::Gold.index()] =
                    gold_power(king_color, pos);
                self.king_attack_squares[king_color.index()][KindEffect::Bishop.index()] =
                    bishop_reachable(self.occupied, pos);
                self.king_attack_squares[king_color.index()][KindEffect::Rook.index()] =
                    rook_reachable(self.occupied, pos);
                self.king_attack_squares[king_color.index()][KindEffect::King.index()] =
                    king_power(pos);
                self.king_attack_squares[king_color.index()][KindEffect::ProBishop.index()] =
                    probishop_reachable(self.occupied, pos);
                self.king_attack_squares[king_color.index()][KindEffect::ProRook.index()] =
                    prorook_reachable(self.occupied, pos);
            } else {
                // Do nothing
            }
            return;
        }
        let Some(king) = self.king[king_color.index()] else {
            return;
        };

        for k in [KindEffect::Lance, KindEffect::Bishop, KindEffect::Rook].iter() {
            let bb = self.king_attack_squares[king_color.index()][k.index()];
            if !bb.contains(pos) {
                continue;
            }
            let reach = match k {
                KindEffect::Lance => lance_reachable(self.occupied, king_color, king),
                KindEffect::Bishop => bishop_reachable(self.occupied, king),
                KindEffect::Rook => rook_reachable(self.occupied, king),
                _ => unreachable!(),
            };
            self.king_attack_squares[king_color.index()][k.index()] = reach;
            if k == &KindEffect::Lance {
                continue;
            }
            let pro_reach =
                reach | self.king_attack_squares[king_color.index()][KindEffect::King.index()];
            self.king_attack_squares[king_color.index()][k.promoted().unwrap().index()] = pro_reach;
        }
    }

    fn recompute_king_attack_squares_if_needed(&mut self, king_color: Color, kind: KindEffect) {
        let flag = king_attack_squares_flag(king_color, kind);
        if self.need_update & flag == 0 {
            return;
        }
        self.need_update &= !flag;

        let Some(king_pos) = self.king[king_color.index()] else {
            return;
        };

        self.king_attack_squares[king_color.index()][kind.index()] =
            reachable_cont_sub(self, king_color, king_pos, kind.kinds().next().unwrap());
    }

    pub fn pinning(&mut self, king_color: Color) -> BitBoard {
        self.recompute_pinning_if_needed(king_color);
        self.pinning[king_color.index()]
    }

    fn recompute_pinning_if_needed(&mut self, king_color: Color) {
        let flag = match king_color {
            Color::BLACK => PINNING_BLACK,
            Color::WHITE => PINNING_WHITE,
        };
        if self.need_update & flag == 0 {
            return;
        }
        self.need_update &= !flag;

        let attacker_color = king_color.opposite();
        let Some(king_pos) = self.king[king_color.index()] else {
            return;
        };
        let q = self.king_attack_squares(king_color, KindEffect::Bishop)
            | self.king_attack_squares(king_color, KindEffect::Rook);
        let other_occupied = self.occupied.and_not(q);
        let pinning_cands = (bishop_reachable(other_occupied, king_pos)
            | rook_reachable(other_occupied, king_pos))
            & self.color_bb(attacker_color)
            & other_occupied;
        self.pinning[king_color.index()].clear();
        for pos in pinning_cands {
            let Some(p) = self.get(pos) else {
                continue;
            };
            if p.color() != attacker_color {
                continue;
            }
            if p.is_slider() && attackable_sliders(king_pos, pos, attacker_color).contains(p.kind())
            {
                self.pinning[king_color.index()].set(pos);
            }
        }
    }

    #[allow(dead_code)]
    fn update_pinning(&mut self, pos: Square, p: BoardPiece, set: bool, king_color: Color) {
        let attacker_color = king_color.opposite();

        if p.is_king() && p.color() == king_color {
            if set {
                self.pinning[king_color.index()].clear();

                let black_or_white = self.black_bb() | self.white;
                let queen_pinned_cands = (self.king_attack_squares(king_color, KindEffect::Bishop)
                    | self.king_attack_squares(king_color, KindEffect::Rook))
                    & black_or_white;
                let other_occupied = self.occupied.and_not(queen_pinned_cands);
                let attacker_cands = (bishop_reachable(other_occupied, pos)
                    | rook_reachable(other_occupied, pos))
                    & other_occupied
                    & self.color_bb(attacker_color);

                for attacker_pos in attacker_cands {
                    let p = self.get(attacker_pos).unwrap();
                    if p.is_slider()
                        && attackable_sliders(pos, attacker_pos, attacker_color).contains(p.kind())
                    {
                        self.pinning[king_color.index()].set(attacker_pos);
                    }
                }
            } else {
                // Do nothing
            }
            return;
        }
        let Some(king) = self.king[king_color.index()] else {
            return;
        };
        let (dx, dy) = (
            (pos.col() as i32 - king.col() as i32),
            (pos.row() as i32 - king.row() as i32),
        );
        if dx != 0 && dy != 0 && dx.abs() != dy.abs() {
            return;
        }
        let black_or_white = self.black_bb() | self.white;
        let outside = outside(king, pos);

        let attackable_sliders = attackable_sliders(king, pos, attacker_color);
        let test_kind = if attackable_sliders.contains(Kind::Bishop) {
            KindEffect::Bishop
        } else {
            KindEffect::Rook
        };

        if set {
            // p interferes with pinning
            if let Some(affected) = (outside & self.pinning[king_color.index()]).next() {
                self.pinning[king_color.index()].unset(affected);
            }

            // p becomes pinned or attacker
            let in_between = between(king, pos) & self.occupied;
            let count = in_between.count_ones();
            if count == 0 {
                let other_occupied = self.occupied.without(pos);
                if let Some(pinning_cand) = (match test_kind {
                    KindEffect::Bishop => bishop_reachable(other_occupied, king),
                    KindEffect::Rook => rook_reachable(other_occupied, king),
                    _ => unreachable!(),
                } & outside
                    & self.color_bb(attacker_color))
                .next()
                {
                    let k = self.must_get_kind(pinning_cand);
                    if attackable_sliders.contains(k) {
                        self.pinning[king_color.index()].set(pinning_cand);
                    }
                }
            } else if count == 1 {
                if p.color() == attacker_color
                    && attackable_sliders.contains(p.kind())
                    && !(in_between & black_or_white).is_empty()
                {
                    self.pinning[king_color.index()].set(pos);
                }
            }
        } else {
            // p was attacker
            if self.pinning[king_color.index()].contains(pos) {
                self.pinning[king_color.index()].unset(pos);
            }
            // p was pinned
            if let Some(pinning) = (outside & self.pinning[king_color.index()]).next() {
                debug_assert!(self.pinning[king_color.index()].contains(pinning));
                debug_assert_eq!((outside & self.pinning[king_color.index()]).count_ones(), 1);

                self.pinning[king_color.index()].unset(pinning);
            }

            let attack_squares = self.king_attack_squares(king_color, test_kind);

            // p was in pinned position or attacker position
            let in_between = between(king, pos) & self.occupied;
            if in_between.count_ones() > 1 {
                return;
            }
            let other_occupied = self.occupied.and_not(attack_squares);
            let next_reachable = match test_kind {
                KindEffect::Bishop => bishop_reachable(other_occupied, king),
                KindEffect::Rook => rook_reachable(other_occupied, king),
                _ => unreachable!(),
            };
            let Some(pinning_cand) = (outside & next_reachable & other_occupied).next() else {
                return;
            };
            let Some(p) = self.get(pinning_cand) else {
                return;
            };
            if p.color() == attacker_color && attackable_sliders.contains(p.kind()) {
                self.pinning[king_color.index()].set(pinning_cand);
            }
        }
    }

    #[allow(dead_code)]
    fn dcheck_pinning(&self, pos: Square, p: BoardPiece, set: bool) {
        for king_color in Color::iter() {
            let attacker_color = king_color.opposite();
            let Some(king_pos) = self.king[king_color.index()] else {
                continue;
            };
            let q = self.king_attack_squares[king_color.index()][KindEffect::Bishop.index()]
                | self.king_attack_squares[king_color.index()][KindEffect::Rook.index()];
            let other_occupied = self.occupied.and_not(q);
            let pinning_cands = (bishop_reachable(other_occupied, king_pos)
                | rook_reachable(other_occupied, king_pos))
                & self.color_bb(attacker_color)
                & other_occupied;
            let mut pinning = BitBoard::default();
            for pos in pinning_cands {
                let Some(p) = self.get(pos) else {
                    continue;
                };
                if p.color() != attacker_color {
                    continue;
                }
                if p.is_slider()
                    && attackable_sliders(king_pos, pos, attacker_color).contains(p.kind())
                {
                    pinning.set(pos);
                }
            }
            debug_assert_eq!(
                pinning,
                self.pinning[king_color.index()],
                "{:?} {:?} {:?} {}",
                self,
                pos,
                p,
                set
            );
        }
    }

    pub fn white_king_empty_or_white_attack_squares(&mut self, kind: KindEffect) -> BitBoard {
        self.king_attack_squares(Color::WHITE, kind)
            .and_not(self.black_or_stone)
    }

    pub fn hands_mut(&mut self) -> &mut Hands {
        self.core.hands_mut()
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
                debug_assert!(
                    capture_kind_hint.is_none()
                        || capture_kind_hint.unwrap() == self.get_kind(*dest)
                );
                debug_assert!(
                    source_kind_hint.is_none()
                        || source_kind_hint.unwrap() == self.must_get_kind(*source)
                );

                let source_kind = source_kind_hint.unwrap_or_else(|| self.must_get_kind(*source));
                let capture_kind = capture_kind_hint.unwrap_or_else(|| self.get_kind(*dest));
                let dest_kind = if *promote {
                    source_kind.promote().unwrap()
                } else {
                    source_kind
                };

                self.set_delta(0);

                if let Some(capture_kind) = capture_kind {
                    self.unset(*dest, (turn.opposite(), capture_kind).into());
                    self.hands_mut().add(turn, capture_kind.maybe_unpromote());
                }
                self.unset(*source, (turn, source_kind).into());
                self.set(*dest, (turn, dest_kind).into());

                self.core.set_pawn_drop(false);
                self.core.set_turn(turn.opposite());
            }
            Movement::Drop(pos, kind) => {
                self.set_delta(0);

                self.set(*pos, (turn, *kind).into());
                self.hands_mut().remove(turn, *kind);

                self.core.set_pawn_drop(*kind == Kind::Pawn);
                self.core.set_turn(turn.opposite());
            }
        }
    }

    pub(crate) fn pawn_drop(&self) -> bool {
        self.core.hands().pawn_drop()
    }

    pub(crate) fn occupied_bb(&self) -> BitBoard {
        self.occupied
    }

    pub(crate) fn must_king_pos(&self, c: Color) -> Square {
        self.king[c.index()].unwrap()
    }

    pub fn king_pos(&self, c: Color) -> Option<Square> {
        self.king[c.index()]
    }

    pub(crate) fn capturable_by(&self, c: Color) -> BitBoard {
        match c {
            Color::BLACK => self.white,
            Color::WHITE => self.core.black(),
        }
    }

    pub(crate) fn must_turn_king_pos(&self) -> Square {
        self.must_king_pos(self.turn())
    }

    pub(crate) fn color_bb_and_stone(&self, c: Color) -> BitBoard {
        match c {
            Color::BLACK => self.black_or_stone,
            Color::WHITE => self.white | self.stone,
        }
    }

    pub(crate) fn hands(&self) -> Hands {
        self.core.hands()
    }

    pub(crate) fn bishopish(&self) -> BitBoard {
        self.core.kind_bb().bishopish()
    }

    pub(crate) fn rookish(&self) -> BitBoard {
        self.core.kind_bb().rookish()
    }

    pub(crate) fn bitboard(&self, c: Color, k: Kind) -> BitBoard {
        self.core.bitboard(c, k)
    }

    pub(crate) fn pawn_mask(&self, c: Color) -> usize {
        self.core.bitboard(c, Kind::Pawn).col_mask()
    }

    pub(crate) fn moved_digest(&self, movement: &Movement) -> u64 {
        self.core.moved_digest(&movement) ^ self.stone_digest
    }

    pub(crate) fn black_king_pos(&self) -> Option<Square> {
        self.king[Color::BLACK.index()]
    }

    pub(crate) fn white_king_pos(&self) -> Square {
        self.king[Color::WHITE.index()].unwrap()
    }

    pub(crate) fn pawn_silver_goldish(&self) -> BitBoard {
        self.core.kind_bb().pawn_silver_goldish()
    }

    pub(crate) fn black_bb(&self) -> BitBoard {
        self.core.black()
    }

    pub(crate) fn set_core(&mut self, core: &Position) {
        *self.core.hands_mut() = core.hands();

        let (remove, add) = self.diff(core);

        self.set_delta(0);

        for pos in remove {
            self.unset(pos, self.get(pos).unwrap());
        }
        for pos in add {
            self.set(pos, core.get(pos).unwrap());
        }
    }

    pub(crate) fn core(&self) -> &Position {
        &self.core
    }

    pub(crate) fn color_bb(&self, c: Color) -> BitBoard {
        match c {
            Color::BLACK => self.core.black(),
            Color::WHITE => self.white,
        }
    }

    fn diff(&self, other: &Position) -> (/* remove */ BitBoard, /* add */ BitBoard) {
        let kind_bb = self.core.kind_bb();
        let other_kind_bb = other.kind_bb();
        let other_occupied = other_kind_bb.occupied();
        let remove = self.occupied.and_not(other_occupied);
        let add = other_occupied.and_not(self.occupied);

        let mut changed = self.black_bb() ^ other.black();
        changed |= kind_bb.promote ^ other_kind_bb.promote;
        changed |= kind_bb.kind0 ^ other_kind_bb.kind0;
        changed |= kind_bb.kind1 ^ other_kind_bb.kind1;
        changed |= kind_bb.kind2 ^ other_kind_bb.kind2;

        (changed.and_not(add), changed.and_not(remove))
    }

    pub(crate) fn white_king_attack_empty_squares(&mut self, effect: KindEffect) -> BitBoard {
        self.king_attack_squares(Color::WHITE, effect)
            .and_not(self.occupied)
    }

    pub(crate) fn king_attack_squares(
        &mut self,
        king_color: Color,
        effect: KindEffect,
    ) -> BitBoard {
        self.recompute_king_attack_squares_if_needed(king_color, effect);
        self.king_attack_squares[king_color.index()][effect.index()]
    }

    pub fn kindish_bb(&self, k: Kindish) -> BitBoard {
        self.kindish_bb[k.index()]
    }

    pub fn bitboard2(&mut self, c: Color, k: Kindish) -> BitBoard {
        self.kindish_bb(k) & self.color_bb(c)
    }

    fn recompute_attackable_if_needed(&mut self, kind: Kindish) {
        let flag = attackable_flag(kind);
        if self.need_update & flag == 0 {
            return;
        }
        self.need_update &= !flag;

        self.attackable[kind.index()] = BitBoard::default();

        let mut attacker_cands = self.bitboard2(Color::BLACK, kind);
        if attacker_cands.is_empty() {
            return;
        }

        let raw_attack_squares = self.white_king_empty_or_white_attack_squares(kind.effect());
        let pro_attack_squares =
            self.white_king_empty_or_white_attack_squares(kind.promoted().effect());

        let attack_squares = raw_attack_squares | pro_attack_squares;

        if matches!(kind, Kindish::Bishop | Kindish::Rook | Kindish::King)
            || attacker_cands.count_ones() <= attack_squares.count_ones()
        {
            for attacker_pos in attacker_cands {
                let k = self.must_get_kind(attacker_pos);
                let reach = reachable_cont_sub(self, Color::BLACK, attacker_pos, k);
                if !(reach & raw_attack_squares).is_empty() {
                    self.attackable[kind.index()].set(attacker_pos);
                    continue;
                }
                if matches!(k, Kind::ProBishop | Kind::ProRook) {
                    if !(reach & pro_attack_squares).is_empty() {
                        self.attackable[kind.index()].set(attacker_pos);
                    }
                } else if k.can_promote() {
                    let mut dest_cands = reach & pro_attack_squares;
                    if !BitBoard::BLACK_PROMOTABLE.contains(attacker_pos) {
                        dest_cands &= BitBoard::BLACK_PROMOTABLE;
                    }
                    if !dest_cands.is_empty() {
                        self.attackable[kind.index()].set(attacker_pos);
                    }
                }
            }
        } else {
            for dest in attack_squares {
                let mut cands =
                    reachable_cont_sub(self, Color::WHITE, dest, kind.weakest()) & attacker_cands;
                if cands.is_empty() {
                    continue;
                }

                let should_promote = !raw_attack_squares.contains(dest);
                if should_promote && !BitBoard::BLACK_PROMOTABLE.contains(dest) {
                    cands &= BitBoard::BLACK_PROMOTABLE;
                }
                if cands.is_empty() {
                    continue;
                }

                self.attackable[kind.index()] |= cands;

                attacker_cands.and_not_assign(cands);
                if attacker_cands.is_empty() {
                    break;
                }
            }
        }
    }

    pub(crate) fn attackable(&mut self, kind: Kindish) -> BitBoard {
        self.recompute_attackable_if_needed(kind);
        self.attackable[kind.index()]
    }
}

fn attackable_sliders(king: Square, attacker: Square, attacker_color: Color) -> Kinds {
    let (dx, dy) = (
        (king.col() as i32 - attacker.col() as i32),
        (king.row() as i32 - attacker.row() as i32),
    );
    let mut kinds = Kinds::default();
    if dx != 0 && dy != 0 {
        if dx.abs() == dy.abs() {
            kinds.set(Kind::Bishop);
            kinds.set(Kind::ProBishop);
        }
        return kinds;
    }

    kinds.set(Kind::Rook);
    kinds.set(Kind::ProRook);
    if dy < 0 && attacker_color.is_black() || dy > 0 && attacker_color.is_white() {
        kinds.set(Kind::Lance);
    }

    kinds
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pinning() {
        for (sfen, want) in [
            (
                "3+R1l1+B1/5l3/3N1l3/9/3ksR3/9/3S1K3/3G2B2/3L5 b G2g2s3n18p 1",
                vec![
                    (
                        Square::new(5, 0),
                        BoardPiece::new(Color::WHITE, Kind::ProRook),
                    ),
                    (
                        Square::new(1, 0),
                        BoardPiece::new(Color::WHITE, Kind::ProBishop),
                    ),
                    (
                        Square::new(3, 2),
                        BoardPiece::new(Color::BLACK, Kind::Lance),
                    ),
                    (Square::new(3, 4), BoardPiece::new(Color::WHITE, Kind::Rook)),
                    (
                        Square::new(2, 7),
                        BoardPiece::new(Color::WHITE, Kind::Bishop),
                    ),
                ],
            ),
            (
                "5L3/2B2G3/3K1S3/9/3Rsk3/9/3l1N3/3l5/1+B1l1+R3 b 2G2S3N18Pg 1",
                vec![
                    (
                        Square::new(3, 8),
                        BoardPiece::new(Color::WHITE, Kind::ProRook),
                    ),
                    (
                        Square::new(7, 8),
                        BoardPiece::new(Color::WHITE, Kind::ProBishop),
                    ),
                    (Square::new(5, 4), BoardPiece::new(Color::WHITE, Kind::Rook)),
                    (
                        Square::new(6, 1),
                        BoardPiece::new(Color::WHITE, Kind::Bishop),
                    ),
                ],
            ),
        ] {
            let position = PositionAux::from_sfen(sfen).unwrap();
            let mut controller =
                PositionController::new(position.core().clone(), *position.stone());

            let mut want_bb = [BitBoard::default(); 2];
            for (pos, p) in want {
                want_bb[p.color().index()].set(pos);
            }

            let got = [
                controller.pinning(Color::BLACK),
                controller.pinning(Color::WHITE),
            ];

            assert_eq!(got, want_bb, "{:?}", position);
        }
    }
}
