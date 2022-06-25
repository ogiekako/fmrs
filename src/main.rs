#[macro_use]
extern crate lazy_static;
extern crate arr_macro;
extern crate rand;
extern crate serde;

#[macro_use]
pub mod board;
pub mod piece;
pub mod position;
pub mod sfen;
pub mod solver;

fn main() -> anyhow::Result<()> {
    println!("Enter SFEN (hint: https://sfenreader.appspot.com/ja/create_board.html)");
    print!("> ");

    let mut s = "".to_string();
    std::io::stdin().read_line(&mut s)?;

    let position = sfen::decode_position(&s).map_err(|_e| anyhow::anyhow!("parse failed"))?;

    let answer = solver::solve(&position, None).map_err(|e| anyhow::anyhow!("{}", e))?;

    if answer.is_empty() {
        println!("No solution");
        return Ok(());
    }
    println!("Solved in {} steps", answer[0].len());
    if answer.len() > 1 {
        println!("Multiple solutions found: showing only the first one");
    }
    let mut position = position.clone();
    for x in answer[0].iter() {
        position.do_move(x);
        println!("{}", sfen::encode_move(x));
    }
    println!("last state: {:?}", position);
    Ok(())
}
