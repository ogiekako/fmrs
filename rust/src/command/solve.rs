use fmrs_core::sfen;

use crate::{
    command::parse_to_sfen,
    solver::{self, Algorithm},
};

pub fn solve(algorithm: Algorithm, sfen_or_file_or_url: Option<String>) -> anyhow::Result<()> {
    let position = sfen::decode_position(
        match sfen_or_file_or_url {
            Some(x) => parse_to_sfen(&x)?,
            None => {
                eprintln!("Enter SFEN (hint: https://ogiekako.github.io/fmrs)");
                eprint!("> ");

                let mut input = "".to_string();
                std::io::stdin().read_line(&mut input)?;

                let s = parse_to_sfen(input.trim())?;

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
    eprintln!("Solved in {} steps", answer[0].0.len());
    if answer.len() > 1 {
        eprintln!("Multiple solutions found: showing only the first one");
    }
    let mut position = position;
    for x in answer[0].0.iter() {
        position.do_move(x);
        print!(" {}", sfen::encode_move(x));
    }
    println!();
    eprintln!("last state: {:?}", position);
    Ok(())
}
