use crate::{
    piece::{Color, EssentialKind, Kind},
    position::{
        bitboard::{self, BitBoard},
        Position, Square,
    },
};

use super::state_info::StateInfo;

// pinned piece and its movable positions (capturing included) pairs.
#[derive(Debug)]
pub(super) struct Pinned {
    mask: BitBoard,
    pinned_area: Vec<(Square, BitBoard)>,
}

impl Pinned {
    pub fn empty() -> Self {
        Self {
            mask: BitBoard::default(),
            pinned_area: vec![],
        }
    }
    fn new(pinned_areaa: Vec<(Square, BitBoard)>) -> Self {
        let mut mask = BitBoard::default();
        pinned_areaa.iter().for_each(|(x, _)| mask.set(*x));
        Self {
            mask,
            pinned_area: pinned_areaa,
        }
    }
    pub fn pinned_mask(&self) -> BitBoard {
        self.mask
    }
    pub fn is_pinned(&self, pos: Square) -> bool {
        self.mask.get(pos)
    }
    pub fn iter(&self) -> impl Iterator<Item = &(Square, BitBoard)> {
        self.pinned_area.iter()
    }
    pub fn is_unpin_move(&self, source: Square, dest: Square) -> bool {
        self.is_pinned(source) && !self.pinned_area(source).get(dest)
    }
    // Reachable pinned area including capturing move
    // #[inline(never)]
    pub fn pinned_area(&self, source: Square) -> BitBoard {
        for (pinned_pos, movable) in self.pinned_area.iter() {
            if source == *pinned_pos {
                return *movable;
            }
        }
        panic!("BUG: is_pinned(source) should be true");
    }
}

// #[inline(never)]
pub(super) fn pinned(
    state: &StateInfo,
    king_color: Color,
    king_pos: Square,
    blocker_color: Color,
) -> Pinned {
    let mut res = vec![];

    let attacker_color = king_color.opposite();

    let color_bb = state.position.color_bb();
    let kind_bb = state.position.kind_bb();
    let attacker_color_bb = *color_bb.bitboard(attacker_color);

    for attacker_kind in [Kind::Lance, Kind::Bishop, Kind::Rook] {
        let mask = bitboard::power(king_color, king_pos, attacker_kind.to_essential_kind())
            & attacker_color_bb;
        if mask.is_empty() {
            continue;
        }

        let attackers = if attacker_kind == Kind::Lance {
            *kind_bb.bitboard(attacker_kind)
        } else {
            kind_bb.bitboard(attacker_kind) | kind_bb.bitboard(attacker_kind.promote().unwrap())
        } & mask;

        if attackers.is_empty() {
            continue;
        }
        let king_seeing = bitboard::reachable(
            color_bb,
            king_color,
            king_pos,
            attacker_kind.to_essential_kind(),
            king_color == blocker_color,
        );
        if king_seeing.is_empty() {
            continue;
        }

        for attacker_pos in attackers {
            let attacker_within_reach = bitboard::reachable(
                color_bb,
                attacker_color,
                attacker_pos,
                attacker_kind.to_essential_kind(),
                king_color != blocker_color,
            );
            if attacker_within_reach.get(king_pos) {
                continue;
            }
            let pinned_pos = {
                let mut pinned = king_seeing & attacker_within_reach;
                if pinned.is_empty() {
                    continue;
                }
                pinned.next().unwrap()
            };
            let pinned_kind = state.get(pinned_pos).unwrap().1;
            let pinned_reachable = bitboard::reachable(
                color_bb,
                blocker_color,
                pinned_pos,
                pinned_kind.to_essential_kind(),
                false,
            );
            let mut same_line =
                bitboard::power(king_color, king_pos, attacker_kind.to_essential_kind())
                    & bitboard::power(
                        attacker_color,
                        attacker_pos,
                        attacker_kind.to_essential_kind(),
                    );
            same_line.set(attacker_pos);
            res.push((pinned_pos, pinned_reachable & same_line))
        }
    }
    Pinned::new(res)
}
