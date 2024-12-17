use fmrs_core::{
    memo::Memo,
    nohash::NoHashSet,
    piece::Color,
    position::{advance, AdvanceOptions, Position, PositionExt},
};

pub fn one_way_mate_steps(initial_position: &Position) -> Option<usize> {
    if initial_position.turn().is_black() {
        if initial_position.checked_slow(Color::WHITE) {
            return None;
        }
    } else {
        if initial_position.checked_slow(Color::BLACK)
            || !initial_position.checked_slow(Color::WHITE)
        {
            return None;
        }
    }
    let initial_step = if initial_position.turn().is_black() {
        1
    } else {
        0
    };

    let mut position = initial_position.clone();

    let options = AdvanceOptions {
        max_allowed_branches: Some(1),
        no_memo: true,
    };

    let mut seen_positions = NoHashSet::default();

    let mut unused_memo = Memo::default();

    let mut black_movements = vec![];
    let mut white_movements = vec![];

    for step in (initial_step..).step_by(2) {
        if step > 0 {
            advance(
                &mut position,
                &mut unused_memo,
                0,
                &options,
                &mut black_movements,
            )
            .ok()?;
            debug_assert!(black_movements.len() <= 2);
            if black_movements.len() == 0 {
                return None;
            }
            if black_movements.len() == 2 {
                if black_movements[0].is_pawn_drop() {
                    black_movements.swap(0, 1);
                }
                debug_assert!(black_movements[1].is_pawn_drop());
                let pawn_move = black_movements.remove(1);
                let undo = position.do_move(&pawn_move);
                advance(
                    &mut position,
                    &mut unused_memo,
                    0,
                    &options,
                    &mut white_movements,
                )
                .ok()?;
                if !white_movements.is_empty() {
                    return None;
                }
                position.undo_move(&undo);
            }

            if black_movements.len() != 1 {
                return None;
            }

            position.do_move(&black_movements.remove(0));
        }

        assert!(position.turn().is_white());
        let is_mate = advance(
            &mut position,
            &mut unused_memo,
            0,
            &options,
            &mut white_movements,
        )
        .ok()?;

        if is_mate {
            if !position.hands().is_empty(Color::BLACK) {
                return None;
            }
            return (step as usize).into();
        }

        if white_movements.len() != 1 {
            return None;
        }

        debug_assert_eq!(white_movements.len(), 1);

        position.do_move(&white_movements.remove(0));

        if step > 60 {
            // Avoid perpetual check
            let digest = position.digest();
            if seen_positions.contains(&digest) {
                return None;
            }
            seen_positions.insert(digest);
        }
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
    fn oneway43() {
        let position = Position::from_sfen(
            "sg7/1b1S3+P1/pNPp4g/S+P1sgpppB/1L2p2P+p/1l3RK1k/P1pGP1+p1p/1PNNN3P/RL6L b -",
        )
        .unwrap();
        assert_eq!(one_way_mate_steps(&position), Some(43));
    }

    #[test]
    fn oneway_from_white() {
        for (sfen, step) in [
            (
                "sg7/1b1S3+P1/pNPp4g/S+P1sgpppB/1L2p2P+p/1l3R2k/P1pGP1K1p/1PNNN3P/RL6L w P",
                42,
            ),
            (
                "4lk+P2/5n3/S2Pp4/1SG2pL1G/2SL4S/1N1p1l1B1/B4NR2/3g1K1p1/PNP1P3P w Prg7p",
                54,
            ),
        ] {
            let position = Position::from_sfen(sfen).unwrap();
            assert_eq!(position.turn(), Color::WHITE);
            assert_eq!(one_way_mate_steps(&position), Some(step));
        }
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
