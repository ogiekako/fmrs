use crate::{
    piece::{Color, Kind},
    position::{
        bitboard::{self, lance_reachable, reachable, reachable_sub, BitBoard, ColorBitBoard},
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

    let capture_same_color_from_king = king_color == blocker_color;

    let attacker_color = king_color.opposite();
    let capture_same_color_from_attacker = attacker_color == blocker_color;

    let mut res = vec![];

    let color_bb = position.color_bb();
    let both = color_bb.both();

    // Lance
    if let Some(e) = lance_pinned(position, king_color, king_pos, blocker_color) {
        res.push(e);
    }

    for attacker_kind in [Kind::Bishop, Kind::Rook] {
        let power_mask = bitboard::power(king_color, king_pos, attacker_kind);
        let potential_attackers = if attacker_kind == Kind::Lance {
            position.bitboard(attacker_color.into(), attacker_kind.into())
        } else {
            position.bitboard(attacker_color.into(), attacker_kind.into())
                | position.bitboard(attacker_color.into(), attacker_kind.promote())
        } & power_mask;
        if potential_attackers.is_empty() {
            continue;
        }
        let king_seeing =
            bitboard::reachable_sub(&color_bb, king_color, king_pos, attacker_kind) & both;
        if king_seeing.is_empty() {
            continue;
        }
        let king_seeing_blockers = king_seeing & color_bb.bitboard(blocker_color);
        if king_seeing_blockers.is_empty() {
            continue;
        }

        let updated_color_bb = match blocker_color {
            Color::BLACK => {
                ColorBitBoard::new(color_bb.black().and_not(king_seeing), color_bb.white())
            }
            Color::WHITE => {
                ColorBitBoard::new(color_bb.black(), color_bb.white().and_not(king_seeing))
            }
        };

        let attackers = (reachable(
            &updated_color_bb,
            king_color,
            king_pos,
            attacker_kind,
            false,
        ) & potential_attackers)
            .and_not(king_seeing);
        if attackers.is_empty() {
            continue;
        }

        for attacker_pos in attackers {
            let attacker_within_reach = bitboard::reachable(
                color_bb,
                attacker_color,
                attacker_pos,
                attacker_kind,
                capture_same_color_from_attacker,
            );
            let pinned_pos = {
                let mut pinned = king_seeing & attacker_within_reach;
                pinned.next().unwrap()
            };
            let pinned_kind = position.get(pinned_pos).unwrap().1;
            let pinned_reachable = bitboard::reachable(
                position.color_bb(),
                blocker_color,
                pinned_pos,
                pinned_kind,
                false,
            );
            let mut same_line = bitboard::power(king_color, king_pos, attacker_kind)
                & bitboard::power(attacker_color, attacker_pos, attacker_kind);
            same_line.set(attacker_pos);
            res.push((pinned_pos, pinned_reachable & same_line))
        }
    }
    Pinned::new(res)
}

fn lance_pinned(
    position: &Position,
    king_color: Color,
    king_pos: Square,
    blocker_color: Color,
) -> Option<(Square, BitBoard)> {
    let attacker_color = king_color.opposite();
    let color_bb = position.color_bb();

    let lances = position.bitboard(attacker_color.into(), Kind::Lance.into());
    if lances.is_empty() {
        return None;
    }

    let occupied = color_bb.both();

    let king_seeing = lance_reachable(occupied, king_color, king_pos);

    if let Some(blocker_pos) = (king_seeing & color_bb.bitboard(blocker_color)).next() {
        let blocker_seeing = lance_reachable(occupied, king_color, blocker_pos);
        if let Some(lance_pos) = (blocker_seeing & color_bb.bitboard(king_color.opposite())).next()
        {
            // TODO: check kind only
            if position.get(lance_pos) == Some((king_color.opposite(), Kind::Lance)) {
                let blocker_kind = position.get(blocker_pos).unwrap().1;
                let reach = reachable(color_bb, blocker_color, blocker_pos, blocker_kind, false)
                    & (king_seeing | blocker_seeing);
                return Some((blocker_pos, reach));
            }
        }
    }
    None
}
