use crate::solver::db_parallel_solve;
use crate::solver::memory_save_solve;
use crate::solver::parallel_solve;
use crate::solver::standard_solve;
use fmrs_core::piece::*;
use fmrs_core::position::Position;
use fmrs_core::position::PositionExt;
use fmrs_core::solve::Solution;

#[derive(Debug, Clone, PartialEq, Eq, clap::ValueEnum)]
pub enum Algorithm {
    MemorySave,
    Standard,
    Parallel,
    DbParallel,
}

impl Algorithm {
    #[cfg(test)]
    fn iter() -> impl Iterator<Item = Algorithm> {
        [
            Algorithm::MemorySave,
            Algorithm::Standard,
            Algorithm::Parallel,
            Algorithm::DbParallel,
        ]
        .into_iter()
    }
}

pub fn solve(
    board: Position,
    solution_upto: Option<usize>,
    algorithm: Algorithm,
) -> anyhow::Result<Vec<Solution>> {
    let (tx, _rx) = futures::channel::mpsc::unbounded();
    solve_with_progress(tx, board, solution_upto, algorithm)
}

pub fn solve_with_progress(
    progress: futures::channel::mpsc::UnboundedSender<usize>,
    position: Position,
    solutions_upto: Option<usize>,
    algorithm: Algorithm,
) -> anyhow::Result<Vec<Solution>> {
    if position.turn() != Color::BLACK {
        anyhow::bail!("The turn should be from black");
    }
    if position.checked_slow(Color::WHITE) {
        anyhow::bail!("on black's turn, white is already checked.");
    }
    debug_assert_ne!(
        position.turn() == Color::BLACK,
        position.checked_slow(Color::WHITE)
    );

    let solutions_upto = solutions_upto.unwrap_or(usize::MAX);
    match algorithm {
        Algorithm::MemorySave => memory_save_solve::solve(position, progress, solutions_upto),
        Algorithm::Parallel => parallel_solve::solve(position, progress, solutions_upto),
        Algorithm::DbParallel => db_parallel_solve::solve(position, progress, solutions_upto),
        Algorithm::Standard => standard_solve::solve(position, solutions_upto),
    }
}

#[cfg(test)]
mod tests {
    use solve::Algorithm;

    use crate::solver::solve;
    use fmrs_core::sfen;
    use fmrs_core::{position::Movement, solve::Solution};

    #[test]
    fn test_solve() {
        for tc in vec![
            (
                // http://sfenreader.appspot.com/sfen?sfen=3%2Bpks3%2F9%2F4%2BP4%2F9%2F9%2F8B%2F9%2F9%2F9%20b%20S2rb4g2s4n4l16p%201
                "3+pks3/9/4+P4/9/9/8B/9/9/9 b S2rb4g2s4n4l16p 1",
                vec!["1f5b+ 4a5b S*4b"],
            ),
            (
                "9/9/6+R2/3bk4/9/3+R5/9/9/9 b B4g4s4n4l18p 1",
                vec!["B*63"]
            ),
            (
                "9/9/3k5/3b1+R3/9/3+R5/9/9/9 b B4g4s4n4l18p 1",
                vec!["4433 6354 B*63"]
            ),
            (
                // http://sfenreader.appspot.com/sfen?sfen=9%2F9%2F9%2F3bkb3%2F9%2F3%2BR1%2BR3%2F9%2F9%2F9%20b%204g4s4n4l18p%201
                "9/9/9/3bkb3/9/3+R1+R3/9/9/9 b 4g4s4n4l18p 1",
                vec!["4644 5463 4433 6354 B*63", "6664 5443 6473 4354 B*43"],
            ),
            (
                // http://sfenreader.appspot.com/sfen?sfen=9%2F6b2%2F7k1%2F5b3%2F7sL%2F9%2F9%2F9%2F9%20b%20Rr4g3s4n3l18p%201
                "9/6b2/7k1/5b3/7sL/9/9/9/9 b Rr4g3s4n3l18p 1",
                vec!["1513+ 2324 1323 2414 R*13"],
            ),
            // http://sfenreader.appspot.com/sfen?sfen=9%2F9%2F9%2F9%2F9%2F9%2F9%2F5G2l%2FK4%2BB1kr%20b%20rb3g4s4n3l18p%201
            ("9/9/9/9/9/9/9/5G2l/K4+B1kr b rb3g4s4n3l18p", vec!["4938"]),
            (
                // http://sfenreader.appspot.com/sfen?sfen=7nk%2F9%2F9%2F9%2F8%2BB%2F9%2F9%2F9%2F8L%20b%202rb4g4s3n3l18p%201
                // Double check should not be counted twice.
                "7nk/9/9/9/8+B/9/9/9/8L b 2rb4g4s3n3l18p 1",
                vec!["1533"],
            ),
            (
                // http://sfenreader.appspot.com/sfen?sfen=7br%2F5ssss%2F5gggg%2F9%2F9%2FB8%2FRn1K5%2F9%2F3k5%20b%203n4l18p%201
                "7br/5ssss/5gggg/9/9/B8/Rn1K5/9/3k5 b 3n4l18p 1",
                vec!["9799"],
            ),
            // http://sfenreader.appspot.com/sfen?sfen=9%2F6K1k%2F9%2F9%2F9%2F8N%2F8R%2F9%2F9%20b%20r2b4g4s3n4l18p%201
            ("9/6K1k/9/9/9/8N/8R/9/9 b r2b4g4s3n4l18p 1", vec!["1624"]),
            (
                // http://sfenreader.appspot.com/sfen?sfen=9%2F8P%2F9%2F6K1k%2F1lll5%2F1nnn5%2Fssss3N1%2F1ggg3gp%2Fbbrr4L%20b%2016p%201
                "9/8P/9/6K1k/1lll5/1nnn5/ssss3N1/1ggg3gp/bbrr4L b 16p 1",
                vec!["1918 P*15 1815"],
            ),
            (
                // http://sfenreader.appspot.com/sfen?sfen=6sgr%2F6sgb%2F6sgb%2F6sg1%2F9%2F9%2F9%2FR1Kppp3%2F4k4%20b%204n4l15p%201
                "6sgr/6sgb/6sgb/6sg1/9/9/9/R1Kppp3/4k4 b 4n4l15p 1",
                vec!["9899 6869+ 9969"],
            ),
            (
                // http://sfenreader.appspot.com/sfen?sfen=k8%2Fl8%2F9%2F9%2F9%2F9%2F9%2FL8%2FK6Br%20b%20rb4g4s4n2l18p%201
                "k8/l8/9/9/9/9/9/L8/K6Br b rb4g4s4n2l18p 1",
                vec!["9892+"],
            ),
            (
                // http://sfenreader.appspot.com/sfen?sfen=k8%2Fl8%2F9%2F9%2F9%2F9%2F9%2FL8%2FK6Br%20b%20rb4g4s4n2l18p%201
                // Capturing a pinning piece (lance)
                "k8/l8/9/9/9/9/9/L8/K6Br b rb4g4s4n2l18p 1",
                vec!["9892+"],
            ),
            (
                // http://sfenreader.appspot.com/sfen?sfen=8k%2F7b1%2F9%2F9%2F9%2F9%2F9%2F1B7%2FK6Lr%20b%20r4g4s4n3l18p%201
                // Capturing a pinning piece (bishop)
                "8k/7b1/9/9/9/9/9/1B7/K6Lr b r4g4s4n3l18p 1",
                vec!["8822+"],
            ),
            (
                // http://sfenreader.appspot.com/sfen?sfen=3ppppp1%2F9%2F2B6%2FK2%2BP4r%2F9%2Fllll5%2Fnnnn5%2Fbggs4s%2Frggs3sk%20b%2012p%201
                // Discovered check with moving a pinned piece
                "3ppppp1/9/2B6/K2+P4r/9/llll5/nnnn5/bggs4s/rggs3sk b 12p 1",
                vec!["6454"],
            ),
            (
                // http://sfenreader.appspot.com/sfen?sfen=9%2FG2s4G%2FLLpNGNpPP%2F4L4%2F1sN3N2%2F1g1bpb1ss%2F1pL3R2%2FP4k3%2F1PPPP3K%20b%202Pr5p%201
                "9/G2s4G/LLpNGNpPP/4L4/1sN3N2/1g1bpb1ss/1pL3R2/P4k3/1PPPP3K b 2Pr5p 1",
                vec!["P*4i 4h3g P*3h 3g3f 3h3g 3f4g 4i4h 4g5g 5i5h 5g6g 6i6h 6g7g 7i7h 7g7f 7h7g 7f7e 7g7f 7e7d 7f7e 7d8c 9b8b 8c8b 9c9b+ 8b7b 9b8b 7b6a 5c5b"],
            ),
            #[cfg(not(debug_assertions))]
            // http://cavesfairy.g1.xrea.com/pub/qgfairy/
            (
                // 06-07 (57 steps)
                "9/9/9/9/7bb/1ppssssp1/K5k2/+RL1l1gg2/rL3gg1+l b 4N15P 1",
                vec!["N*29 3829 P*38 3747 N*59 4859 P*48 4757 N*69 6869+ P*58 5767 P*68 6778 P*79 6979 8886 7877 P*78 7768 P*69 5969 7877 6867 P*68 6758 P*59 4959 6867 5857 P*58 5748 P*49 3949 5857 4847 P*48 4738 P*39 2939 4847 3837 P*38 3728 3837 3938 P*29 2817 P*18 1716 N*28 3828 1817 1627 2928 2737 G*38"]
            ),
        ] {
            for algorithm in Algorithm::iter() {
                let board = sfen::decode_position(tc.0).expect("Failed to parse");
                let want: Vec<Solution> =
                    tc.1.clone()
                        .into_iter()
                        .map(|x| sfen::decode_moves(x).unwrap())
                        .collect();

                eprintln!("Solving {:?} (algo={:?})", board, algorithm);
                let got = solve(board, None, algorithm).unwrap();

                assert_eq!(got, want);
            }
        }
    }

    #[test]
    fn no_answer() {
        for sfen in [
            "4k4/9/4P4/9/9/8p/8K/9/9 b G2r2b3g4s4n4l16p 1",
            "9/9/9/5bp1G/6k2/6l1P/8K/9/8N b 2rb3g4s3n3l16p 1",
            "9/9/9/9/9/pp7/kl7/9/K8 b P2r2b4g4s4n3l15p 1",
            "7pk/7bg/9/8K/8N/8P/8L/9/9 b 2rb3g4s3n3l16p",
            "9/9/9/4k4/9/3pK3+r/4LP3/9/9 b r2b4g4s4n3l16p 1",
            "9/9/9/4k4/9/4K1P1+r/9/8B/9 b rb4g4s4n4l17p 1",
            "9/7PP/7Lk/9/7LK/7LL/9/9/9 b 2r2b4g4s4n16p 1",
        ] {
            for algorithm in Algorithm::iter() {
                let board = sfen::decode_position(sfen).unwrap();
                eprintln!("solving {}", sfen);
                let got = solve(board.clone(), None, algorithm).unwrap();
                let want: Vec<Vec<Movement>> = vec![];
                assert_eq!(got, want);
            }
        }
    }

    #[test]
    fn two_answers() {
        for sfen in ["7lk/7r1/7lP/8G/9/9/9/9/9 b Lr2b3g4s4nl17p 1"] {
            for algorithm in Algorithm::iter() {
                eprintln!("solving {} {:?}", sfen, algorithm);
                let board = sfen::decode_position(sfen).unwrap();
                let got = solve(board.clone(), None, algorithm).unwrap();
                assert_eq!(got.len(), 2);
            }
        }
    }
}
