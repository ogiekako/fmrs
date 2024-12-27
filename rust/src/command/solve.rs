use fmrs_core::{position::PositionExt, sfen};

use crate::solver::{self, Algorithm};

pub fn solve(algorithm: Algorithm, sfen_or_file: Option<String>) -> anyhow::Result<()> {
    let position = sfen::decode_position(
        match sfen_or_file {
            Some(sfen_or_file) => {
                if sfen_or_file.ends_with(".sfen") {
                    std::fs::read_to_string(sfen_or_file)?
                } else {
                    sfen_or_file
                }
            }
            None => {
                eprintln!("Enter SFEN (hint: https://sfenreader.appspot.com/ja/create_board.html)");
                eprint!("> ");

                let mut s = "".to_string();
                std::io::stdin().read_line(&mut s)?;

                print!("position {} moves", s);
                s
            }
        }
        .as_str(),
    )
    .map_err(|_e| anyhow::anyhow!("parse failed"))?;

    let answer = solver::solve(position.clone(), Some(10), algorithm, None)
        .map_err(|e| anyhow::anyhow!("{}", e))?;

    if answer.is_empty() {
        eprintln!("No solution");
        return Ok(());
    }
    eprintln!("Solved in {} steps", answer[0].len());
    if answer.len() > 1 {
        eprintln!("Multiple solutions found: showing only the first one");
    }
    let mut position = position;
    for x in answer[0].iter() {
        position.do_move(x);
        print!(" {}", sfen::encode_move(x));
    }
    println!();
    eprintln!("last state: {:?}", position);
    Ok(())
}
