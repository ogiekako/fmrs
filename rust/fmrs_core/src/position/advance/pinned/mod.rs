pub mod generated_magics;
pub mod magics_generator;

use crate::{
    piece::{Color, Kind},
    position::{
        bitboard::{
            bishop_power, bishop_reachable, lance_power, reachable, rook_power, rook_reachable,
            BitBoard,
        },
        position::PositionAux,
        Square,
    },
};

// pinned piece and its movable positions (capturing included) pairs.
#[derive(Debug, Default)]
pub struct Pinned {
    pinned_bb: BitBoard,
    pinned_area: Vec<(Square, BitBoard)>,
}

impl Pinned {
    fn push(&mut self, pos: Square, area: BitBoard) {
        self.pinned_bb.set(pos);
        self.pinned_area.push((pos, area));
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
        if !self.pinned_bb.contains(source) {
            return None;
        }
        for &(pinned_pos, movable) in self.pinned_area.iter() {
            if source == pinned_pos {
                return movable.into();
            }
        }
        unreachable!()
    }
}

pub fn pinned(position: &mut PositionAux, king_color: Color, blocker_color: Color) -> Pinned {
    let Some(king_pos) = position.king_pos(king_color) else {
        return Pinned::default();
    };
    let mut res = Pinned::default();

    lance_pinned(position, king_color, blocker_color, &mut res);
    bishop_pinned(position, king_color, blocker_color, king_pos, &mut res);
    rook_pinned(position, king_color, blocker_color, king_pos, &mut res);

    res
}

// #[inline(never)]
fn bishop_pinned(
    position: &mut PositionAux,
    king_color: Color,
    blocker_color: Color,
    king_pos: Square,
    res: &mut Pinned,
) {
    let attacker_color = king_color.opposite();

    let power_from_king = bishop_power(king_pos);
    let mut potential_attackers =
        position.bishopish() & position.color_bb(attacker_color) & power_from_king;
    if potential_attackers.is_empty() {
        return;
    }

    let reachable_from_king = bishop_reachable(position.occupied_bb(), king_pos);

    potential_attackers = potential_attackers.and_not(reachable_from_king);

    for attacker_pos in potential_attackers {
        let power_from_attacker = bishop_power(attacker_pos);

        let block = reachable_from_king
            & bishop_reachable(position.occupied_bb(), attacker_pos)
            & position.color_bb(blocker_color);
        if block.is_empty() {
            continue;
        }
        let blocker_pos = block.singleton();

        let blocker_kind = position.must_get_kind(blocker_pos);
        let reach = reachable(position, blocker_color, blocker_pos, blocker_kind, false)
            & (power_from_attacker & power_from_king | BitBoard::EMPTY.with(attacker_pos));

        res.push(blocker_pos, reach);
    }
}

// #[inline(never)]
fn rook_pinned(
    position: &mut PositionAux,
    king_color: Color,
    blocker_color: Color,
    king_pos: Square,
    res: &mut Pinned,
) {
    let attacker_color = king_color.opposite();

    let power_from_king = rook_power(king_pos);
    let mut potential_attackers =
        position.rookish() & position.color_bb(attacker_color) & power_from_king;
    if potential_attackers.is_empty() {
        return;
    }

    let reachable_from_king = rook_reachable(position.occupied_bb(), king_pos);

    potential_attackers = potential_attackers.and_not(reachable_from_king);

    for attacker_pos in potential_attackers {
        let power_from_attacker = rook_power(attacker_pos);

        let block = reachable_from_king
            & rook_reachable(position.occupied_bb(), attacker_pos)
            & position.color_bb(blocker_color);
        if block.is_empty() {
            continue;
        }
        let blocker_pos = block.singleton();

        let blocker_kind = position.must_get_kind(blocker_pos);
        let reach = reachable(position, blocker_color, blocker_pos, blocker_kind, false)
            & (power_from_attacker & power_from_king | BitBoard::EMPTY.with(attacker_pos));

        res.push(blocker_pos, reach);
    }
}

// #[inline(never)]
fn lance_pinned(
    position: &mut PositionAux,
    king_color: Color,
    blocker_color: Color,
    res: &mut Pinned,
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
        res.push(blocker_pos, reach);
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
        res.push(blocker_pos, reach);
    }
}
