use fmrs_core::{position::PositionExt, sfen};

use crate::solver::{self, Algorithm};

pub async fn solve(algorithm: Algorithm, problem_sfen: Option<String>) -> anyhow::Result<()> {
    let position = sfen::decode_position(
        match problem_sfen {
            Some(s) => s,
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

    let answer = solver::solve(position.clone(), Some(10), algorithm)
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
