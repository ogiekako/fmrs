use fmrs_core::{
    nohash::{NoHashMap, NoHashSet},
    piece::Color,
    position::{advance, checked, AdvanceOptions, Position},
};

pub fn one_way_mate_steps(position: &Position) -> Option<usize> {
    let mut position = position.clone();
    if checked(&position, Color::WHITE) {
        return None;
    }

    let mut visited = NoHashSet::default();

    let options = {
        let mut options = AdvanceOptions::default();
        options.max_allowed_branches = Some(1);
        options
    };

    let mut hashmap = NoHashMap::default();

    // TODO: `advance` without cache.
    for step in (1..).step_by(2) {
        hashmap.clear();
        let (white_positions, _) = advance(&position, &mut hashmap, step, &options).ok()?;
        debug_assert!(white_positions.len() <= 1);
        if white_positions.len() != 1 {
            return None;
        }

        hashmap.clear();
        let (mut black_positions, is_mate) =
            advance(&white_positions[0], &mut hashmap, step + 1, &options).ok()?;

        if is_mate && !white_positions[0].pawn_drop() {
            if !white_positions[0].hands().is_empty(Color::BLACK) {
                return None;
            }
            return (step as usize).into();
        }

        debug_assert_eq!(black_positions.len(), 1);

        if !visited.insert(position.digest()) {
            return None;
        }

        position = black_positions.remove(0);
    }
    unreachable!();
}

#[cfg(test)]
mod tests {
    use rand::{rngs::SmallRng, Rng, SeedableRng};

    use super::*;
    use crate::command::one_way_mate::action::Action;

    #[test]
    fn test_one_way_mate_steps() {
        let mut rng = SmallRng::seed_from_u64(0);

        let mut got: Vec<usize> = vec![];
        for _ in 0..3 {
            let mut sum_steps = 0;
            let mut position =
                Position::from_sfen("4k4/9/9/9/9/9/9/9/4K4 b 2r2b4g4s4n4l18p 1").unwrap();

            for _ in 0..1_000_000 {
                let action = random_action(&mut rng);
                if action.try_apply(&mut position).is_err() {
                    continue;
                }
                if let Some(steps) = one_way_mate_steps(&position) {
                    sum_steps += steps;
                }
            }
            got.push(sum_steps);
        }
        assert_eq!(got, vec![33, 22, 54]);
    }

    #[test]
    fn test_diamond() {
        let position = Position::from_sfen(include_str!("../../../problems/diamond.sfen")).unwrap();
        assert_eq!(one_way_mate_steps(&position), Some(55));
    }

    fn random_action(rng: &mut SmallRng) -> Action {
        loop {
            match rng.gen_range(0..100) {
                0..=9 => return Action::Move(rng.gen(), rng.gen()),
                10..=19 => return Action::Swap(rng.gen(), rng.gen()),
                20..=29 => return Action::FromHand(rng.gen(), rng.gen(), rng.gen(), rng.gen()),
                30..=39 => return Action::ToHand(rng.gen(), Color::WHITE),
                40..=49 => return Action::Shift(rng.gen()),
                _ => (),
            }
        }
    }
}
