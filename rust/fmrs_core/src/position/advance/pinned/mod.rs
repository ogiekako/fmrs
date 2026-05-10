pub mod generated_magics;
pub mod magics_generator;

use crate::{
    piece::{Color, Kind},
    position::{
        bitboard::{
            bishop_power, lance_power, reachable, rook_power,
            BitBoard,
        },
        position::PositionAux,
        Square,
    },
};

/// Squares strictly between `a` and `b` on a rook line (same row or column).
/// Returns empty for pairs not on a rook line. Indexed by [a.index()][b.index()].
const ROOK_BETWEEN: [[BitBoard; 81]; 81] = build_rook_between();

/// Squares strictly between `a` and `b` on a bishop line (same diagonal).
/// Returns empty for pairs not on a bishop diagonal. Indexed by [a.index()][b.index()].
const BISHOP_BETWEEN: [[BitBoard; 81]; 81] = build_bishop_between();

const fn build_rook_between() -> [[BitBoard; 81]; 81] {
    let mut res = [[BitBoard::const_default(); 81]; 81];
    let mut a = 0;
    while a < 81 {
        let mut b = 0;
        while b < 81 {
            let a_sq = Square::from_index(a);
            let b_sq = Square::from_index(b);
            let mut bb = BitBoard::const_default();
            if a_sq.col() == b_sq.col() {
                let (lo, hi) = if a_sq.row() < b_sq.row() {
                    (a_sq.row(), b_sq.row())
                } else {
                    (b_sq.row(), a_sq.row())
                };
                let mut r = lo + 1;
                while r < hi {
                    bb.set(Square::new(a_sq.col(), r));
                    r += 1;
                }
            } else if a_sq.row() == b_sq.row() {
                let (lo, hi) = if a_sq.col() < b_sq.col() {
                    (a_sq.col(), b_sq.col())
                } else {
                    (b_sq.col(), a_sq.col())
                };
                let mut c = lo + 1;
                while c < hi {
                    bb.set(Square::new(c, a_sq.row()));
                    c += 1;
                }
            }
            res[a][b] = bb;
            b += 1;
        }
        a += 1;
    }
    res
}

const fn build_bishop_between() -> [[BitBoard; 81]; 81] {
    let mut res = [[BitBoard::const_default(); 81]; 81];
    let mut a = 0;
    while a < 81 {
        let mut b = 0;
        while b < 81 {
            let a_sq = Square::from_index(a);
            let b_sq = Square::from_index(b);
            let mut bb = BitBoard::const_default();
            let dc = b_sq.col() as isize - a_sq.col() as isize;
            let dr = b_sq.row() as isize - a_sq.row() as isize;
            if dc != 0 && (dc == dr || dc == -dr) {
                let step_c: isize = if dc > 0 { 1 } else { -1 };
                let step_r: isize = if dr > 0 { 1 } else { -1 };
                let mut c = a_sq.col() as isize + step_c;
                let mut r = a_sq.row() as isize + step_r;
                while c != b_sq.col() as isize {
                    bb.set(Square::new(c as usize, r as usize));
                    c += step_c;
                    r += step_r;
                }
            }
            res[a][b] = bb;
            b += 1;
        }
        a += 1;
    }
    res
}

#[inline(always)]
fn rook_between(a: Square, b: Square) -> BitBoard {
    ROOK_BETWEEN[a.index()][b.index()]
}

#[inline(always)]
fn bishop_between(a: Square, b: Square) -> BitBoard {
    BISHOP_BETWEEN[a.index()][b.index()]
}

// pinned piece and its movable positions (capturing included) pairs.
//
// SmallVec inline-buffers up to PINNED_INLINE_CAP entries; tsumeshogi rarely
// produces >2 pinned pieces per king (theoretical max ~8: 4 diagonals + 4
// orthogonals through the king, plus lance, minus overlaps). Storing inline
// avoids the malloc/free pair that `Vec::push` triggered on first push
// (~3% of total instructions on near_mate).
const PINNED_INLINE_CAP: usize = 2;

#[derive(Debug, Default)]
pub struct Pinned {
    pinned_bb: BitBoard,
    pinned_area: smallvec::SmallVec<[(Square, BitBoard); PINNED_INLINE_CAP]>,
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
    let mut res = Pinned::default();
    pinned_into(position, king_color, blocker_color, &mut res);
    res
}

/// Same as `pinned`, but writes into a caller-supplied `Pinned`. Saves the
/// return-by-value move (~96 bytes copy with PINNED_INLINE_CAP=2 SmallVec)
/// at hot call sites that own a `Pinned` slot inline.
pub fn pinned_into(
    position: &mut PositionAux,
    king_color: Color,
    blocker_color: Color,
    res: &mut Pinned,
) {
    let Some(king_pos) = position.king_pos(king_color) else {
        return;
    };

    let attacker_color = king_color.opposite();
    let attacker_bb = position.color_bb(attacker_color);
    let occupied = position.occupied_bb();
    let blocker_bb = position.color_bb(blocker_color);

    lance_pinned(position, king_color, blocker_color, res);

    let bishop_attackers =
        position.bishopish() & attacker_bb & bishop_power(king_pos);
    for attacker_pos in bishop_attackers {
        let between = bishop_between(king_pos, attacker_pos);
        check_line_pin(
            position,
            blocker_color,
            blocker_bb,
            occupied,
            attacker_pos,
            between,
            res,
        );
    }

    let rook_attackers = position.rookish() & attacker_bb & rook_power(king_pos);
    for attacker_pos in rook_attackers {
        let between = rook_between(king_pos, attacker_pos);
        check_line_pin(
            position,
            blocker_color,
            blocker_bb,
            occupied,
            attacker_pos,
            between,
            res,
        );
    }
}

#[inline]
fn check_line_pin(
    position: &PositionAux,
    blocker_color: Color,
    blocker_bb: BitBoard,
    occupied: BitBoard,
    attacker_pos: Square,
    between: BitBoard,
    res: &mut Pinned,
) {
    let blockers = between & occupied;
    if blockers.count_ones() != 1 {
        return;
    }
    let blocker_pos = blockers.singleton();
    if !blocker_bb.contains(blocker_pos) {
        return;
    }

    let blocker_kind = position.must_get_kind(blocker_pos);
    let pin_mask = between | BitBoard::EMPTY.with(attacker_pos);
    let reach = reachable(position, blocker_color, blocker_pos, blocker_kind, false) & pin_mask;

    res.push(blocker_pos, reach);
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
