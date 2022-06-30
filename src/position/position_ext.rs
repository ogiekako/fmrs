use std::collections::HashMap;

use anyhow::bail;

use crate::piece::{Color, Kind, NUM_KIND};

use super::{bitboard::BitBoard, rule::promotable, Movement, Position, Square, UndoToken};

pub trait PositionExt {
    fn do_move(&mut self, m: &Movement) -> UndoToken;
    fn undo(&mut self, token: &UndoToken) -> Movement;
    // fn undo(&mut self, token: &UndoToken) -> Movement;
    fn move_candidates(&self, res: &mut Vec<Movement>) -> anyhow::Result<()>;
    fn checked(&self, c: Color) -> bool;
}

impl PositionExt for Position {
    fn do_move(&mut self, m: &Movement) -> UndoToken {
        let c = self.turn;
        let token;
        match m {
            Movement::Drop(pos, k) => {
                let (pos, k) = (*pos, *k);
                self.hands_mut().remove(c, k);
                self.set(pos, c, k);
                token = UndoToken::UnDrop((pos, self.pawn_drop()));
                self.set_pawn_drop(k == Kind::Pawn);
            }
            Movement::Move { from, to, promote } => {
                let (from, to, promote) = (*from, *to, *promote);
                // TODO: return error instead of unwrapping.
                let k = self.get(from).unwrap().1;
                self.unset(from, c, k);
                let mut capture = None;
                if self.bitboard(Some(c.opposite()), None).get(to) {
                    for capt_k in Kind::iter() {
                        if self.bitboard(None, Some(capt_k)).get(to) {
                            self.unset(to, c.opposite(), capt_k);
                            self.hands_mut().add(c, capt_k.maybe_unpromote());
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
                token = UndoToken::UnMove {
                    from,
                    to,
                    promote,
                    capture,
                    pawn_drop: self.pawn_drop(),
                };
                self.set_pawn_drop(false);
            }
        }
        self.turn = c.opposite();
        token
    }

    // Undoes an movement. The token should be valid for the current position and otherwise it panics.
    // Returns the movement to redo.
    fn undo(&mut self, token: &UndoToken) -> Movement {
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
                self.hands_mut().add(c, k.maybe_unpromote());
                self.set_pawn_drop(pawn_drop);
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
                    self.hands_mut().remove(c, captured_k.maybe_unpromote());
                }
                self.set_pawn_drop(pawn_drop);
                Movement::Move { from, to, promote }
            }
        }
    }

    // Generate check/uncheck moves without checking illegality.
    fn move_candidates(&self, res: &mut Vec<Movement>) -> anyhow::Result<()> {
        let occu = self.bitboard(None, None);
        let white_king_pos =
            king(self, Color::White).ok_or(anyhow::anyhow!("White king not found"))?;

        let turn = self.turn;

        // Black.
        if turn == Color::Black {
            // Drop
            for k in self.hands().kinds(turn) {
                for pos in (!occu)
                    & (super::bitboard::movable_positions(occu, white_king_pos, Color::White, k))
                {
                    res.push(Movement::Drop(pos, k));
                }
            }

            let mut direct_attack_goals = [BitBoard::new(); NUM_KIND];
            for k in Kind::iter() {
                direct_attack_goals[k.index()] = !self.bitboard(Some(Color::Black), None)
                    & super::bitboard::movable_positions(occu, white_king_pos, Color::White, k);
            }

            // Direct attack
            for k in Kind::iter() {
                if k == Kind::King {
                    continue;
                }
                let froms = self.bitboard(Some(Color::Black), Some(k));
                let goal = direct_attack_goals[k.index()];
                for from in froms {
                    for to in goal & super::bitboard::movable_positions(occu, from, Color::Black, k)
                    {
                        res.push(Movement::Move {
                            from,
                            to,
                            promote: false,
                        })
                    }
                }
                // promote
                if let Some(k) = k.unpromote() {
                    for from in self.bitboard(Some(Color::Black), Some(k)) {
                        for to in
                            goal & super::bitboard::movable_positions(occu, from, Color::Black, k)
                        {
                            if promotable(from, Color::Black) || promotable(to, Color::Black) {
                                res.push(Movement::Move {
                                    from,
                                    to,
                                    promote: true,
                                });
                            }
                        }
                    }
                }
            }

            // Discovered attack
            for k in vec![Kind::Lance, Kind::Bishop, Kind::Rook] {
                let mut attacker_cands = self.bitboard(Some(Color::Black), Some(k));
                if k != Kind::Lance {
                    attacker_cands |= self.bitboard(Some(Color::Black), Some(k.promote().unwrap()));
                }
                attacker_cands &= super::bitboard::attacks_from(white_king_pos, Color::White, k);
                if attacker_cands.is_empty() {
                    continue;
                }
                let blocker_cands = self.bitboard(Some(Color::Black), None)
                    & super::bitboard::movable_positions(occu, white_king_pos, Color::White, k);
                if blocker_cands.is_empty() {
                    continue;
                }
                for attacker in attacker_cands {
                    if let Some(from) =
                        (super::bitboard::movable_positions(occu, attacker, Color::Black, k)
                            & blocker_cands)
                            .next()
                    {
                        let from_k = self.get(from).unwrap().1;
                        for to in
                            (!(super::bitboard::attacks_from(white_king_pos, Color::White, k)
                                & super::bitboard::attacks_from(attacker, Color::Black, k)))
                                & ((!self.bitboard(Some(Color::Black), None))
                                    & super::bitboard::movable_positions(
                                        occu,
                                        from,
                                        Color::Black,
                                        from_k,
                                    ))
                        {
                            if !direct_attack_goals[from_k.index()].get(to) {
                                res.push(Movement::Move {
                                    from,
                                    to,
                                    promote: false,
                                })
                            }

                            if (promotable(from, Color::Black) || promotable(to, Color::Black))
                                && from_k.promote().is_some()
                                && !direct_attack_goals[from_k.promote().unwrap().index()].get(to)
                            {
                                res.push(Movement::Move {
                                    from,
                                    to,
                                    promote: true,
                                })
                            }
                        }
                    }
                }
            }
        } else {
            generate_attack_preventing_moves(
                self,
                res,
                Color::White,
                white_king_pos,
                attackers_to(self, white_king_pos, Color::Black).collect(),
            )?;
        }
        Ok(())
    }

    fn checked(&self, c: Color) -> bool {
        match king(self, c) {
            Some(king_pos) => attackers_to(self, king_pos, c.opposite()).next().is_some(),
            None => false,
        }
    }
}

// Attackers with the given color to the given position, excluding king's movement.
pub(super) fn attackers_to(
    position: &Position,
    to: Square,
    c: Color,
) -> impl Iterator<Item = (Square, Kind)> + '_ {
    let occupied = position.bitboard(None, None);
    Kind::iter().flat_map(move |k| {
        let b = if k == Kind::King {
            BitBoard::new()
        } else {
            super::bitboard::movable_positions(occupied, to, c.opposite(), k)
                & position.bitboard(Some(c), Some(k))
        };
        b.map(move |from| (from, k))
    })
}

pub(super) fn attackers_to_with_king(
    position: &Position,
    to: Square,
    c: Color,
) -> impl Iterator<Item = (Square, Kind)> + '_ {
    let occupied = position.bitboard(None, None);
    Kind::iter().flat_map(move |k| {
        let b = super::bitboard::movable_positions(occupied, to, c.opposite(), k)
            & position.bitboard(Some(c), Some(k));
        b.map(move |from| (from, k))
    })
}

pub(super) fn generate_attack_preventing_moves(
    position: &Position,
    res: &mut Vec<Movement>,
    turn: Color,
    king_pos: Square,
    attackers: Vec<(Square, Kind)>,
) -> anyhow::Result<()> {
    if attackers.is_empty() {
        bail!("Wrong board optision: no attacker");
    }
    if attackers.len() > 2 {
        bail!("Attacked by more than 2 pieces");
    }

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
                add_movements_to(position, res, pin_pos, turn);
            }
        }
    }
    // Capture
    if attackers.len() == 1 {
        for (pos, kind) in attackers_to(&position, attackers[0].0, turn) {
            add_move(res, pos, attackers[0].0, turn, kind);
        }
    }
    // King move
    for pos in super::bitboard::movable_positions(
        position.bitboard(None, None),
        king_pos,
        turn,
        Kind::King,
    ) & (!position.bitboard(Some(turn), None))
    {
        if hidden.get(pos) {
            continue;
        }
        res.push(Movement::Move {
            from: king_pos,
            to: pos,
            promote: false,
        });
    }
    Ok(())
}

fn king(position: &Position, c: Color) -> Option<Square> {
    for k in position.bitboard(Some(c), Some(Kind::King)) {
        return Some(k);
    }
    None
}

pub(super) fn movable(pos: Square, c: Color, k: Kind) -> bool {
    MOVABLE[pos.index()][c.index()][k.index()]
}

// Movements with the given color to the given position, excluding king's movement.
fn add_movements_to(position: &Position, res: &mut Vec<Movement>, to: Square, c: Color) {
    let occupied = position.bitboard(None, None);
    // Drop
    for k in position.hands().kinds(c) {
        res.push(Movement::Drop(to, k));
    }
    // Movement::Move
    for k in Kind::iter() {
        if k == Kind::King {
            continue;
        }
        for from in super::bitboard::movable_positions(occupied, to, c.opposite(), k)
            & position.bitboard(Some(c), Some(k))
        {
            add_move(res, from, to, c, k);
        }
    }
}

pub(super) fn add_move(moves: &mut Vec<Movement>, from: Square, to: Square, c: Color, k: Kind) {
    moves.push(Movement::Move {
        from,
        to,
        promote: false,
    });
    if (promotable(from, c) || promotable(to, c)) && k.promote().is_some() {
        moves.push(Movement::Move {
            from,
            to,
            promote: true,
        })
    }
}

fn line_piece_index(k: Kind) -> Option<usize> {
    Some(match k {
        Kind::Lance => 0,
        Kind::Bishop | Kind::ProBishop => 1,
        Kind::Rook | Kind::ProRook => 2,
        _ => return None,
    })
}

// pinned returns a list of pairs of pinned piece and its movable positions.
// king_pos is the king's position whose color is c.
// For example, if c is black, this method returns black pieces that are not movable
// because the black king is pinned.
pub(super) fn pinned(position: &Position, king_pos: Square, c: Color) -> HashMap<Square, BitBoard> {
    let mut res = HashMap::new();
    for line_piece_kind in Kind::iter() {
        if let Some(i) = line_piece_index(line_piece_kind) {
            for opponent_line_piece in position.bitboard(Some(c.opposite()), Some(line_piece_kind))
            {
                if let Some(pinned_bb) =
                    PIN[opponent_line_piece.index()][king_pos.index()][c.opposite().index()][i]
                {
                    let all_pinned = pinned_bb & (position.bitboard(None, None));
                    if all_pinned.into_iter().count() <= 2 {
                        let mut pinned = pinned_bb & position.bitboard(Some(c), None);
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

pub(super) fn has_pawn_in_col(position: &Position, pos: Square, c: Color) -> bool {
    let b = position.bitboard(Some(c), Some(Kind::Pawn));
    !(COL_MASKS[pos.col()] & b).is_empty()
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
                    for k in [Kind::Lance, Kind::Bishop, Kind::Rook].iter().map(|k|*k) {
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
        position::{
            position_ext::{king, pinned},
            Movement, Position, PositionExt, Square,
        },
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
            pinned(&board, king(&board, Color::Black).unwrap(), Color::Black),
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
            pinned(&board, king(&board, Color::White).unwrap(), Color::White),
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
        ] {
            let board = sfen::decode_position(tc.0).expect(&format!("Failed to decode {}", tc.0));
            let mut res = vec![];
            board.move_candidates(&mut res).unwrap();
            let mut got = res
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
