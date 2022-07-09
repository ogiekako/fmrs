use crate::{
    piece::{Color, Kind},
    position::{
        bitboard::{self, BitBoard},
        rule, Movement, Position, Square,
    },
};

pub fn checked(position: &Position, color: Color) -> bool {
    let king_pos = {
        if let Some(king_pos) = position.bitboard(color.into(), Kind::King.into()).next() {
            king_pos
        } else {
            return false;
        }
    };
    let opponent_pieces = position.bitboard(color.opposite().into(), None);
    let turn_pieces = position.bitboard(color.into(), None);
    let around_king = bitboard::power(color, king_pos, Kind::King);
    // Non line or leap moves
    for attacker_pos in around_king & opponent_pieces {
        let arracker_kind = position.get(attacker_pos).unwrap().1;
        if arracker_kind == Kind::Knight || arracker_kind.is_line_piece() {
            continue;
        }
        let attacker_power = bitboard::power(color.opposite(), attacker_pos, arracker_kind);
        if attacker_power.get(king_pos) {
            return true;
        }
    }
    for attacker_kind in [
        Kind::Lance,
        Kind::Knight,
        Kind::Bishop,
        Kind::Rook,
        Kind::ProBishop,
        Kind::ProRook,
    ] {
        let attackers = position.bitboard(color.opposite().into(), attacker_kind.into());
        if attackers.is_empty() {
            continue;
        }
        let attack_squares = if color == Color::Black {
            bitboard::reachable(
                turn_pieces,
                opponent_pieces,
                Color::Black,
                king_pos,
                attacker_kind,
            )
        } else {
            bitboard::reachable(
                opponent_pieces,
                turn_pieces,
                Color::White,
                king_pos,
                attacker_kind,
            )
        };
        if !(attackers & attack_squares).is_empty() {
            return true;
        }
    }
    false
}

// Checks double pawn, unmovable pieces.
pub(super) fn maybe_legal_movement(
    turn: Color,
    movement: &Movement,
    kind: Kind,
    pawn_mask: usize,
) -> bool {
    match movement {
        Movement::Drop(pos, kind) => {
            if kind == &Kind::Pawn && pawn_mask >> pos.col() & 1 > 0 {
                return false;
            }
            rule::is_movable(turn, *pos, *kind)
        }
        Movement::Move {
            source,
            dest,
            promote,
        } => rule::is_allowed_move(turn, *source, *dest, kind, *promote),
    }
}

// pinned piece and its movable positions (capturing included) pairs.
#[derive(Debug)]
pub(super) struct Pinned {
    mask: BitBoard,
    allowed_dests: Vec<(Square, BitBoard)>,
}

impl Pinned {
    pub fn empty() -> Self {
        Self {
            mask: BitBoard::empty(),
            allowed_dests: vec![],
        }
    }
    fn new(allowed_dests: Vec<(Square, BitBoard)>) -> Self {
        let mut mask = BitBoard::empty();
        allowed_dests.iter().for_each(|(x, _)| mask.set(*x));
        Self {
            mask,
            allowed_dests,
        }
    }
    pub fn legal_move(&self, pos: Square, dest: Square) -> bool {
        !self.is_pinned(pos) || self.legal_dests(pos).get(dest)
    }
    pub fn is_pinned(&self, pos: Square) -> bool {
        self.mask.get(pos)
    }
    pub fn legal_dests(&self, source: Square) -> BitBoard {
        for (pinned_pos, movable) in self.allowed_dests.iter() {
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
) -> Pinned {
    let mut res = vec![];
    for attacker_kind in [Kind::Lance, Kind::Bishop, Kind::Rook] {
        let power_mask = bitboard::power(king_color, king_pos, attacker_kind);
        let attackers = if attacker_kind == Kind::Lance {
            position.bitboard(king_color.opposite().into(), attacker_kind.into())
        } else {
            position.bitboard(king_color.opposite().into(), attacker_kind.into())
                | position.bitboard(king_color.opposite().into(), attacker_kind.promote())
        } & power_mask;
        if attackers.is_empty() {
            continue;
        }
        let king_seeing = bitboard::reachable(
            white_pieces,
            black_pieces,
            king_color,
            king_pos,
            attacker_kind,
        );
        for attacker_pos in attackers {
            let attacker_reachable = bitboard::reachable(
                black_pieces,
                white_pieces,
                king_color.opposite(),
                attacker_pos,
                attacker_kind,
            );
            if attacker_reachable.get(king_pos) {
                continue;
            }
            let pinned_pos = {
                let mut pinned = king_seeing & attacker_reachable;
                if pinned.is_empty() {
                    continue;
                }
                pinned.next().unwrap()
            };
            let pinned_kind = position.get(pinned_pos).unwrap().1;
            let pinned_reachable = bitboard::reachable(
                black_pieces,
                white_pieces,
                king_color,
                pinned_pos,
                pinned_kind,
            );
            let same_line = bitboard::power(king_color, king_pos, attacker_kind)
                & bitboard::power(king_color.opposite(), attacker_pos, attacker_kind);
            res.push((pinned_pos, pinned_reachable & same_line))
        }
    }
    Pinned::new(res)
}
