use anyhow::bail;

use crate::piece::*;

pub enum UndoToken {
    UnDrop((Square, bool /* pawn drop */)),
    UnMove {
        from: Square,
        to: Square,
        promote: bool,
        capture: Option<Kind>,
        pawn_drop: bool,
    },
}

#[derive(Clone, Eq, Hash, PartialEq, Ord, PartialOrd)]
pub struct Position {
    kind_bb: [BitBoard; NUM_KIND],
    color_bb: [BitBoard; 2],
    hands: Hands,
    turn: Color,
    pawn_drop: bool,
}

#[test]
fn test_position_size() {
    // 272 bytes.
    assert_eq!(272, std::mem::size_of::<Position>());
}

use crate::sfen;
use std::fmt;
impl fmt::Debug for Position {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", sfen::encode_position(self))
    }
}

use std::collections::HashMap;

use super::bitboard::BitBoard;
use super::hands::Hands;
use super::Movement;
use super::Square;

impl Position {
    pub fn new() -> Position {
        Position {
            kind_bb: [BitBoard::new(); NUM_KIND],
            color_bb: [BitBoard::new(); 2],
            hands: Hands::new(),
            turn: Black,
            pawn_drop: false,
        }
    }
    pub fn turn(&self) -> Color {
        self.turn
    }
    pub fn set_turn(&mut self, c: Color) {
        self.turn = c;
    }
    pub fn hands(&self) -> &Hands {
        &self.hands
    }
    pub fn inc_hands(&mut self, c: Color, k: Kind) {
        self.hands.add(c, k);
    }
    pub fn get(&self, pos: Square) -> Option<(Color, Kind)> {
        for c in Color::iter() {
            if !self.color_bb[c.index()].get(pos) {
                continue;
            }
            for k in Kind::iter() {
                if self.kind_bb[k.index()].get(pos) {
                    return Some((c, k));
                }
            }
        }
        None
    }
    pub fn checked(&self, c: Color) -> bool {
        match self.king(c) {
            Some(king_pos) => self.attackers_to(king_pos, c.opposite()).next().is_some(),
            None => false,
        }
    }
    pub fn was_pawn_drop(&self) -> bool {
        self.pawn_drop
    }
    fn king(&self, c: Color) -> Option<Square> {
        for k in self.piece_bb(c, King) {
            return Some(k);
        }
        None
    }
    fn kind(&self, pos: Square) -> Option<Kind> {
        for k in Kind::iter() {
            if self.kind_bb[k.index()].get(pos) {
                return Some(k);
            }
        }
        None
    }

    fn occupied(&self) -> BitBoard {
        self.color_bb[0] | self.color_bb[1]
    }

    fn piece_bb(&self, c: Color, k: Kind) -> BitBoard {
        self.color_bb[c.index()] & self.kind_bb[k.index()]
    }
    // Movements with the given color to the given position, excluding king's movement.
    fn movements_to(&self, to: Square, c: Color) -> Vec<(Movement, Kind)> {
        let occupied = self.occupied();
        let mut res = vec![];
        // Drop
        for k in self.hands.kinds(c) {
            res.push((Movement::Drop(to, k), k));
        }
        // Movement::Move
        for k in Kind::iter() {
            if k == King {
                continue;
            }
            for from in super::bitboard::movable_positions(occupied, to, c.opposite(), k)
                & self.piece_bb(c, k)
            {
                add_move(&mut res, from, to, c, k);
            }
        }
        res
    }
    // Attackers with the given color to the given position, excluding king's movement.
    fn attackers_to(&self, to: Square, c: Color) -> impl Iterator<Item = (Square, Kind)> + '_ {
        let occupied = self.occupied();
        Kind::iter().flat_map(move |k| {
            let b = if k == King {
                BitBoard::new()
            } else {
                super::bitboard::movable_positions(occupied, to, c.opposite(), k)
                    & self.piece_bb(c, k)
            };
            b.map(move |from| (from, k))
        })
    }

    fn attackers_to_with_king(
        &self,
        to: Square,
        c: Color,
    ) -> impl Iterator<Item = (Square, Kind)> + '_ {
        let occupied = self.occupied();
        Kind::iter().flat_map(move |k| {
            let b = super::bitboard::movable_positions(occupied, to, c.opposite(), k)
                & self.piece_bb(c, k);
            b.map(move |from| (from, k))
        })
    }

    fn has_pawn_in_col(&self, pos: Square, c: Color) -> bool {
        let b = self.piece_bb(c, Pawn);
        !(COL_MASKS[pos.col()] & b).is_empty()
    }
    // pinned returns a list of pairs of pinned piece and its movable positions.
    // king_pos is the king's position whose color is c.
    // For example, if c is black, this method returns black pieces that are not movable
    // because the black king is pinned.
    fn pinned(&self, king_pos: Square, c: Color) -> HashMap<Square, BitBoard> {
        let mut res = HashMap::new();
        for line_piece_kind in Kind::iter() {
            if let Some(i) = line_piece_index(line_piece_kind) {
                for opponent_line_piece in self.piece_bb(c.opposite(), line_piece_kind) {
                    if let Some(pinned_bb) =
                        PIN[opponent_line_piece.index()][king_pos.index()][c.opposite().index()][i]
                    {
                        let all_pinned = pinned_bb & (self.color_bb[0] | self.color_bb[1]);
                        if all_pinned.into_iter().count() <= 2 {
                            let mut pinned = pinned_bb & self.color_bb[c.index()];
                            if let Some(p) = pinned.next() {
                                res.insert(p, pinned_bb);
                            }
                        }
                    }
                }
            }
        }
        res
    }

    // Generate check/uncheck moves checking illegality (two pawns, drop pawn mate, unmovable piece, self check).
    pub fn move_candidates(&self) -> anyhow::Result<Vec<Movement>> {
        let occu = self.occupied();
        let white_king_pos = self
            .king(White)
            .ok_or(anyhow::anyhow!("White king not found"))?;

        let turn = self.turn;
        let pinned = self.king(turn).map(|king_pos| self.pinned(king_pos, turn));

        // If black king is checked, it must be stopped.
        let black_attack_prevent_moves = {
            let mut black_attack_prevent_moves = None;
            if turn == Black {
                if let Some(black_king_pos) = self.king(Black) {
                    let attackers: Vec<_> = self.attackers_to(black_king_pos, White).collect();
                    if !attackers.is_empty() {
                        let mut moves = self
                            .generate_attack_preventing_moves(Black, black_king_pos, attackers)
                            .unwrap();
                        moves.sort();
                        black_attack_prevent_moves = Some(moves);
                    }
                }
            }
            black_attack_prevent_moves
        };

        let is_allowed = |m: Movement, k: Kind| -> bool {
            if let Some(allowed_moves) = &black_attack_prevent_moves {
                if allowed_moves.binary_search(&(m.clone(), k)).is_err() {
                    return false;
                }
            }
            match m {
                Movement::Drop(pos, k2) => {
                    debug_assert_eq!(k, k2);
                    if !movable(pos, turn, k) {
                        return false;
                    }
                    if k == Pawn {
                        if self.has_pawn_in_col(pos, turn) {
                            return false;
                        }
                    }
                }
                Movement::Move { from, to, promote } => {
                    if !promote && !movable(to, turn, k) {
                        return false;
                    }
                    if promote {
                        if !promotable(from, turn) && !promotable(to, turn) {
                            return false;
                        }
                    }
                    if let Some(mask) = pinned.as_ref().and_then(|x| x.get(&from)) {
                        if !mask.get(to) {
                            return false;
                        }
                    }
                    if k == King {
                        if self
                            .attackers_to_with_king(to, turn.opposite())
                            .next()
                            .is_some()
                        {
                            return false;
                        }
                    }
                }
            }
            return true;
        };

        let mut res = vec![];
        // Black.
        if turn == Black {
            // Drop
            for k in self.hands.kinds(turn) {
                for pos in
                    (!occu) & (super::bitboard::movable_positions(occu, white_king_pos, White, k))
                {
                    res.push((Movement::Drop(pos, k), k));
                }
            }
            // Direct attack
            for k in Kind::iter() {
                if k == King {
                    continue;
                }
                let froms = self.piece_bb(Black, k);
                let goal = (!self.color_bb[Black.index()])
                    & super::bitboard::movable_positions(occu, white_king_pos, White, k);
                for from in froms {
                    for to in goal & super::bitboard::movable_positions(occu, from, Black, k) {
                        res.push((
                            Movement::Move {
                                from,
                                to,
                                promote: false,
                            },
                            k,
                        ))
                    }
                }
                // promote
                if let Some(k) = k.unpromote() {
                    for from in self.piece_bb(Black, k) {
                        for to in goal & super::bitboard::movable_positions(occu, from, Black, k) {
                            if promotable(from, Black) || promotable(to, Black) {
                                res.push((
                                    Movement::Move {
                                        from,
                                        to,
                                        promote: true,
                                    },
                                    k,
                                ))
                            }
                        }
                    }
                }
            }

            // Discovered attack
            for k in vec![Lance, Bishop, Rook] {
                let mut attacker_cands = self.piece_bb(Black, k);
                if k != Lance {
                    attacker_cands |= self.piece_bb(Black, k.promote().unwrap());
                }
                attacker_cands &= super::bitboard::attacks_from(white_king_pos, White, k);
                if attacker_cands.is_empty() {
                    continue;
                }
                let blocker_cands = self.color_bb[Black.index()]
                    & super::bitboard::movable_positions(occu, white_king_pos, White, k);
                if blocker_cands.is_empty() {
                    continue;
                }
                for attacker in attacker_cands {
                    if let Some(from) =
                        (super::bitboard::movable_positions(occu, attacker, Black, k)
                            & blocker_cands)
                            .next()
                    {
                        let from_k = self.kind(from).unwrap();
                        for to in (!(super::bitboard::attacks_from(white_king_pos, White, k)
                            & super::bitboard::attacks_from(attacker, Black, k)))
                            & ((!self.color_bb[Black.index()])
                                & super::bitboard::movable_positions(occu, from, Black, from_k))
                        {
                            add_move(&mut res, from, to, Black, from_k);
                        }
                    }
                }
            }
        } else {
            res = self.generate_attack_preventing_moves(
                White,
                white_king_pos,
                self.attackers_to(white_king_pos, Black).collect(),
            )?;
        }
        let mut res: Vec<Movement> = res
            .into_iter()
            .filter(|(m, k)| is_allowed(*m, *k))
            .map(|x| x.0)
            .collect();
        res.sort_unstable();
        // TODO: Remove necessity of dedup.
        res.dedup();
        Ok(res)
    }

    fn generate_attack_preventing_moves(
        &self,
        turn: Color,
        king_pos: Square,
        attackers: Vec<(Square, Kind)>,
    ) -> anyhow::Result<Vec<(Movement, Kind)>> {
        if attackers.is_empty() {
            bail!("Wrong board optision: no attacker");
        }
        if attackers.len() > 2 {
            bail!("Attacked by more than 2 pieces");
        }

        let mut res = vec![];

        // Potential attacked positions which are currently hidden by the king. King cannot move there.
        // It's a workaround for the bug that those places are not considered as attacked in is_allowed.
        fn hidden_square(attacker_pos: Square, king_pos: Square) -> Option<Square> {
            let (kc, kr) = (king_pos.col() as isize, king_pos.row() as isize);
            let (ac, ar) = (attacker_pos.col() as isize, attacker_pos.row() as isize);

            let (dc, dr) = (kc - ac, kr - ar);
            let d = dc.abs().max(dr.abs());
            let (rc, rr) = (kc + dc / d, kr + dr / d);
            if 0 <= rc && rc < 9 && 0 <= rr && rr < 9 {
                Some(Square::new(rc as usize, rr as usize))
            } else {
                None
            }
        }

        let mut hidden = BitBoard::new();
        for (pos, k) in attackers.iter() {
            if k.is_line_piece() {
                if let Some(k) = k.unpromote() {
                    if !super::bitboard::attacks_from(*pos, turn.opposite(), k).get(king_pos) {
                        continue;
                    }
                }
                if let Some(p) = hidden_square(*pos, king_pos) {
                    hidden.set(p);
                }
            }
        }
        let hidden = hidden;
        // Pin
        if attackers.len() == 1 && attackers[0].1.is_line_piece() {
            let (pos, k) = attackers[0];
            if let Some(mut pin_bb) = PIN[pos.index()][king_pos.index()][turn.opposite().index()]
                [line_piece_index(k).unwrap()]
            {
                pin_bb.unset(pos);
                for pin_pos in pin_bb {
                    res.append(&mut self.movements_to(pin_pos, turn));
                }
            }
        }
        // Capture
        if attackers.len() == 1 {
            for (pos, kind) in self.attackers_to(attackers[0].0, turn) {
                add_move(&mut res, pos, attackers[0].0, turn, kind);
            }
        }
        // King move
        for pos in super::bitboard::movable_positions(self.occupied(), king_pos, turn, King)
            & (!self.color_bb[turn.index()])
        {
            if hidden.get(pos) {
                continue;
            }
            res.push((
                Movement::Move {
                    from: king_pos,
                    to: pos,
                    promote: false,
                },
                King,
            ));
        }
        Ok(res)
    }

    // Make the movement assuming m is a valid movement. Otherwise it panics.
    // TODO: return a Result.
    pub fn do_move(&mut self, m: &Movement) -> UndoToken {
        use UndoToken::*;
        let c = self.turn;
        let token;
        match m {
            Movement::Drop(pos, k) => {
                let (pos, k) = (*pos, *k);
                self.hands.remove(c, k);
                self.set(pos, c, k);
                token = UnDrop((pos, self.pawn_drop));
                self.pawn_drop = k == Kind::Pawn;
            }
            Movement::Move { from, to, promote } => {
                let (from, to, promote) = (*from, *to, *promote);
                // TODO: return error instead of unwrapping.
                let k = self.kind(from).unwrap();
                self.unset(from, c, k);
                let mut capture = None;
                if self.color_bb[c.opposite().index()].get(to) {
                    for capt_k in Kind::iter() {
                        if self.kind_bb[capt_k.index()].get(to) {
                            self.unset(to, c.opposite(), capt_k);
                            self.hands.add(c, capt_k.maybe_unpromote());
                            capture = Some(capt_k);
                            break;
                        }
                    }
                }
                if promote {
                    self.set(to, c, k.promote().unwrap());
                } else {
                    self.set(to, c, k);
                }
                token = UnMove {
                    from,
                    to,
                    promote,
                    capture,
                    pawn_drop: self.pawn_drop,
                };
                self.pawn_drop = false;
            }
        }
        self.turn = c.opposite();
        token
    }
    // Undoes an movement. The token should be valid for the current position and otherwise it panics.
    // Returns the movement to redo.
    pub fn undo(&mut self, token: &UndoToken) -> Movement {
        use UndoToken::*;
        let prev_turn = self.turn.opposite();
        self.turn = prev_turn;
        match token {
            &UnDrop((pos, pawn_drop)) => {
                let (c, k) = self
                    .get(pos)
                    .expect(&format!("{:?} doesn't contain any piece", pos));
                debug_assert_eq!(prev_turn, c);
                self.unset(pos, c, k);
                self.hands.add(c, k.maybe_unpromote());
                self.pawn_drop = pawn_drop;
                Movement::Drop(pos, k.maybe_unpromote())
            }
            &UnMove {
                from,
                to,
                promote,
                capture,
                pawn_drop,
            } => {
                let (c, k) = self
                    .get(to)
                    .expect(&format!("{:?} doesn't contain any piece", to));
                debug_assert_eq!(prev_turn, c);
                self.unset(to, c, k);
                debug_assert_eq!(None, self.get(from));
                let prev_k = if promote {
                    k.unpromote().expect(&format!("can't unpromote {:?}", k))
                } else {
                    k
                };
                self.set(from, c, prev_k);
                if let Some(captured_k) = capture {
                    self.set(to, c.opposite(), captured_k);
                    self.hands.remove(c, captured_k.maybe_unpromote());
                }
                self.pawn_drop = pawn_drop;
                Movement::Move { from, to, promote }
            }
        }
    }
    pub fn set(&mut self, pos: Square, c: Color, k: Kind) {
        debug_assert_eq!(false, self.color_bb[c.index()].get(pos));
        self.color_bb[c.index()].set(pos);
        debug_assert_eq!(false, self.kind_bb[k.index()].get(pos));
        self.kind_bb[k.index()].set(pos);
    }
    fn unset(&mut self, pos: Square, c: Color, k: Kind) {
        debug_assert!(self.color_bb[c.index()].get(pos));
        self.color_bb[c.index()].unset(pos);
        debug_assert!(self.kind_bb[k.index()].get(pos));
        self.kind_bb[k.index()].unset(pos);
    }
}

fn promotable(pos: Square, c: Color) -> bool {
    match c {
        Black => pos.row() < 3,
        White => pos.row() >= 6,
    }
}

fn add_move(moves: &mut Vec<(Movement, Kind)>, from: Square, to: Square, c: Color, k: Kind) {
    moves.push((
        Movement::Move {
            from,
            to,
            promote: false,
        },
        k,
    ));
    if (promotable(from, c) || promotable(to, c)) && k.promote().is_some() {
        moves.push((
            Movement::Move {
                from,
                to,
                promote: true,
            },
            k,
        ))
    }
}

fn line_piece_index(k: Kind) -> Option<usize> {
    Some(match k {
        Lance => 0,
        Bishop | ProBishop => 1,
        Rook | ProRook => 2,
        _ => return None,
    })
}

fn movable(pos: Square, c: Color, k: Kind) -> bool {
    MOVABLE[pos.index()][c.index()][k.index()]
}

lazy_static! {
    static ref COL_MASKS: [BitBoard; 9] = {
        let mut res = [BitBoard::new(); 9];
        for pos in Square::iter() {
            res[pos.col()].set(pos);
        }
        res
    };
    // line_pos, king_pos, color (of line piece), line_piece_index -> pinned squares.
    static ref PIN: Vec<[[[Option<BitBoard>; 3]; 2]; 81]> = {
        let mut res = vec![[[[None; 3]; 2]; 81]; 81];
        for from in Square::iter() {
            for to in Square::iter() {
                let mut bounding = BitBoard::new();
                let (c1, c2) = (from.col(), to.col());
                let (r1, r2) = (from.row(), to.row());
                for c in c1.min(c2)..=c1.max(c2) {
                    for r in r1.min(r2)..=r1.max(r2) {
                        bounding.set(Square::new(c, r));
                    }
                }

                for c in Color::iter() {
                    for k in [Lance, Bishop, Rook].iter().map(|k|*k) {
                        let a = super::bitboard::attacks_from(from, c, k);
                        if !a.get(to) {
                            continue;
                        }
                        let mut mask = super::bitboard::attacks_from(to, c.opposite(), k) & a & bounding;
                        if !mask.is_empty() {
                            mask.set(from);
                            res[from.index()][to.index()][c.index()][line_piece_index(k).unwrap()]
                                = Some(mask);
                        }
                    }
                }
            }
        }
        res
    };
    // pos, color, kind -> MOVABLE
    // It's used to check illegal position of a piece. (e.g. black pawn in the first row.)
    static ref MOVABLE: [[[bool; NUM_KIND]; 2]; 81] = {
        let mut res = [[[false; NUM_KIND]; 2]; 81];
        for k in Kind::iter() {
            for c in Color::iter() {
                for pos in Square::iter() {
                    res[pos.index()][c.index()][k.index()] = !super::bitboard::attacks_from(pos, c, k).is_empty();
                }
            }
        }
        res
    };
}

#[cfg(test)]
mod tests {
    use crate::{
        piece::{Color, Kind},
        position::{Movement, Position, Square},
        sfen,
    };

    #[test]
    fn test_pin() {
        macro_rules! map(
            { $($key:expr => $value:expr),* } => {
                {
                    let mut m = ::std::collections::HashMap::new();
                    $(
                        m.insert($key, $value);
                    )*
                    m
                }
            };
        );
        let board =
            sfen::decode_position("3ll4/7B1/3Pp1p2/3+P5/4k4/4r4/3K2G+r1/4L4/8+B b 3g4s4nl14p 1")
                .unwrap();
        assert_eq!(
            board.pinned(board.king(Color::Black).unwrap(), Color::Black),
            map! {
                Square::new(2, 6) => bitboard!{
                    ".........",
                    ".........",
                    ".........",
                    ".........",
                    ".........",
                    ".........",
                    "....****.",
                    ".........",
                    ".........",
                }
            }
        );
        assert_eq!(
            board.pinned(board.king(Color::White).unwrap(), Color::White),
            map! {
                Square::new(4, 5) => bitboard!{
                    ".........",
                    ".........",
                    ".........",
                    ".........",
                    ".........",
                    "....*....",
                    "....*....",
                    "....*....",
                    ".........",
                },
                Square::new(2, 2) => bitboard!{
                    ".........",
                    ".......*.",
                    "......*..",
                    ".....*...",
                    ".........",
                    ".........",
                    ".........",
                    ".........",
                    ".........",
                }
            }
        );
    }

    #[test]
    fn test_next_positions() {
        use crate::sfen;
        use pretty_assertions::assert_eq;
        for tc in vec![
            // Black moves
            (
                "8k/9/9/9/9/9/9/9/9 b P2r2b4g4s4n4l17p 1",
                // Drop pawn
                vec!["P*12"],
            ),
            (
                "9/9/5lp2/5lk2/5l3/9/5N3/7L1/9 b P2r2b4g4s3n16p 1",
                // Drop pawn mate is not checked here
                vec!["P*35"],
            ),
            (
                "8k/9/8K/9/9/9/9/9/9 b 2r2b4g4s4n4l18p 1",
                // King cannot attack
                vec![],
            ),
            (
                "4R4/9/4P4/9/4k1P1R/9/2N1s4/1B7/4L4 b b4g3s3n3l16p 1",
                // Discovered attacks
                vec!["7785", "7765", "5957", "3534"],
            ),
            (
                "9/8k/9/9/9/8N/9/9/8L b 2r2b4g4s3n3l18p 1",
                // Double check should not be counted twice.
                vec!["1624"],
            ),
            (
                "9/1P5B1/2SS2P2/5N3/+RL2k4/9/3pK1G+r1/9/4L3+B b N3g2s2n2l15p 1",
                vec![
                    "8e8d", "8e8c", "8e8c+", "7c6d", "6c5d+", "N*47", "3747", "3727", "5767",
                    "5747", "5748",
                ],
            ),
            // White moves
            ("7lk/7nP/9/9/9/9/9/9/8K w 2R2B4G4S3N3L17P 1", vec!["1112"]),
            (
                "9/9/6B2/5n3/2G1kn3/5n3/3K5/9/4L4 w 2RB3G4SN3L18P 1",
                vec!["4658+", "4557", "4557+"],
            ),
            (
                "9/9/9/3bkb3/5+R3/3+R5/9/9/9 w 4g4s4n4l18p 1",
                vec!["5463", "5453", "5443", "5445"],
            ),
        ] {
            let board = sfen::decode_position(tc.0).expect(&format!("Failed to decode {}", tc.0));
            let mut got = board
                .move_candidates()
                .expect("Failed to get next positions")
                .into_iter()
                .map(|x| {
                    let mut np = board.clone();
                    np.do_move(&x);
                    np
                })
                .collect::<Vec<Position>>();
            got.sort();
            let mut want = sfen::decode_moves(&tc.1.join(" "))
                .unwrap()
                .iter()
                .map(|m| {
                    let mut b = board.clone();
                    b.do_move(m);
                    b
                })
                .collect::<Vec<Position>>();
            want.sort();
            eprintln!("{}", tc.0);
            assert_eq!(got, want);
        }
    }

    #[test]
    fn test_do_move_undo() {
        use crate::sfen;
        for tc in vec![
            (
                sfen::tests::START,
                Movement::Move {
                    from: Square::new(1, 6),
                    to: Square::new(1, 5),
                    promote: false,
                },
                "lnsgkgsnl/1r5b1/ppppppppp/9/9/7P1/PPPPPPP1P/1B5R1/LNSGKGSNL w -",
            ),
            (
                sfen::tests::RYUO,
                Movement::Drop(Square::new(2, 0), Kind::Pawn),
                "6p1l/1l+R2P3/p2pBG1pp/kps1p4/Nn1P2G2/P1P1P2PP/1PS6/1KSG3+r1/LN2+p3L b Sbgn2p",
            ),
            (
                sfen::tests::RYUO,
                // Capture and promote.
                Movement::Move {
                    from: Square::new(7, 4),
                    to: Square::new(6, 6),
                    promote: true,
                },
                "8l/1l+R2P3/p2pBG1pp/kps1p4/N2P2G2/P1P1P2PP/1P+n6/1KSG3+r1/LN2+p3L b Sbgsn3p",
            ),
        ] {
            let (board_sfen, movement, want) = (tc.0, tc.1, tc.2);
            let mut board = sfen::decode_position(board_sfen).unwrap();
            let token = board.do_move(&movement);
            assert_eq!(want, sfen::encode_position(&board));
            let m = board.undo(&token);
            assert_eq!(board_sfen, sfen::encode_position(&board));
            board.do_move(&m);
            assert_eq!(want, sfen::encode_position(&board));
        }
    }
}
