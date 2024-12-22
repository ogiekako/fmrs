use fmrs_core::{
    memo::Memo,
    nohash::NoHashSet,
    piece::Color,
    position::{
        advance::advance::advance_aux, position::PositionAux, AdvanceOptions, Movement, Position,
    },
};

pub fn one_way_mate_steps(
    initial_position: &Position,
    movements: &mut Vec<Movement>,
) -> Option<usize> {
    let mut position = PositionAux::new(initial_position.clone());
    if initial_position.turn().is_black() {
        if position.checked_slow(Color::WHITE) {
            return None;
        }
    } else {
        if position.checked_slow(Color::BLACK) {
            return None;
        }
    }
    let initial_step = if position.turn().is_black() { 1 } else { 0 };

    let options = AdvanceOptions {
        max_allowed_branches: Some(1),
        no_memo: true,
    };

    let mut seen_positions = NoHashSet::default();

    let mut unused_memo = Memo::default();

    for step in (initial_step..).step_by(2) {
        if step > 0 {
            let prev_len = movements.len();
            advance_aux(&mut position, &mut unused_memo, 0, &options, movements).ok()?;
            debug_assert!(movements.len() - prev_len <= 2);
            if movements.len() == prev_len {
                return None;
            }
            if movements.len() == prev_len + 2 {
                if movements[movements.len() - 2].is_pawn_drop() {
                    movements.swap(prev_len, prev_len + 1);
                }
                debug_assert!(movements[movements.len() - 1].is_pawn_drop());
                let pawn_move = movements.pop().unwrap();

                let orig = position.clone();
                position.do_move(&pawn_move);
                advance_aux(&mut position, &mut unused_memo, 0, &options, movements).ok()?;
                if movements.len() != prev_len + 1 {
                    return None;
                }
                position = orig;
            } else if movements.len() != prev_len + 1 {
                return None;
            }

            position.do_move(&movements.last().unwrap());
        }

        assert!(position.turn().is_white(), "{:?}", position);

        let prev_len = movements.len();
        let is_mate = advance_aux(&mut position, &mut unused_memo, 0, &options, movements).ok()?;

        if is_mate {
            if !position.hands().is_empty(Color::BLACK) {
                return None;
            }
            return (step as usize).into();
        }

        if movements.len() != prev_len + 1 {
            return None;
        }

        debug_assert_eq!(movements.len(), prev_len + 1);

        position.do_move(&movements.last().unwrap());

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
                if let Some(steps) = one_way_mate_steps(&position, &mut vec![]) {
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
        assert_eq!(one_way_mate_steps(&position, &mut vec![]), Some(43));
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
            assert_eq!(one_way_mate_steps(&position, &mut vec![]), Some(step));
        }
    }

    #[test]
    fn test_diamond() {
        let position = Position::from_sfen(include_str!("../../../problems/diamond.sfen")).unwrap();
        assert_eq!(one_way_mate_steps(&position, &mut vec![]), Some(55));
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
