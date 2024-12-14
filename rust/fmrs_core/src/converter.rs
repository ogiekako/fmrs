use std::collections::BTreeMap;

use crate::jkf::{self, JsonKifFormat};
use crate::solve::Solution;
use crate::{
    piece::{Color, Kind},
    position::{Hands, Movement, Position, PositionExt, Square},
};

fn color(color: Color) -> jkf::Color {
    match color {
        Color::BLACK => jkf::Color::Black,
        Color::WHITE => jkf::Color::White,
    }
}

fn kind(kind: Kind) -> jkf::Kind {
    match kind {
        Kind::Pawn => jkf::Kind::FU,
        Kind::Lance => jkf::Kind::KY,
        Kind::Knight => jkf::Kind::KE,
        Kind::Silver => jkf::Kind::GI,
        Kind::Gold => jkf::Kind::KI,
        Kind::Bishop => jkf::Kind::KA,
        Kind::Rook => jkf::Kind::HI,
        Kind::King => jkf::Kind::OU,
        Kind::ProPawn => jkf::Kind::TO,
        Kind::ProLance => jkf::Kind::NY,
        Kind::ProKnight => jkf::Kind::NK,
        Kind::ProSilver => jkf::Kind::NG,
        Kind::ProBishop => jkf::Kind::UM,
        Kind::ProRook => jkf::Kind::RY,
    }
}

fn piece(c: Color, k: Kind) -> jkf::Piece {
    jkf::Piece {
        color: Some(color(c)),
        kind: Some(kind(k)),
    }
}

fn raw_kind(kind: Kind) -> Option<jkf::RawKind> {
    Some(match kind {
        Kind::Pawn => jkf::RawKind::FU,
        Kind::Lance => jkf::RawKind::KY,
        Kind::Knight => jkf::RawKind::KE,
        Kind::Silver => jkf::RawKind::GI,
        Kind::Gold => jkf::RawKind::KI,
        Kind::Bishop => jkf::RawKind::KA,
        Kind::Rook => jkf::RawKind::HI,
        Kind::King => panic!("BUG"),
        Kind::ProPawn => panic!("BUG"),
        Kind::ProLance => panic!("BUG"),
        Kind::ProKnight => panic!("BUG"),
        Kind::ProSilver => panic!("BUG"),
        Kind::ProBishop => panic!("BUG"),
        Kind::ProRook => panic!("BUG"),
    })
}

fn hands(hands: &Hands) -> Vec<BTreeMap<jkf::RawKind, usize>> {
    let mut res = vec![];
    for color in [Color::BLACK, Color::WHITE] {
        let mut map = BTreeMap::default();
        for k in hands.kinds(color) {
            map.insert(raw_kind(k).unwrap(), hands.count(color, k));
        }
        res.push(map);
    }
    res
}

fn initial(position: &Position) -> jkf::Initial {
    let color = jkf::Color::Black;
    let board = {
        let mut board = vec![vec![jkf::Piece::default(); 9]; 9];
        for col in 0..9 {
            for row in 0..9 {
                if let Some((color, kind)) = position.get(Square::new(col, row)) {
                    board[col][row] = piece(color, kind)
                }
            }
        }
        board
    };
    let hands = hands(position.hands());
    jkf::Initial {
        preset: "OTHER".to_string(),
        data: Some(jkf::StateFormat {
            color,
            board,
            hands,
        }),
    }
}

fn place_format(pos: Square) -> jkf::PlaceFormat {
    let x = pos.col() + 1;
    let y = pos.row() + 1;
    jkf::PlaceFormat { x, y }
}

fn tail_move_format(move_move_format: jkf::MoveMoveFormat) -> jkf::MoveFormat {
    jkf::MoveFormat {
        comments: None,
        r#move: Some(move_move_format),
        time: None,
        special: None,
        forks: None,
    }
}

fn update_move_format(
    mut move_format: &mut Vec<jkf::MoveFormat>,
    mut position: Position,
    solution: &Solution,
) {
    let mut i = 0;
    for movement in solution {
        let color = color(position.turn());
        let (from, to, piece, same, promote, capture) = match movement {
            Movement::Drop(to, k) => (None, place_format(*to), kind(*k), None, None, None),
            Movement::Move {
                source: from,
                dest: to,
                promote,
                ..
            } => {
                let (_, k) = position.get(*from).unwrap();
                let capture = if let Some((_, k)) = position.get(*to) {
                    Some(kind(k))
                } else {
                    None
                };
                (
                    Some(place_format(*from)),
                    place_format(*to),
                    kind(k),
                    None,
                    if *promote { Some(true) } else { None },
                    capture,
                )
            }
        };
        let move_move_format = jkf::MoveMoveFormat {
            color,
            from,
            to,
            piece,
            same,
            promote,
            capture,
            relative: None,
        };
        position.do_move(movement);

        if i >= move_format.len() {
            move_format.push(tail_move_format(move_move_format));
            i += 1;
            continue;
        }
        if move_format[i].r#move.as_ref() == Some(&move_move_format) {
            i += 1;
            continue;
        }

        // Compute fork
        let mut fork_index: Option<usize> = None;
        for forks in move_format[i].forks.iter() {
            for (j, fork) in forks.iter().enumerate() {
                if fork[0].r#move.as_ref() == Some(&move_move_format) {
                    fork_index = Some(j);
                    break;
                }
            }
        }

        if let Some(fork_index) = fork_index {
            move_format = move_format[i]
                .forks
                .as_mut()
                .unwrap()
                .get_mut(fork_index)
                .unwrap();
            i = 1;
            continue;
        }

        // Add a fork
        if let Some(forks) = move_format[i].forks.as_mut() {
            forks.push(vec![tail_move_format(move_move_format)]);
        } else {
            move_format[i].forks = Some(vec![vec![tail_move_format(move_move_format)]]);
        }
        move_format = move_format[i].forks.as_mut().unwrap().last_mut().unwrap();
        i = 1;
    }
}

pub fn convert(position: &Position, solutions: &[Solution]) -> JsonKifFormat {
    let header = BTreeMap::default();
    let initial = Some(initial(position));
    let moves = {
        let mut moves = vec![];
        for solution in solutions {
            update_move_format(&mut moves, position.clone(), solution);
        }
        let move0 = jkf::MoveFormat::default();
        vec![move0].into_iter().chain(moves.into_iter()).collect()
    };
    JsonKifFormat {
        header,
        initial,
        moves,
    }
}

#[cfg(test)]
mod tests {
    use crate::jkf::JsonKifFormat;
    use crate::solve::{Solution, SolverStatus};
    use crate::{position::Position, solve::StandardSolver};

    #[test]
    fn convert() {
        for (want, problem) in [
            (
                r#"{"header":{},"initial":{"preset":"OTHER","data":{"color":0,"board":[[{},{},{},{},{},{},{},{},{}],[{"color":1,"kind":"OU"},{},{"color":0,"kind":"FU"},{},{},{},{},{},{}],[{},{},{},{},{},{},{},{},{}],[{},{},{},{},{},{},{},{},{}],[{},{},{},{},{},{},{},{},{}],[{},{},{},{},{},{},{},{},{}],[{},{},{},{},{},{},{},{},{}],[{},{},{},{},{},{},{},{},{}],[{},{},{},{},{},{},{},{},{}]],"hands":[{"KI":1},{"KA":2,"HI":2,"KY":4,"FU":17,"KE":4,"GI":4,"KI":3}]}},"moves":[{},{"move":{"color":0,"to":{"x":2,"y":2},"piece":"KI"}}]}"#,
                "7k1/9/7P1/9/9/9/9/9/9 b G2r2b3g4s4n4l17p 1",
            ),
            (
                r#"{"header":{},"initial":{"preset":"OTHER","data":{"color":0,"board":[[{},{},{},{},{},{},{},{},{}],[{},{},{},{},{},{},{},{},{}],[{},{},{},{},{},{},{},{},{}],[{},{},{},{},{},{},{},{},{}],[{},{"color":1,"kind":"OU"},{},{},{},{},{},{},{}],[{},{},{},{},{},{},{},{},{}],[{},{},{},{},{},{},{},{},{}],[{},{},{},{},{},{},{},{},{}],[{},{},{},{},{},{},{},{},{}]],"hands":[{"KE":1,"KI":1},{"FU":18,"KY":4,"KE":3,"GI":4,"KI":3,"KA":2,"HI":2}]}},"moves":[{},{"move":{"color":0,"to":{"x":4,"y":4},"piece":"KE"},"forks":[[{"move":{"color":0,"to":{"x":6,"y":4},"piece":"KE"}},{"move":{"color":1,"from":{"x":5,"y":2},"to":{"x":5,"y":1},"piece":"OU"}},{"move":{"color":0,"to":{"x":5,"y":2},"piece":"KI"}}]]},{"move":{"color":1,"from":{"x":5,"y":2},"to":{"x":5,"y":1},"piece":"OU"}},{"move":{"color":0,"to":{"x":5,"y":2},"piece":"KI"}}]}"#,
                "9/4k4/9/9/9/9/9/9/9 b GN2r2b3g4s3n4l18p 1",
            ),
            (
                r#"{"header":{},"initial":{"preset":"OTHER","data":{"color":0,"board":[[{},{},{},{},{},{},{},{},{}],[{},{"color":1,"kind":"OU"},{},{},{},{},{},{},{}],[{"color":1,"kind":"KY"},{"color":1,"kind":"KY"},{"color":1,"kind":"KY"},{},{},{},{},{},{}],[{},{},{},{},{},{},{},{},{}],[{},{},{},{},{},{},{},{},{}],[{},{},{},{},{},{},{},{},{}],[{},{},{},{},{},{},{},{},{}],[{},{},{},{},{},{},{},{},{}],[{},{},{},{},{},{},{},{},{}]],"hands":[{"KE":1,"KI":1},{"FU":18,"KY":1,"KE":3,"GI":4,"KI":3,"KA":2,"HI":2}]}},"moves":[{}, {"move":{"color":0,"to":{"x":1,"y":4},"piece":"KE"},"forks":[[{"move":{"color":0,"to":{"x":3,"y":4},"piece":"KE"}},{"move":{"color":1,"from":{"x":2,"y":2},"to":{"x":1,"y":1},"piece":"OU"},"forks":[[{"move":{"color":1,"from":{"x":2,"y":2},"to":{"x":2,"y":1},"piece":"OU"}},{"move":{"color":0,"to":{"x":2,"y":2},"piece":"KI"}}]]},{"move":{"color":0,"to":{"x":2,"y":2},"piece":"KI"}}]]},{"move":{"color":1,"from":{"x":2,"y":2},"to":{"x":1,"y":1},"piece":"OU"},"forks":[[{"move":{"color":1,"from":{"x":2,"y":2},"to":{"x":2,"y":1},"piece":"OU"}},{"move":{"color":0,"to":{"x":2,"y":2},"piece":"KI"}}]]},{"move":{"color":0,"to":{"x":2,"y":2},"piece":"KI"}}]}"#,
                "6l2/6lk1/6l2/9/9/9/9/9/9 b GN2r2b3g4s3nl18p 1",
            ),
        ] {
            let want: JsonKifFormat = serde_json::from_str(want).unwrap();
            let want = serde_json::to_string(&want).unwrap(); // normalize

            let problem = crate::sfen::decode_position(problem).unwrap();

            let mut solutions = solve(problem.clone(), 10).unwrap();
            solutions.sort();

            let got = super::convert(&problem, &solutions);
            let got = serde_json::to_string(&got).unwrap();
            eprintln!("got = {}", got);
            pretty_assertions::assert_eq!(got, want);
        }
    }

    fn solve(position: Position, solutions_upto: usize) -> anyhow::Result<Vec<Solution>> {
        let mut solver = StandardSolver::new(position, solutions_upto);
        loop {
            let status = solver.advance()?;
            match status {
                SolverStatus::Intermediate => continue,
                SolverStatus::Mate(solutions) => return Ok(solutions),
                SolverStatus::NoSolution => return Ok(vec![]),
            }
        }
    }
}
