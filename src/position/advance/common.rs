use crate::{
    piece::{Color, Kind},
    position::{
        bitboard11::{self, BitBoard},
        rule, Movement, Position, Square,
    },
};

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
pub(super) struct Pinned(Vec<(Square, BitBoard)>);

impl Pinned {
    pub(super) fn legal_move(&self, source: Square, dest: Square) -> bool {
        for (pinned_pos, movable) in self.0.iter() {
            if source == *pinned_pos {
                return movable.get(dest);
            }
        }
        true
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
        let power_mask = bitboard11::power(king_color, king_pos, attacker_kind);
        let attackers = if attacker_kind == Kind::Lance {
            position.bitboard(king_color.opposite().into(), attacker_kind.into())
        } else {
            position.bitboard(king_color.opposite().into(), attacker_kind.into())
                | position.bitboard(king_color.opposite().into(), attacker_kind.promote())
        } & power_mask;
        if attackers.is_empty() {
            continue;
        }
        let king_seeing = bitboard11::reachable(
            white_pieces,
            black_pieces,
            king_color,
            king_pos,
            attacker_kind,
        );
        for attacker_pos in attackers {
            let attacker_reachable = bitboard11::reachable(
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
            let pinned_reachable = bitboard11::reachable(
                black_pieces,
                white_pieces,
                king_color,
                pinned_pos,
                pinned_kind,
            );
            let same_line = bitboard11::power(king_color, king_pos, attacker_kind)
                & bitboard11::power(king_color.opposite(), attacker_pos, attacker_kind);
            res.push((pinned_pos, pinned_reachable & same_line))
        }
    }
    Pinned(res)
}
