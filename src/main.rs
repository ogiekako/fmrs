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

fn main() {
    for tc in vec![(
        board::BitBoard::new(),
        board::Square::new(0, 0),
        piece::Black,
        piece::Bishop,
    )] {
        let b = board::movable_positions(tc.0, tc.1, tc.2, tc.3);
        println!("{:?} -> {:?}", tc, b);
    }
}
