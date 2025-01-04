use fmrs_core::{
    piece::{Color, Kind},
    position::{bitboard::power, position::PositionAux, BitBoard, Square},
};
use rand::{rngs::SmallRng, seq::SliceRandom, SeedableRng};

use super::batch::{batch_solve, Criteria};

pub fn batch_square() -> anyhow::Result<()> {
    let mut positions = positions();
    positions.shuffle(&mut SmallRng::seed_from_u64(20250105));

    eprintln!("{} positions", positions.len());

    let mut solutions = batch_solve(positions, Criteria::AllUnique)?;

    solutions.sort_by_key(|(_, solution)| solution.0.len());

    for (mut position, solution) in solutions {
        println!("{} {}", solution.0.len(), position.sfen());
    }

    Ok(())
}

fn positions() -> Vec<PositionAux> {
    let mut positions = vec![];
    for h in 2..=5 {
        for w in 3..=5 {
            let area = h * w;
            if area >= 25 || area < 9 {
                continue;
            }
            insert(&mut positions, h, w);
        }
    }
    positions
}

fn insert(positions: &mut Vec<PositionAux>, h: usize, w: usize) {
    let mut area = BitBoard::default();
    for i in 0..h {
        for j in 0..w {
            area.set(Square::new(j, 8 - i));
        }
    }
    let mut stone = BitBoard::default();
    for i in 0..h + 1 {
        for j in 0..w + 1 {
            let pos = Square::new(j, 8 - i);
            if !area.get(pos) {
                stone.set(Square::new(j, 8 - i));
            }
        }
    }
    for king in area {
        for kind in [Kind::Rook, Kind::Bishop] {
            for pos in area.and_not(power(Color::WHITE, king, kind)) {
                for black_pawn in 0i32..1 << w {
                    if black_pawn.count_ones() == w as u32 {
                        continue;
                    }
                    for white_pawn in 0i32..1 << w {
                        if white_pawn.count_ones() == w as u32 {
                            continue;
                        }
                        for hand_pawn in 3..=4 {
                            let mut position = PositionAux::default();
                            position.set(king, Color::WHITE, Kind::King);
                            position.set_stone(stone);

                            for i in 0..w {
                                if black_pawn & 1 << i != 0 {
                                    position.set(Square::new(i, 1), Color::BLACK, Kind::Pawn);
                                }
                                if white_pawn & 1 << i != 0 {
                                    position.set(Square::new(i, 0), Color::WHITE, Kind::Pawn);
                                }
                            }
                            let hands = position.hands_mut();
                            for _ in 0..hand_pawn {
                                hands.add(Color::WHITE, Kind::Pawn);
                            }
                            if pos != king {
                                position.set(pos, Color::BLACK, kind);
                            } else {
                                hands.add(Color::BLACK, kind);
                            }

                            positions.push(position);
                        }
                    }
                }
            }
        }
    }
}
