use crate::memo::MemoTrait;
use crate::position::position::PositionAux;
use crate::position::Movement;

use crate::position::Position;

use super::{black, white, AdvanceOptions};

pub fn advance<M: MemoTrait>(
    position: &Position,
    memo: &mut M,
    next_step: u16,
    options: &AdvanceOptions,
    result: &mut Vec<Movement>,
) -> anyhow::Result</* is legal mate */ bool> {
    let mut position = PositionAux::new(position.clone());
    if position.turn().is_black() {
        black::advance(&mut position, memo, next_step, options, result)?;
        Ok(false)
    } else {
        white::advance(&mut position, memo, next_step, options, result)
    }
}

pub fn advance_aux<M: MemoTrait>(
    position: &mut PositionAux,
    memo: &mut M,
    next_step: u16,
    options: &AdvanceOptions,
    result: &mut Vec<Movement>,
) -> anyhow::Result</* is legal mate */ bool> {
    if position.turn().is_black() {
        black::advance(position, memo, next_step, options, result)?;
        Ok(false)
    } else {
        white::advance(position, memo, next_step, options, result)
    }
}

#[cfg(test)]
mod tests {
    use crate::{memo::Memo, position::AdvanceOptions};

    #[test]
    fn advance() {
        use crate::sfen;
        use pretty_assertions::assert_eq;
        for tc in vec![
            // Black moves
            (
                "8k/9/9/9/9/9/9/9/9 b P2r2b4g4s4n4l17p 1",
                // Drop pawn
                vec!["P*12"],
            ),
            (
                "9/9/5lp2/5lk2/5l3/9/5N3/7L1/9 b P2r2b4g4s3n16p 1",
                // Drop pawn mate is not checked here
                vec!["P*35"],
            ),
            (
                "8k/9/8K/9/9/9/9/9/9 b 2r2b4g4s4n4l18p 1",
                // King cannot attack
                vec![],
            ),
            (
                "4R4/9/4P4/9/4k1P1R/9/2N1s4/1B7/4L4 b b4g3s3n3l16p 1",
                // Discovered attacks
                vec!["7785", "7765", "5957", "3534"],
            ),
            ("6k1l/8B/8K/9/9/9/9/9/9 b 2rb4g4s4n3l18p 1", vec![]),
            ("9/9/9/9/9/k8/n1P6/LB7/9 b P2rb4g4s3n3l15p", vec!["9897"]),
            ("9/9/9/9/9/5bk2/9/6P2/8K b 2rb4g4s4n4l17p 1", vec!["3837"]),
            // White moves
            (
                "3pks3/4+B4/4+P4/9/9/9/9/9/9 w S2rb4g2s4n4l16p 1",
                vec!["4152"],
            ),
            (
                "3+pk4/5S3/9/9/9/8B/9/9/9 w 2rb4g2s4n4l17p",
                vec!["5142", "5162"],
            ),
            ("7br/5ssss/5gggg/9/9/B8/1n1K5/9/R2k5 w 3n4l18p 1", vec![]),
            ("k8/1+R7/9/9/9/9/9/9/9 w r2b4g4s4n4l18p 1", vec!["9182"]),
            ("bpgg5/ssgg5/ss7/9/9/+R8/9/+b8/k1+R6 w 4n4l17p 1", vec![]),
        ] {
            eprintln!("{}", tc.0);

            let mut position =
                sfen::decode_position(tc.0).unwrap_or_else(|_| panic!("Failed to decode {}", tc.0));
            let mut got = vec![];
            super::advance(
                &mut position,
                &mut Memo::default(),
                1,
                &AdvanceOptions::default(),
                &mut got,
            )
            .unwrap();
            got.sort();

            let mut want = sfen::decode_moves(&tc.1.join(" ")).unwrap();
            want.sort();
            assert_eq!(got, want);
        }
    }
}
