use crate::{
    position::PositionExt,
    sfen,
    solver::{self, Algorithm}, piece::Color,
};

pub async fn solve(algorithm: Algorithm) -> anyhow::Result<()> {
    println!("Enter SFEN (hint: https://sfenreader.appspot.com/ja/create_board.html)");
    print!("> ");

    let mut s = "".to_string();
    std::io::stdin().read_line(&mut s)?;

    let (position, _) = sfen::decode_position(&s).map_err(|_e| anyhow::anyhow!("parse failed"))?;

    let answer = solver::solve(position.clone(), Some(10), algorithm)
        .map_err(|e| anyhow::anyhow!("{}", e))?;

    if answer.is_empty() {
        println!("No solution");
        return Ok(());
    }
    println!("Solved in {} steps", answer[0].len());
    if answer.len() > 1 {
        println!("Multiple solutions found: showing only the first one");
    }
    let mut position = position;
    let mut turn = Color::Black;
    for x in answer[0].iter() {
        position.do_move(x, turn);
        turn = turn.opposite();
        println!("{}", sfen::encode_move(x));
    }
    println!("last state: {:?}", position);
    Ok(())
}
