use anyhow::{anyhow, Result};

use crate::{
    piece::{Color, EssentialKind, Kind},
    position::{
        bitboard::{self, BitBoard},
        Position, Square,
    },
};

use super::attacker::Attacker;

pub struct StateInfo<'a> {
    pub position: &'a Position,
    pub white_king_pos: Square,
    pub black_king_pos: Option<Square>,

    attack_squares: Vec<Option<BitBoard>>,
    // pieces: Vec<Option<Option<(Color, Kind)>>>,
    // power_of: Vec<Option<BitBoard>>,
}

impl<'a> StateInfo<'a> {
    pub fn new(position: &'a Position) -> Result<Self> {
        let white_bb = position.color_bb().bitboard(Color::White);
        let black_bb = position.color_bb().bitboard(Color::Black);
        let king_bb = position.kind_bb().bitboard(Kind::King);

        let white_king_pos = (king_bb & white_bb)
            .next()
            .ok_or(anyhow!("no white king"))?;
        let black_king_pos = (king_bb & black_bb).next();

        Ok(Self {
            position,
            white_king_pos,
            black_king_pos,
            attack_squares: vec![Default::default(); 10],
            // pieces: vec![Default::default(); 81],
            // power_of: vec![Default::default(); 81],
        })
    }

    // Returns a bitboard. A square exists in the bitboard iff moving of a black piece of the given kind
    // to the square (possibly by capturing a white piece) produces a check to the white king.
    pub fn attack_squares(&mut self, kind: EssentialKind) -> BitBoard {
        *self.attack_squares[kind.index()].get_or_insert_with(|| {
            bitboard::reachable(
                self.position.color_bb(),
                Color::White,
                self.white_king_pos,
                kind,
                true,
            )
        })
    }

    pub fn get(&self, pos: Square) -> Option<(Color, Kind)> {
        self.position.get(pos)
        // *self.pieces[pos.index()].get_or_insert_with(|| self.position.get(pos))
    }

    pub fn power_of(&mut self, pos: Square) -> BitBoard {
        // if let Some(res) = self.power_of[pos.index()] {
        //     return res;
        // }
        let (color, kind) = self.get(pos).unwrap();
        let res = bitboard::power(color, pos, kind.to_essential_kind());
        // self.power_of[pos.index()] = Some(res);
        res
    }

    pub fn attacker(&self, king_color: Color, check_double: bool) -> Option<Attacker> {
        let king_pos = match king_color {
            Color::Black => match self.black_king_pos {
                Some(king_pos) => king_pos,
                None => return None,
            },
            Color::White => self.white_king_pos,
        };

        let color_bb = self.position.color_bb().bitboard(king_color.opposite());
        let kind_bb = self.position.kind_bb();

        let mut attacker: Option<Attacker> = None;

        for attacker_kind in Kind::iter() {
            let existing = color_bb & kind_bb.bitboard(attacker_kind);
            if existing.is_empty() {
                continue;
            }
            // TODO: consider checking power first.
            let attacking = bitboard::reachable(
                self.position.color_bb(),
                king_color,
                king_pos,
                attacker_kind.to_essential_kind(),
                false,
            ) & existing;
            if attacking.is_empty() {
                continue;
            }
            for attacker_pos in attacking {
                if let Some(mut attacker) = attacker.take() {
                    attacker.double_check = Some((attacker_pos, attacker_kind));
                    return Some(attacker);
                }
                attacker = Some(Attacker::new(attacker_pos, attacker_kind));
                if !check_double {
                    return attacker;
                }
            }
        }
        attacker
    }
}
