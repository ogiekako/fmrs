use crate::{
    piece::{Color, Kind},
    position::{
        bitboard::{self, BitBoard},
        Position, Square,
    },
};

// pinned piece and its movable positions (capturing included) pairs.
#[derive(Debug)]
pub(super) struct Pinned {
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
    fn new(pinned_areaa: Vec<(Square, BitBoard)>) -> Self {
        let mut mask = BitBoard::empty();
        pinned_areaa.iter().for_each(|(x, _)| mask.set(*x));
        Self {
            mask,
            pinned_area: pinned_areaa,
        }
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

pub(super) fn pinned(
    position: &Position,
    black_pieces: BitBoard,
    white_pieces: BitBoard,
    king_color: Color,
    king_pos: Square,
    blocker_color: Color,
) -> Pinned {
    let mut res = vec![];

    let attacker_color = king_color.opposite();
    let (blocker_color_pieces, non_blocker_color_pieces) = if blocker_color == Color::Black {
        (black_pieces, white_pieces)
    } else {
        (white_pieces, black_pieces)
    };

    for attacker_kind in [Kind::Lance, Kind::Bishop, Kind::Rook] {
        let power_mask = bitboard::power(king_color, king_pos, attacker_kind);
        let attackers = if attacker_kind == Kind::Lance {
            position.bitboard(attacker_color.into(), attacker_kind.into())
        } else {
            position.bitboard(attacker_color.into(), attacker_kind.into())
                | position.bitboard(attacker_color.into(), attacker_kind.promote())
        } & power_mask;
        if attackers.is_empty() {
            continue;
        }
        let king_seeing = bitboard::reachable2(
            blocker_color_pieces,
            non_blocker_color_pieces,
            king_color,
            king_pos,
            attacker_kind,
        );
        for attacker_pos in attackers {
            let attacker_within_reach = bitboard::reachable2(
                blocker_color_pieces,
                non_blocker_color_pieces,
                attacker_color,
                attacker_pos,
                attacker_kind,
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
            let pinned_kind = position.get(pinned_pos).unwrap().1;
            let pinned_reachable = bitboard::reachable(
                black_pieces,
                white_pieces,
                blocker_color,
                pinned_pos,
                pinned_kind,
            );
            let mut same_line = bitboard::power(king_color, king_pos, attacker_kind)
                & bitboard::power(attacker_color, attacker_pos, attacker_kind);
            same_line.set(attacker_pos);
            res.push((pinned_pos, pinned_reachable & same_line))
        }
    }
    Pinned::new(res)
}
