use crate::{piece::Kind, position::Square};

pub struct Attacker {
    pub pos: Square,
    pub kind: Kind,
    pub double_check: Option<(Square, Kind)>,
}

impl Attacker {
    pub fn new(pos: Square, kind: Kind) -> Self {
        Self {
            pos,
            kind,
            double_check: None,
        }
    }
}
