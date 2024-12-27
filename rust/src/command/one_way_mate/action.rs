use anyhow::bail;
use fmrs_core::{
    direction::Direction,
    piece::{Color, Kind},
    position::{position::PositionAux, Square},
};

#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(unused)]
pub enum Action {
    Move(Square, Square),
    Swap(Square, Square),
    FromHand(Color, Square, Color, Kind), // drop to an empty square
    ToHand(Square, Color),                // move a piece to hand
    TwoActions(Box<Action>, Box<Action>),
    Shift(Direction),
    ChangeTurn,
    HandToHand(Color, Kind),
}

impl Action {
    pub(super) fn try_apply(self, position: &mut PositionAux) -> anyhow::Result<Action> {
        match self {
            Action::Move(from, to) => {
                if let Some((color, kind)) = position.get(from) {
                    if position.get(to).is_some() {
                        bail!("to is not empty");
                    }
                    position.unset(from, color, kind);
                    position.set(to, color, kind);
                    Ok(Action::Move(to, from))
                } else {
                    bail!("from is empty");
                }
            }
            Action::Swap(a, b) => {
                if a == b {
                    return Ok(Action::Swap(a, b));
                }
                match (position.get(a), position.get(b)) {
                    (None, None) => bail!("both are None"),
                    (None, Some((b_color, b_kind))) => {
                        position.unset(b, b_color, b_kind);
                        position.set(a, b_color, b_kind);
                    }
                    (Some((a_color, a_kind)), None) => {
                        position.unset(a, a_color, a_kind);
                        position.set(b, a_color, a_kind);
                    }
                    (Some((a_color, a_kind)), Some((b_color, b_kind))) => {
                        position.unset(a, a_color, a_kind);
                        position.unset(b, b_color, b_kind);
                        position.set(a, b_color, b_kind);
                        position.set(b, a_color, a_kind);
                    }
                }
                Ok(Action::Swap(a, b))
            }
            Action::FromHand(hand_color, pos, color, kind) => {
                if kind == Kind::King {
                    bail!("cannot drop king");
                }
                if position.get(pos).is_some() {
                    bail!("to is not empty");
                }
                let hand_kind = kind.maybe_unpromote();
                if !position.hands().has(hand_color, hand_kind) {
                    bail!("no piece in hand");
                }
                position.hand_remove(hand_color, hand_kind);
                position.set(pos, color, kind);
                Ok(Action::ToHand(pos, hand_color))
            }
            Action::ToHand(pos, hand_color) => {
                if let Some((color, kind)) = position.get(pos) {
                    if kind == Kind::King {
                        bail!("cannot take king");
                    }
                    let hand_kind = kind.maybe_unpromote();
                    position.hand_add(hand_color, hand_kind);
                    position.unset(pos, color, kind);
                    Ok(Action::FromHand(hand_color, pos, color, kind))
                } else {
                    bail!("from is empty");
                }
            }
            Action::TwoActions(a, b) => {
                let undo_a = a.try_apply(position)?;
                match b.try_apply(position) {
                    Ok(undo_b) => Ok(Action::TwoActions(Box::new(undo_b), Box::new(undo_a))),
                    Err(e) => {
                        undo_a.try_apply(position).unwrap();
                        Err(e)
                    }
                }
            }
            Action::Shift(dir) => {
                position.shift(dir);
                Ok(Action::Shift(dir.opposite()))
            }
            Action::ChangeTurn => {
                position.set_turn(position.turn().opposite());
                Ok(Action::ChangeTurn)
            }
            Action::HandToHand(color, kind) => {
                if kind == Kind::King {
                    bail!("cannot take king");
                }
                let hand_kind = kind.maybe_unpromote();
                if !position.hands().has(color, hand_kind) {
                    bail!("no piece in hand");
                }
                position.hand_remove(color, hand_kind);
                position.hand_add(color.opposite(), hand_kind);
                Ok(Action::HandToHand(color.opposite(), hand_kind))
            }
        }
    }
}
