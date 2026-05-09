use crate::position::advance::options::AdvanceResult;
use crate::position::position::PositionAux;
use crate::position::Movement;

use super::{black, white, AdvanceOptions};

pub fn advance_aux(
    position: &mut PositionAux,
    options: &AdvanceOptions,
    result: &mut Vec<Movement>,
) -> AdvanceResult</* is legal mate */ bool> {
    if position.turn().is_black() {
        black::advance(position, options, result)?;
        Ok(false)
    } else {
        white::advance(position, options, result)
    }
}

#[cfg(test)]
mod tests {
    use crate::position::AdvanceOptions;

    /// Move-count regression test for the positions exercised by
    /// `benches/bench.rs::bench_black_advance` and `bench_white_advance`.
    /// These are the same positions used as performance baselines, so locking
    /// in the expected move counts here catches changes to advance generation
    /// (e.g. dedup logic, pinned filters) without needing to run cargo bench.
    #[test]
    fn advance_count_for_bench_positions() {
        use crate::sfen;

        // black side: rust/problems/ofm-139_5.sfen content embedded.
        let black_position =
            "B+l+pn1+pR+p1/+lR7/3+p+p+pB+p1/2+p1+p4/3+p1+p1+p+l/2n1+p2+p1/3+p+p1k1g/7s1/3gs1+p2 b GSNgsnlp 1";
        let white_positions: &[(&str, usize)] = &[
            (
                "B+l+pn1+pR+p1/+lR7/3+p+p+p1+p1/2+p1+p4/3+p1+p1+p+l/2n+B+p2+p1/3+p+p1k1g/7s1/3gs1+p2 w GSNgsnlp 1",
                42,
            ),
            (
                "B+l+pn1+pR+p1/+l8/3+p+p+pB+p1/2+p1+p4/3+p1+p1+p+l/2n1+p2+p1/1+R1+p+p1k1g/7s1/3gs1+p2 w GSNgsnlp 1",
                49,
            ),
            (
                "B+l+pn1+pR+p1/+lR7/3+p+p+pB+p1/2+p1+p4/3+p1+p1+p+l/2n1+p2+p1/3+p+p1k1g/7s1/3gs1+pN1 w GSgsnlp 1",
                9,
            ),
        ];

        let mut position = sfen::decode_position(black_position).unwrap();
        let mut result = vec![];
        super::advance_aux(&mut position, &AdvanceOptions::default(), &mut result).unwrap();
        assert_eq!(
            result.len(),
            66,
            "black bench position (ofm-139_5) move count regression"
        );

        for &(sfen_str, want) in white_positions {
            let mut position = sfen::decode_position(sfen_str).unwrap();
            let mut result = vec![];
            super::advance_aux(&mut position, &AdvanceOptions::default(), &mut result).unwrap();
            assert_eq!(
                result.len(),
                want,
                "white bench position move count regression: {}",
                sfen_str
            );
        }
    }

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
            super::advance_aux(&mut position, &AdvanceOptions::default(), &mut got).unwrap();
            got.sort();

            let mut want = sfen::decode_moves(&tc.1.join(" ")).unwrap();
            want.sort();
            assert_eq!(got, want);
        }
    }
}
