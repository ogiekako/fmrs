use crate::{
    piece::{Color, KIND_LANCE},
    position::{
        bitboard::{bishop_power, lance_power, magic, reachable, rook_power, BitBoard},
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
            .map(|area| !area.get(dest))
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

pub fn pinned<const THEM: bool, const BLOCKER: bool>(
    position: &mut PositionAux,
    king_color: Color,
    blocker_color: Color,
) -> Pinned {
    let mut res = vec![];

    lance_pinned::<BLOCKER>(position, king_color, blocker_color, &mut res);
    bishop_pinned::<THEM, BLOCKER>(position, king_color, blocker_color, &mut res);
    rook_pinned::<THEM, BLOCKER>(position, king_color, blocker_color, &mut res);

    Pinned::new(res)
}

// #[inline(never)]
fn bishop_pinned<const THEM: bool, const BLOCKER: bool>(
    position: &mut PositionAux,
    king_color: Color,
    blocker_color: Color,
    res: &mut Vec<(Square, BitBoard)>,
) {
    let power_from_king = bishop_power(position.must_king_pos(king_color));
    let mut potential_attackers =
        position.bishopish() & position.color_bb::<THEM>() & power_from_king;
    if potential_attackers.is_empty() {
        return;
    }

    let reachable_from_king =
        magic::bishop_reachable(position.occupied_bb(), position.must_king_pos(king_color));

    potential_attackers = potential_attackers.and_not(reachable_from_king);

    for attacker_pos in potential_attackers {
        let power_from_attacker = bishop_power(attacker_pos);

        let block = reachable_from_king
            & magic::bishop_reachable(position.occupied_bb(), attacker_pos)
            & position.color_bb::<BLOCKER>();
        if block.is_empty() {
            continue;
        }
        let blocker_pos = block.singleton();

        let blocker_kind = position.must_get_kind(blocker_pos);
        let reach = reachable(position, blocker_color, blocker_pos, blocker_kind, false)
            & (power_from_attacker & power_from_king | BitBoard::from_square(attacker_pos));

        res.push((blocker_pos, reach));
    }
}

// #[inline(never)]
fn rook_pinned<const THEM: bool, const BLOCKER: bool>(
    position: &mut PositionAux,
    king_color: Color,
    blocker_color: Color,
    res: &mut Vec<(Square, BitBoard)>,
) {
    let power_from_king = rook_power(position.must_king_pos(king_color));
    let mut potential_attackers =
        position.rookish() & position.color_bb::<THEM>() & power_from_king;
    if potential_attackers.is_empty() {
        return;
    }

    let reachable_from_king =
        magic::rook_reachable(position.occupied_bb(), position.must_king_pos(king_color));

    potential_attackers = potential_attackers.and_not(reachable_from_king);

    for attacker_pos in potential_attackers {
        let power_from_attacker = rook_power(attacker_pos);

        let block = reachable_from_king
            & magic::rook_reachable(position.occupied_bb(), attacker_pos)
            & position.color_bb::<BLOCKER>();
        if block.is_empty() {
            continue;
        }
        let blocker_pos = block.singleton();

        let blocker_kind = position.must_get_kind(blocker_pos);
        let reach = reachable(position, blocker_color, blocker_pos, blocker_kind, false)
            & (power_from_attacker & power_from_king | BitBoard::from_square(attacker_pos));

        res.push((blocker_pos, reach));
    }
}

// #[inline(never)]
fn lance_pinned<const BLOCKER: bool>(
    position: &mut PositionAux,
    king_color: Color,
    blocker_color: Color,
    res: &mut Vec<(Square, BitBoard)>,
) {
    let attacker_color = king_color.opposite();

    let lances = position.bitboard::<KIND_LANCE>(attacker_color);
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
        if !position.color_bb::<BLOCKER>().get(blocker_pos) {
            return;
        }
        let Some(attacker_pos) = occupied.next() else {
            return;
        };
        if !lances.get(attacker_pos) {
            return;
        }
        let blocker_kind = position.must_get_kind(blocker_pos);
        let reach =
            reachable(position, blocker_color, blocker_pos, blocker_kind, false) & power_from_king;
        res.push((blocker_pos, reach));
    } else {
        let mut occupied = occupied.u128();
        let blocker_pos = Square::from_index(127 - occupied.leading_zeros() as usize);
        if !position.color_bb::<BLOCKER>().get(blocker_pos) {
            return;
        }
        occupied &= !(1 << blocker_pos.index());
        if occupied == 0 {
            return;
        }
        let attacker_pos = Square::from_index(127 - occupied.leading_zeros() as usize);
        if !lances.get(attacker_pos) {
            return;
        }
        let blocker_kind = position.must_get_kind(blocker_pos);
        let reach =
            reachable(position, blocker_color, blocker_pos, blocker_kind, false) & power_from_king;
        res.push((blocker_pos, reach));
    }
}
