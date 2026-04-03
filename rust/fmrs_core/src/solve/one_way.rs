use crate::{
    nohash::NoHashSet64,
    piece::Color,
    position::{advance::advance::advance_aux, position::PositionAux, AdvanceOptions, Movement},
};

pub fn one_way_mate_steps(
    position: &mut PositionAux,
    movements: &mut Vec<Movement>,
) -> Result<usize, usize> {
    if position.turn().is_black() {
        if position.checked_slow(Color::WHITE) {
            return Err(0);
        }
    } else if position.checked_slow(Color::BLACK) {
        return Err(0);
    }
    let mut orig = None;
    let res = one_way_mate_steps_inner(position, movements, &mut orig);
    if let Some(orig) = orig {
        *position = orig;
    }
    res
}

fn one_way_mate_steps_inner(
    position: &mut PositionAux,
    movements: &mut Vec<Movement>,
    orig: &mut Option<PositionAux>,
) -> Result<usize, usize> {
    let initial_step = if position.turn().is_black() { 1 } else { 0 };

    let options = AdvanceOptions {
        max_allowed_branches: Some(1),
    };

    let mut seen_positions = NoHashSet64::default();

    for step in (initial_step..).step_by(2) {
        if step > 0 {
            let prev_len = movements.len();
            if advance_aux(position, &options, movements).is_err() {
                return Err(step);
            }
            debug_assert!(movements.len() - prev_len <= 2);
            if movements.len() == prev_len {
                return Err(step);
            }
            if movements.len() == prev_len + 2 {
                if movements[movements.len() - 2].is_pawn_drop() {
                    movements.swap(prev_len, prev_len + 1);
                }
                debug_assert!(movements[movements.len() - 1].is_pawn_drop());
                let pawn_move = movements.pop().unwrap();

                orig.get_or_insert_with(|| position.clone());
                let prev = position.clone();
                position.do_move(&pawn_move);
                if advance_aux(position, &options, movements).is_err() {
                    return Err(step);
                }
                if movements.len() != prev_len + 1 {
                    return Err(step);
                }
                *position = prev;
            } else if movements.len() != prev_len + 1 {
                return Err(step);
            }

            orig.get_or_insert_with(|| position.clone());
            position.do_move(movements.last().unwrap());
        }

        assert!(position.turn().is_white(), "{:?}", position);

        let prev_len = movements.len();
        let is_mate = match advance_aux(position, &options, movements) {
            Ok(x) => x,
            Err(_) => return Err(step + 1),
        };

        if is_mate {
            if !position.hands().is_empty(Color::BLACK) {
                return Err(step + 1);
            }
            return Ok(step as usize);
        }

        if movements.len() != prev_len + 1 {
            return Err(step + 1);
        }

        debug_assert_eq!(movements.len(), prev_len + 1);

        orig.get_or_insert_with(|| position.clone());
        position.do_move(movements.last().unwrap());

        if step > 60 {
            // Avoid perpetual check
            let digest = position.digest();
            if seen_positions.contains(&digest) {
                return Err(step + 1);
            }
            seen_positions.insert(digest);
        }
    }
    unreachable!();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oneway43() {
        let mut position = PositionAux::from_sfen(
            "sg7/1b1S3+P1/pNPp4g/S+P1sgpppB/1L2p2P+p/1l3RK1k/P1pGP1+p1p/1PNNN3P/RL6L b -",
        )
        .unwrap();
        assert_eq!(one_way_mate_steps(&mut position, &mut vec![]), Ok(43));
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
            let mut position = PositionAux::from_sfen(sfen).unwrap();
            assert_eq!(position.turn(), Color::WHITE);
            assert_eq!(one_way_mate_steps(&mut position, &mut vec![]), Ok(step));
        }
    }

    #[test]
    fn test_diamond() {
        let mut position =
            PositionAux::from_sfen(include_str!("../../../problems/diamond.sfen")).unwrap();
        assert_eq!(one_way_mate_steps(&mut position, &mut vec![]), Ok(55));
    }
}