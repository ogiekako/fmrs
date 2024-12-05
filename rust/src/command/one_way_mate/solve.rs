use fmrs_core::{
    piece::Color,
    position::{advance, checked, Position},
};
use nohash_hasher::{IntMap, IntSet};

pub fn one_way_mate_steps(mut position: Position) -> Option<usize> {
    if checked(&position, Color::White) {
        return None;
    }

    let mut visited = IntSet::default();

    // TODO: `advance` without cache.
    for step in (1..).step_by(2) {
        let (white_positions, _) = advance(&position, &mut IntMap::default(), step).unwrap();
        if white_positions.len() != 1 {
            return None;
        }

        let (mut black_positions, is_mate) =
            advance(&white_positions[0], &mut IntMap::default(), step + 1).unwrap();

        if is_mate && !white_positions[0].pawn_drop() {
            if !white_positions[0].hands().is_empty(Color::Black) {
                return None;
            }
            return (step as usize).into();
        }

        if black_positions.len() != 1 {
            return None;
        }

        if !visited.insert(position.digest()) {
            return None;
        }

        position = black_positions.remove(0);
    }
    unreachable!();
}
