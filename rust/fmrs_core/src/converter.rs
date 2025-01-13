use std::collections::HashMap;

use shogi_kifu_converter::converter::ToKif as _;

use crate::jkf::{self, JsonKifuFormat};
use crate::position::position::PositionAux;
use crate::solve::Solution;
use crate::{
    piece::{Color, Kind},
    position::{Hands, Movement, Square},
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

fn hands(hands: Hands) -> [jkf::Hand; 2] {
    let mut res: [jkf::Hand; 2] = Default::default();
    for color in [Color::BLACK, Color::WHITE] {
        res[color.index()].FU = hands.count(color, Kind::Pawn) as u8;
        res[color.index()].KY = hands.count(color, Kind::Lance) as u8;
        res[color.index()].KE = hands.count(color, Kind::Knight) as u8;
        res[color.index()].GI = hands.count(color, Kind::Silver) as u8;
        res[color.index()].KI = hands.count(color, Kind::Gold) as u8;
        res[color.index()].KA = hands.count(color, Kind::Bishop) as u8;
        res[color.index()].HI = hands.count(color, Kind::Rook) as u8;
    }
    res
}

fn initial(position: &mut PositionAux) -> jkf::Initial {
    let color = jkf::Color::Black;
    let board = {
        let mut board = [[jkf::Piece::default(); 9]; 9];
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
        preset: jkf::Preset::PresetOther,
        data: Some(jkf::StateFormat {
            color,
            board,
            hands,
        }),
    }
}

fn place_format(pos: Square) -> jkf::PlaceFormat {
    let x = pos.col() as u8 + 1;
    let y = pos.row() as u8 + 1;
    jkf::PlaceFormat { x, y }
}

fn tail_move_format(move_move_format: jkf::MoveMoveFormat) -> jkf::MoveFormat {
    jkf::MoveFormat {
        comments: None,
        move_: Some(move_move_format),
        time: None,
        special: None,
        forks: None,
    }
}

fn update_move_format(
    mut move_format: &mut Vec<jkf::MoveFormat>,
    mut position: PositionAux,
    solution: &Solution,
) {
    let mut i = 0;
    for movement in solution.iter() {
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
        if move_format[i].move_.as_ref() == Some(&move_move_format) {
            i += 1;
            continue;
        }

        // Compute fork
        let mut fork_index: Option<usize> = None;
        for forks in move_format[i].forks.iter() {
            for (j, fork) in forks.iter().enumerate() {
                if fork[0].move_.as_ref() == Some(&move_move_format) {
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

pub fn convert(position: &mut PositionAux, solutions: &[Solution]) -> JsonKifuFormat {
    let header = HashMap::default();
    let initial = Some(initial(position));
    let moves = {
        let mut moves = vec![];
        for solution in solutions {
            update_move_format(&mut moves, position.clone(), solution);
        }
        let move0 = jkf::MoveFormat::default();
        vec![move0].into_iter().chain(moves).collect()
    };
    JsonKifuFormat {
        header,
        initial,
        moves,
    }
}

pub fn convert_to_kif(position: &mut PositionAux, solutions: &[Solution]) -> String {
    let jkf = convert(position, solutions);
    jkf.to_kif_owned()
}

#[cfg(test)]
mod tests {
    use crate::jkf::JsonKifuFormat;
    use crate::position::position::PositionAux;
    use crate::solve::StandardSolver;
    use crate::solve::{Solution, SolverStatus};

    #[test]
    fn convert() {
        for (want, problem) in [
            (
                r#"{"header":{},"initial":{"preset":"OTHER","data":{"color":0,"board":[[{},{},{},{},{},{},{},{},{}],[{"color":1,"kind":"OU"},{},{"color":0,"kind":"FU"},{},{},{},{},{},{}],[{},{},{},{},{},{},{},{},{}],[{},{},{},{},{},{},{},{},{}],[{},{},{},{},{},{},{},{},{}],[{},{},{},{},{},{},{},{},{}],[{},{},{},{},{},{},{},{},{}],[{},{},{},{},{},{},{},{},{}],[{},{},{},{},{},{},{},{},{}]],"hands":[{"FU":0,"KY":0,"KE":0,"GI":0,"KI":1,"KA":0,"HI":0},{"FU":17,"KY":4,"KE":4,"GI":4,"KI":3,"KA":2,"HI":2}]}},"moves":[{},{"move":{"color":0,"to":{"x":2,"y":2},"piece":"KI"}}]}"#,
                "7k1/9/7P1/9/9/9/9/9/9 b G2r2b3g4s4n4l17p 1",
            ),
            (
                r#"{"header":{},"initial":{"preset":"OTHER","data":{"color":0,"board":[[{},{},{},{},{},{},{},{},{}],[{},{},{},{},{},{},{},{},{}],[{},{},{},{},{},{},{},{},{}],[{},{},{},{},{},{},{},{},{}],[{},{"color":1,"kind":"OU"},{},{},{},{},{},{},{}],[{},{},{},{},{},{},{},{},{}],[{},{},{},{},{},{},{},{},{}],[{},{},{},{},{},{},{},{},{}],[{},{},{},{},{},{},{},{},{}]],"hands":[{"FU":0,"KY":0,"KE":1,"GI":0,"KI":1,"KA":0,"HI":0},{"FU":18,"KY":4,"KE":3,"GI":4,"KI":3,"KA":2,"HI":2}]}},"moves":[{},{"move":{"color":0,"to":{"x":4,"y":4},"piece":"KE"},"forks":[[{"move":{"color":0,"to":{"x":6,"y":4},"piece":"KE"}},{"move":{"color":1,"from":{"x":5,"y":2},"to":{"x":5,"y":1},"piece":"OU"}},{"move":{"color":0,"to":{"x":5,"y":2},"piece":"KI"}}]]},{"move":{"color":1,"from":{"x":5,"y":2},"to":{"x":5,"y":1},"piece":"OU"}},{"move":{"color":0,"to":{"x":5,"y":2},"piece":"KI"}}]}"#,
                "9/4k4/9/9/9/9/9/9/9 b GN2r2b3g4s3n4l18p 1",
            ),
            (
                r#"{"header":{},"initial":{"preset":"OTHER","data":{"color":0,"board":[[{},{},{},{},{},{},{},{},{}],[{},{"color":1,"kind":"OU"},{},{},{},{},{},{},{}],[{"color":1,"kind":"KY"},{"color":1,"kind":"KY"},{"color":1,"kind":"KY"},{},{},{},{},{},{}],[{},{},{},{},{},{},{},{},{}],[{},{},{},{},{},{},{},{},{}],[{},{},{},{},{},{},{},{},{}],[{},{},{},{},{},{},{},{},{}],[{},{},{},{},{},{},{},{},{}],[{},{},{},{},{},{},{},{},{}]],"hands":[{"FU":0,"KY":0,"KE":1,"GI":0,"KI":1,"KA":0,"HI":0},{"FU":18,"KY":1,"KE":3,"GI":4,"KI":3,"KA":2,"HI":2}]}},"moves":[{}, {"move":{"color":0,"to":{"x":1,"y":4},"piece":"KE"},"forks":[[{"move":{"color":0,"to":{"x":3,"y":4},"piece":"KE"}},{"move":{"color":1,"from":{"x":2,"y":2},"to":{"x":1,"y":1},"piece":"OU"},"forks":[[{"move":{"color":1,"from":{"x":2,"y":2},"to":{"x":2,"y":1},"piece":"OU"}},{"move":{"color":0,"to":{"x":2,"y":2},"piece":"KI"}}]]},{"move":{"color":0,"to":{"x":2,"y":2},"piece":"KI"}}]]},{"move":{"color":1,"from":{"x":2,"y":2},"to":{"x":1,"y":1},"piece":"OU"},"forks":[[{"move":{"color":1,"from":{"x":2,"y":2},"to":{"x":2,"y":1},"piece":"OU"}},{"move":{"color":0,"to":{"x":2,"y":2},"piece":"KI"}}]]},{"move":{"color":0,"to":{"x":2,"y":2},"piece":"KI"}}]}"#,
                "6l2/6lk1/6l2/9/9/9/9/9/9 b GN2r2b3g4s3nl18p 1",
            ),
        ] {
            let want: JsonKifuFormat = serde_json::from_str(want).unwrap();
            let want = serde_json::to_string(&want).unwrap(); // normalize

            let mut problem = crate::sfen::decode_position(problem).unwrap();

            let mut solutions = solve(problem.clone(), 10).unwrap();
            solutions.sort();

            let got = super::convert(&mut problem, &solutions);
            let got = serde_json::to_string(&got).unwrap();
            eprintln!("got = {}", got);
            pretty_assertions::assert_eq!(got, want);
        }
    }

    fn solve(position: PositionAux, solutions_upto: usize) -> anyhow::Result<Vec<Solution>> {
        let mut solver = StandardSolver::new(position, solutions_upto, false)?;
        loop {
            let status = solver.advance()?;
            match status {
                SolverStatus::Intermediate(_) => continue,
                SolverStatus::Mate(reconstructor) => return Ok(reconstructor.solutions()),
                SolverStatus::NoSolution => return Ok(vec![]),
            }
        }
    }
}
