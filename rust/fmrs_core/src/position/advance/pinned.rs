use crate::{
    piece::{Color, Kind},
    position::{
        bitboard::{bishop_power, lance_power, magic, reachable, rook_power, BitBoard},
        Position, Square,
    },
};

// pinned piece and its movable positions (capturing included) pairs.
#[derive(Debug)]
pub struct Pinned {
    mask: BitBoard,
    pinned_area: Vec<(Square, BitBoard)>,
}

impl Pinned {
    pub fn empty() -> Self {
        Self {
            mask: BitBoard::empty(),
            pinned_area: vec![],
        }
    }
    fn new(pinned_area: Vec<(Square, BitBoard)>) -> Self {
        let mut mask = BitBoard::empty();
        pinned_area.iter().for_each(|(x, _)| mask.set(*x));
        Self { mask, pinned_area }
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
    pub fn pinned_area(&self, source: Square) -> BitBoard {
        for (pinned_pos, movable) in self.pinned_area.iter() {
            if source == *pinned_pos {
                return *movable;
            }
        }
        panic!("BUG: is_pinned(source) should be true");
    }
}

pub fn pinned(
    position: &Position,
    king_color: Color,
    king_pos: Square,
    blocker_color: Color,
) -> Pinned {
    debug_assert!(position.get(king_pos).unwrap() == (king_color, Kind::King));

    let mut res = vec![];

    lance_pinned(position, king_color, king_pos, blocker_color, &mut res);
    bishop_pinned(position, king_color, king_pos, blocker_color, &mut res);
    rook_pinned(position, king_color, king_pos, blocker_color, &mut res);

    Pinned::new(res)
}

// #[inline(never)]
fn bishop_pinned(
    position: &Position,
    king_color: Color,
    king_pos: Square,
    blocker_color: Color,
    res: &mut Vec<(Square, BitBoard)>,
) {
    let color_bb = position.color_bb();
    let attacker_color = king_color.opposite();

    let power_from_king = bishop_power(king_pos);
    let mut potential_attackers =
        position.kind_bb().bishopish() & color_bb.bitboard(attacker_color) & power_from_king;
    if potential_attackers.is_empty() {
        return;
    }

    let occupied = color_bb.both();

    let reachable_from_king = magic::bishop_reachable(occupied, king_pos);

    potential_attackers = potential_attackers.and_not(reachable_from_king);

    for attacker_pos in potential_attackers {
        let power_from_attacker = bishop_power(attacker_pos);

        let block = reachable_from_king
            & magic::bishop_reachable(occupied, attacker_pos)
            & color_bb.bitboard(blocker_color);
        if block.is_empty() {
            continue;
        }
        let blocker_pos = block.singleton();

        let blocker_kind = position.kind_bb().must_get(blocker_pos);
        let reach = reachable(color_bb, blocker_color, blocker_pos, blocker_kind, false)
            & (power_from_attacker & power_from_king | BitBoard::from_square(attacker_pos));

        res.push((blocker_pos, reach));
    }
}

// #[inline(never)]
fn rook_pinned(
    position: &Position,
    king_color: Color,
    king_pos: Square,
    blocker_color: Color,
    res: &mut Vec<(Square, BitBoard)>,
) {
    let color_bb = position.color_bb();
    let attacker_color = king_color.opposite();

    let power_from_king = rook_power(king_pos);
    let mut potential_attackers =
        position.kind_bb().rookish() & color_bb.bitboard(attacker_color) & power_from_king;
    if potential_attackers.is_empty() {
        return;
    }

    let occupied = color_bb.both();

    let reachable_from_king = magic::rook_reachable(occupied, king_pos);

    potential_attackers = potential_attackers.and_not(reachable_from_king);

    for attacker_pos in potential_attackers {
        let power_from_attacker = rook_power(attacker_pos);

        let block = reachable_from_king
            & magic::rook_reachable(occupied, attacker_pos)
            & color_bb.bitboard(blocker_color);
        if block.is_empty() {
            continue;
        }
        let blocker_pos = block.singleton();

        let blocker_kind = position.kind_bb().must_get(blocker_pos);
        let reach = reachable(color_bb, blocker_color, blocker_pos, blocker_kind, false)
            & (power_from_attacker & power_from_king | BitBoard::from_square(attacker_pos));

        res.push((blocker_pos, reach));
    }
}

// #[inline(never)]
fn lance_pinned(
    position: &Position,
    king_color: Color,
    king_pos: Square,
    blocker_color: Color,
    res: &mut Vec<(Square, BitBoard)>,
) {
    let color_bb = position.color_bb();
    let attacker_color = king_color.opposite();

    let lances = position.bitboard(attacker_color, Kind::Lance);
    if lances.is_empty() {
        return;
    }

    let power_from_king = lance_power(king_color, king_pos);
    let potential_attackers = lances & power_from_king;
    if potential_attackers.is_empty() {
        return;
    }

    let mut occupied = color_bb.both() & power_from_king;

    if king_color.is_white() {
        let blocker_pos = occupied.next().unwrap();
        if !color_bb.bitboard(blocker_color).get(blocker_pos) {
            return;
        }
        let Some(attacker_pos) = occupied.next() else {
            return;
        };
        if !lances.get(attacker_pos) {
            return;
        }
        let blocker_kind = position.kind_bb().must_get(blocker_pos);
        let reach =
            reachable(color_bb, blocker_color, blocker_pos, blocker_kind, false) & power_from_king;
        res.push((blocker_pos, reach));
    } else {
        let mut occupied = occupied.u128();
        let blocker_pos = Square::from_index(127 - occupied.leading_zeros() as usize);
        if !color_bb.bitboard(blocker_color).get(blocker_pos) {
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
        let blocker_kind = position.kind_bb().must_get(blocker_pos);
        let reach =
            reachable(color_bb, blocker_color, blocker_pos, blocker_kind, false) & power_from_king;
        res.push((blocker_pos, reach));
    }
}
