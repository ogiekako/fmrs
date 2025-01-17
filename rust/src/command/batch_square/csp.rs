use fmrs_core::{
    piece::{Color, Kind, KindEffects, Kinds, NUM_KIND},
    position::{
        bitboard::{king_power, reachable, reachable_core, reachable_sub},
        position::PositionAux,
        BitBoard, Square,
    },
};

pub struct Constraint {
    pub uncaptureable_black: BitBoard,
    pub uncheckable: BitBoard,
    pub can_use_king: bool,
}

impl Constraint {
    pub fn search(&self) -> Option<PositionAux> {
        let mut state = State::new(
            self.uncaptureable_black,
            self.uncheckable,
            self.can_use_king,
        );
        if !state.search() {
            return None;
        }
        Some(state.position)
    }
}

struct State {
    uncheckable: BitBoard,
    can_use_king: bool,
    disallowed: [BitBoard; NUM_KIND],
    // mutable
    need_check: BitBoard,
    need_place: BitBoard,
    position: PositionAux,
}

impl State {
    fn new(uncapturable_black: BitBoard, uncheckable: BitBoard, can_use_king: bool) -> Self {
        let mut disallowed = [BitBoard::EMPTY; NUM_KIND];
        let position = PositionAux::default();
        for pos in Square::iter() {
            for kind in Kind::iter() {
                if !can_use_king && kind == Kind::King {
                    disallowed[kind.index()].set(pos);
                    continue;
                }
                let reach = reachable_core(uncapturable_black, Color::BLACK, pos, kind);
                if !(reach & uncheckable).is_empty() {
                    disallowed[kind.index()].set(pos);
                }
            }
        }

        let need_check = uncapturable_black;
        let need_place = uncapturable_black;

        Self {
            uncheckable,
            disallowed,
            can_use_king,
            need_check,
            need_place,
            position,
        }
    }

    fn satisfied(&self) -> bool {
        self.need_check.is_empty() && self.need_place.is_empty()
    }

    fn search(&mut self) -> bool {
        if self.satisfied() {
            return true;
        }
        let mut next_pos = (u32::MAX, /* check or place */ false, Square::new(0, 0));

        for pos in self.need_place {
            let cands = self.placeable_kinds(pos).count_ones();
            if cands == 0 {
                return false;
            }
            if cands < next_pos.0 {
                next_pos = (cands, false, pos);
            }
        }
        for pos in self.need_check {
            let cands = self.checkable_kind_pos(pos).count() as u32;
            if cands == 0 {
                return false;
            }
            if cands < next_pos.0 {
                next_pos = (cands, true, pos);
            }
        }

        if next_pos.1 {
            // check
            let checked_pos = next_pos.2;
            let kind_pos = self.checkable_kind_pos(checked_pos).collect::<Vec<_>>();
            for (kind, pos) in kind_pos {
                if self.search_sub(pos, kind) {
                    return true;
                }
            }
        } else {
            let place_pos = next_pos.2;
            for kind in self.placeable_kinds(place_pos) {
                if self.search_sub(place_pos, kind) {
                    return true;
                }
            }
        }

        false
    }

    fn search_sub(&mut self, pos: Square, kind: Kind) -> bool {
        self.position.set(pos, (Color::BLACK, kind).into());

        let prev_need_check = self.need_check;
        let prev_need_place = self.need_place;

        let reach = reachable_sub(&mut self.position, Color::BLACK, pos, kind);
        self.need_check = self.need_check.and_not(reach);
        self.need_place.unset(pos);

        if self.search() {
            return true;
        }

        self.need_check = prev_need_check;
        self.need_place = prev_need_place;

        self.position.unset(pos, (Color::BLACK, kind).into());

        false
    }

    fn checker_pos_cands(&mut self, checked_pos: Square, kind: Kind) -> BitBoard {
        let placeable = reachable(&mut self.position, Color::WHITE, checked_pos, kind, true)
            .and_not(self.uncheckable | self.disallowed[kind.index()])
            & self.position.settable_bb((Color::BLACK, kind).into());
        placeable & king_power(checked_pos)
    }

    fn placeable_kinds(&mut self, pos: Square) -> Kinds {
        let mut kinds = Kinds::default();
        let mut effects = KindEffects::default();
        for k in Kind::iter() {
            if !self.can_use_king && k == Kind::King {
                continue;
            }
            if effects.contains(k.effect()) {
                continue;
            }
            if self.disallowed[k.index()].contains(pos) {
                continue;
            }
            if self.position.can_set(pos, (Color::BLACK, k).into()).is_ok() {
                effects.set(k.effect());
                kinds.set(k);
            }
        }
        kinds
    }

    fn checkable_kind_pos(&mut self, pos: Square) -> impl Iterator<Item = (Kind, Square)> + '_ {
        let mut kinds = KindEffects::default();
        Kind::iter()
            .filter_map(move |k| {
                if !self.can_use_king && k == Kind::King {
                    return None;
                }
                if kinds.contains(k.effect()) {
                    return None;
                }
                let cands = self.checker_pos_cands(pos, k);
                if !cands.is_empty() {
                    kinds.set(k.effect());
                }
                Some(cands.map(move |pos| (k, pos)))
            })
            .flatten()
    }
}

#[cfg(test)]
mod tests {
    use fmrs_core::{
        bitboard,
        piece::Color,
        position::{bitboard::reachable_sub, BitBoard},
    };

    use crate::command::batch_square::csp::Constraint;

    #[test]
    fn test_csp_search() {
        for (name, stone, room, want_success) in vec![
            (
                "1x1",
                bitboard!(
                    ".........",
                    ".........",
                    ".........",
                    ".........",
                    ".........",
                    ".........",
                    ".........",
                    ".......**",
                    ".......*.",
                ),
                bitboard!(
                    ".........",
                    ".........",
                    ".........",
                    ".........",
                    ".........",
                    ".........",
                    ".........",
                    ".........",
                    "........*",
                ),
                true,
            ),
            (
                "rook and knight",
                bitboard!(
                    ".........",
                    ".........",
                    ".........",
                    ".........",
                    ".........",
                    ".........",
                    "....*****",
                    "....*....",
                    "....**...",
                ),
                bitboard!(
                    ".........",
                    ".........",
                    ".........",
                    ".........",
                    ".........",
                    ".........",
                    ".........",
                    ".....****",
                    "......***",
                ),
                true,
            ),
            (
                "impossible",
                bitboard!(
                    ".........",
                    ".........",
                    ".........",
                    ".........",
                    ".........",
                    ".........",
                    ".....****",
                    ".....*...",
                    ".....***.",
                ),
                bitboard!(
                    ".........",
                    ".........",
                    ".........",
                    ".........",
                    ".........",
                    ".........",
                    ".........",
                    "......***",
                    "........*",
                ),
                false,
            ),
            (
                "bishop and knight",
                bitboard!(
                    ".........",
                    ".........",
                    ".........",
                    ".........",
                    ".........",
                    "..***....",
                    "..*.*****",
                    "..**.*.*.",
                    "..*.*.*.*",
                ),
                bitboard!(
                    ".........",
                    ".........",
                    ".........",
                    ".........",
                    ".........",
                    ".........",
                    "...*.....",
                    "....*.*.*",
                    "...*.*.*.",
                ),
                true,
            ),
        ] {
            let constraint = Constraint {
                uncaptureable_black: stone,
                uncheckable: room,
                can_use_king: false,
            };
            let position = constraint.search();
            assert_eq!(position.is_some(), want_success);

            let Some(mut p) = position else {
                continue;
            };

            assert_eq!(p.white_bb(), BitBoard::EMPTY, "{} {:?}", name, p);
            assert_eq!(p.black_bb() & stone, stone, "{} {:?}", name, p);
            assert_eq!(p.black_bb() & room, BitBoard::EMPTY, "{} {:?}", name, p);
            let mut checked = BitBoard::EMPTY;
            for pos in p.black_bb() {
                let kind = p.get(pos).unwrap().1;
                checked |= reachable_sub(&mut p, Color::BLACK, pos, kind);
            }
            assert_eq!(checked & room, BitBoard::EMPTY, "{} {:?}", name, p);
            assert_eq!(checked & stone, stone, "{} {:?}", name, p);
        }
    }
}
