pub mod v0;

use crate::{
    config::CONFIG,
    piece::Color,
    position::{position::PositionAux, BitBoard, Square},
};

pub fn pinned(position: &mut PositionAux, king_color: Color, blocker_color: Color) -> Pinned {
    const F: fn(&mut PositionAux, Color, Color) -> Pinned = [v0][CONFIG.pinned_v];
    F(position, king_color, blocker_color)
}

fn v0(position: &mut PositionAux, king_color: Color, blocker_color: Color) -> Pinned {
    Pinned::new(Some(Box::new(v0::pinned(
        position,
        king_color,
        blocker_color,
    ))))
}

#[derive(Default)]
pub struct Pinned {
    inner: Option<Box<dyn PinnedTrait>>,
}

impl Pinned {
    fn new(inner: Option<Box<dyn PinnedTrait>>) -> Self {
        Self { inner }
    }
}

impl PinnedTrait for Pinned {
    fn is_unpin_move(&self, source: Square, dest: Square) -> bool {
        self.inner
            .as_ref()
            .map(|x| x.is_unpin_move(source, dest))
            .unwrap_or(false)
    }

    fn pinned_area(&self, source: Square) -> Option<BitBoard> {
        self.inner.as_ref()?.pinned_area(source)
    }

    fn iter(&self) -> Box<dyn Iterator<Item = (Square, BitBoard)> + '_> {
        self.inner
            .as_ref()
            .map(|x| x.iter())
            .unwrap_or(Box::new(std::iter::empty()))
    }
}

pub trait PinnedTrait {
    fn is_unpin_move(&self, source: Square, dest: Square) -> bool;
    // Reachable pinned area including capturing move
    fn pinned_area(&self, source: Square) -> Option<BitBoard>;

    fn iter(&self) -> Box<dyn Iterator<Item = (Square, BitBoard)> + '_>;
}
