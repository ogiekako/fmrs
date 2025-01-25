pub mod generated_magics;
pub mod magics_generator;

use generated_magics::{bishop_pinning, rook_pinning};

use crate::{
    piece::{Color, Kind},
    position::{
        bitboard::{lance_power, power, reachable, BitBoard},
        position::PositionAux,
        Square,
    },
};

// pinned piece and its movable positions (capturing included) pairs.
#[derive(Debug, Default)]
pub struct Pinned {
    pinned_area: Vec<(Square, BitBoard)>,
}

impl Pinned {
    fn new(pinned_area: Vec<(Square, BitBoard)>) -> Self {
        Self { pinned_area }
    }
    pub fn iter(&self) -> impl Iterator<Item = &(Square, BitBoard)> {
        self.pinned_area.iter()
    }
    pub fn is_unpin_move(&self, source: Square, dest: Square) -> bool {
        self.pinned_area(source)
            .map(|area| !area.contains(dest))
            .unwrap_or(false)
    }
    // Reachable pinned area including capturing move
    pub fn pinned_area(&self, source: Square) -> Option<BitBoard> {
        for &(pinned_pos, movable) in self.pinned_area.iter() {
            if source == pinned_pos {
                return movable.into();
            }
        }
        None
    }
}

pub fn pinned(position: &mut PositionAux, king_color: Color, blocker_color: Color) -> Pinned {
    let Some(king_pos) = position.king_pos(king_color) else {
        return Pinned::default();
    };
    let mut res = vec![];

    let occupied = position.occupied_bb();
    let attacker_color = king_color.opposite();
    let attacker_bb = position.color_bb(attacker_color);
    let blocker_bb = if blocker_color == attacker_color {
        attacker_bb
    } else {
        position.color_bb(blocker_color)
    };

    let bishopish_attackers = position.bishopish() & attacker_bb;
    let rookish_attackers = position.rookish() & attacker_bb;

    let bishop_pinning = if bishopish_attackers.is_empty() {
        BitBoard::EMPTY
    } else {
        bishop_pinning(occupied, king_pos) & bishopish_attackers
    };

    let rook_pinning = if rookish_attackers.is_empty() {
        BitBoard::EMPTY
    } else {
        rook_pinning(occupied, king_pos) & rookish_attackers
    };

    for pinning in [bishop_pinning, rook_pinning] {
        for attacker in pinning {
            let mut area = BitBoard::between(king_pos, attacker);
            let Some(blocker) = (area & blocker_bb).next() else {
                continue;
            };
            area.unset(blocker);
            if attacker_color != blocker_color {
                area.set(attacker);
            }
            let kind = position.must_get_kind(blocker);
            area &= power(blocker_color, blocker, kind);
            res.push((blocker, area));
        }
    }

    lance_pinned(position, king_color, blocker_color, &mut res);

    Pinned::new(res)
}

// #[inline(never)]
fn lance_pinned(
    position: &mut PositionAux,
    king_color: Color,
    blocker_color: Color,
    res: &mut Vec<(Square, BitBoard)>,
) {
    let attacker_color = king_color.opposite();

    let lances = position.bitboard(attacker_color, Kind::Lance);
    if lances.is_empty() {
        return;
    }

    let power_from_king = lance_power(king_color, position.must_king_pos(king_color));
    let potential_attackers = lances & power_from_king;
    if potential_attackers.is_empty() {
        return;
    }

    let mut occupied = position.occupied_bb() & power_from_king;

    if king_color.is_white() {
        let blocker_pos = occupied.next().unwrap();
        if !position.color_bb(blocker_color).contains(blocker_pos) {
            return;
        }
        let Some(attacker_pos) = occupied.next() else {
            return;
        };
        if !lances.contains(attacker_pos) {
            return;
        }
        let blocker_kind = position.must_get_kind(blocker_pos);
        let reach =
            reachable(position, blocker_color, blocker_pos, blocker_kind, false) & power_from_king;
        res.push((blocker_pos, reach));
    } else {
        let mut occupied = occupied.u128();
        let blocker_pos = Square::from_index(127 - occupied.leading_zeros() as usize);
        if !position.color_bb(blocker_color).contains(blocker_pos) {
            return;
        }
        occupied &= !(1 << blocker_pos.index());
        if occupied == 0 {
            return;
        }
        let attacker_pos = Square::from_index(127 - occupied.leading_zeros() as usize);
        if !lances.contains(attacker_pos) {
            return;
        }
        let blocker_kind = position.must_get_kind(blocker_pos);
        let reach =
            reachable(position, blocker_color, blocker_pos, blocker_kind, false) & power_from_king;
        res.push((blocker_pos, reach));
    }
}
