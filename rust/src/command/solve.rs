use fmrs_core::{position::PositionExt, sfen};
use url::Url;

use crate::solver::{self, Algorithm};

fn parse_input(sfen_or_file_or_url: &str) -> anyhow::Result<String> {
    Ok(match sfen_or_file_or_url {
        x if x.ends_with(".sfen") => std::fs::read_to_string(x)?,
        x if x.starts_with("http") => {
            let url = Url::parse(x)?;
            url.query_pairs()
                .find(|(k, _)| k == "sfen")
                .map(|(_, v)| v.to_string())
                .ok_or_else(|| anyhow::anyhow!("no sfen query parameter"))?
        }
        _ => sfen_or_file_or_url.to_string(),
    })
}

pub fn solve(algorithm: Algorithm, sfen_or_file_or_url: Option<String>) -> anyhow::Result<()> {
    let position = sfen::decode_position(
        match sfen_or_file_or_url {
            Some(x) => parse_input(&x)?,
            None => {
                eprintln!("Enter SFEN (hint: https://ogiekako.github.io/fmrs)");
                eprint!("> ");

                let mut input = "".to_string();
                std::io::stdin().read_line(&mut input)?;

                let s = parse_input(input.trim())?;

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
