use rustc_hash::FxHashMap;

use crate::{piece::Color, position::Digest};

use crate::position::Position;

use super::{black, white, AdvanceOptions};

pub fn advance(
    position: &Position,
    memo: &mut FxHashMap<Digest, u32>,
    next_step: u32,
    options: &AdvanceOptions,
) -> anyhow::Result<(Vec<Position>, /* is mate */ bool)> {
    match position.turn() {
        Color::Black => black::advance(position, memo, next_step, options).map(|x| (x, false)),
        Color::White => white::advance(position, memo, next_step, options),
    }
}

pub fn advance_old(position: &Position) -> anyhow::Result<Vec<Position>> {
    match position.turn() {
        Color::Black => black::advance_old(position),
        Color::White => white::advance_old(position),
    }
}

#[cfg(test)]
mod tests {
    use crate::position::{Position, PositionExt};

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
            (
                "2+B6/kPl6/6+b2/1K4+P1+P/+p8/1pG4+R1/p2+n5/1l2+s1s1R/2S1+l1G2 b 2gs3nl12p",
                vec!["7181"],
            ),
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

            let position =
                sfen::decode_position(tc.0).unwrap_or_else(|_| panic!("Failed to decode {}", tc.0));
            let mut got = super::advance_old(&position).unwrap();
            got.sort();

            let mut want = sfen::decode_moves(&tc.1.join(" "))
                .unwrap()
                .iter()
                .map(|m| {
                    let mut b = position.clone();
                    b.do_move(m);
                    b
                })
                .collect::<Vec<Position>>();
            want.sort();
            assert_eq!(got, want);
        }
    }
}
