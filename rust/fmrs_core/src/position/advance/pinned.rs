use crate::{
    piece::Color,
    position::{
        bitboard::{
            bitboard::{between, outside},
            BitBoard,
        },
        controller::PositionController,
        Square,
    },
};

// pinned piece and its movable positions (capturing included) pairs.
#[derive(Debug)]
pub struct Pinned {
    king_pos: Square,
    pinning: BitBoard,
}

impl Default for Pinned {
    fn default() -> Self {
        Self {
            king_pos: Square::S11,
            pinning: BitBoard::default(),
        }
    }
}

impl Pinned {
    pub fn is_unpin_move(&self, source: Square, dest: Square) -> bool {
        if self.pinning.is_empty() {
            return false;
        }
        let outside = outside(self.king_pos, source);
        let Some(pinning) = (outside & self.pinning).next() else {
            return false;
        };
        !between(self.king_pos, pinning).contains(dest)
    }

    // Reachable pinned area including capturing move
    #[inline(never)]
    pub fn pinned_area(&self, source: Square) -> Option<BitBoard> {
        if self.pinning.is_empty() {
            return None;
        }
        let outside = outside(self.king_pos, source);
        let Some(pinning) = (outside & self.pinning).next() else {
            return None;
        };
        between(self.king_pos, pinning).with(pinning).into()
    }

    pub fn exclusive_pinned_areas(&self) -> impl Iterator<Item = BitBoard> + '_ {
        self.pinning
            .map(move |pinning| between(self.king_pos, pinning))
    }
}

pub fn pinned(controller: &mut PositionController, king_color: Color) -> Pinned {
    let Some(king_pos) = controller.king_pos(king_color) else {
        return Pinned {
            king_pos: Square::S11,
            pinning: BitBoard::default(),
        };
    };
    let pinning = controller.pinning(king_color);
    return Pinned { king_pos, pinning };
}
