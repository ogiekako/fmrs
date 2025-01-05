use fmrs_core::{
    piece::{Color, Kind},
    position::{
        bitboard::{gold_power, magic::rook_reachable, reachable, rook_power},
        position::PositionAux,
        BitBoard, Square,
    },
    search::backward::backward_search,
    solve::{SolverStatus, StandardSolver},
};
use log::info;
// use rand::{rngs::SmallRng, seq::SliceRandom, SeedableRng};
use rayon::prelude::*;

pub fn batch_square() -> anyhow::Result<()> {
    let positions = positions();
    // positions.shuffle(&mut SmallRng::seed_from_u64(20250105));

    eprintln!("{} positions {:?}", positions.len(), positions[0]);

    let chunk_size = 1000;
    let chunks = positions.chunks(chunk_size).collect::<Vec<_>>();

    let mut best_solutions = (0, vec![]);
    for (i, chunk) in chunks.into_iter().enumerate() {
        let solutions = chunk
            .into_par_iter()
            .map(|position| {
                let res = backward_search(position, true).unwrap();
                debug_assert!(!res.1.is_empty(), "{} {:?}", res.0, position);
                res
            })
            .collect::<Vec<_>>();

        for (step, positions) in solutions {
            if step > best_solutions.0 {
                best_solutions = (step, positions);
            } else if step == best_solutions.0 {
                best_solutions.1.extend(positions);
            }
        }
        info!(
            "{}/{} best {} {:?}",
            ((i + 1) * chunk_size).min(positions.len()),
            positions.len(),
            best_solutions.0,
            best_solutions.1.last().unwrap(),
        );
    }
    eprintln!("mate in {}:", best_solutions.0);
    for mut position in best_solutions.1 {
        eprintln!("{}", position.sfen_url());
        println!("{}", position.sfen());
    }

    Ok(())
}

fn positions() -> Vec<PositionAux> {
    let mut positions = vec![];
    for h in 2..=5 {
        for w in 3..=5 {
            let area = h * w;
            if !(9..25).contains(&area) {
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
                stone.set(pos);
            }
        }
    }
    for king in area {
        for rook in rook_reachable(stone, king).and_not(stone) {
            let mut position = PositionAux::default();
            position.set_stone(stone);
            position.set_turn(Color::WHITE);
            position.set(king, Color::WHITE, Kind::King);
            position.set(rook, Color::BLACK, Kind::Rook);

            let remaining = reachable(&mut position, Color::WHITE, king, Kind::King, false)
                .and_not(rook_power(rook));

            let mut mms = vec![];
            model_mates(&mut position, remaining, rook, &mut mms);

            for mut mm in mms {
                let black_pawn_mask = mm
                    .bitboard(Color::BLACK, Kind::Pawn)
                    .fold(0, |acc, p| acc | 1 << p.col());
                let white_pawn_mask = mm
                    .bitboard(Color::WHITE, Kind::Pawn)
                    .fold(0, |acc, p| acc | 1 << p.col());

                for bp in 0..1 << w {
                    if bp & black_pawn_mask != 0 {
                        continue;
                    }
                    for wp in 0..1 << w {
                        if wp & white_pawn_mask != 0 {
                            continue;
                        }
                        let mut position = mm.clone();
                        for i in 0..w {
                            if bp & 1 << i != 0 {
                                position.set(Square::new(i, 1), Color::BLACK, Kind::Pawn);
                            }
                            if wp & 1 << i != 0 {
                                position.set(Square::new(i, 0), Color::WHITE, Kind::Pawn);
                            }
                        }
                        positions.push(position);
                    }
                }
            }
        }
    }
}

fn model_mates(
    position: &mut PositionAux,
    mut remaining: BitBoard,
    attacker_pos: Square,
    positions: &mut Vec<PositionAux>,
) {
    let Some(pos) = remaining.next() else {
        if !position.checked_slow(Color::WHITE) {
            return;
        }
        let mut solver = StandardSolver::new(position.clone(), 2, true);
        if solver.advance().unwrap() == SolverStatus::Mate(vec![vec![]]) {
            positions.push(position.clone());
        }
        return;
    };
    if position.get(pos).is_none()
        && pos.row() != 8
        && attacker_pos != Square::new(pos.col(), pos.row() + 1)
        && !position.col_has_pawn(Color::WHITE, pos.col())
    {
        position.set(pos, Color::WHITE, Kind::Pawn);
        model_mates(position, remaining, attacker_pos, positions);
        position.unset(pos, Color::WHITE, Kind::Pawn);
    }
    if position.get(pos).is_none() && !gold_power(Color::WHITE, pos).get(attacker_pos) {
        position.set(pos, Color::WHITE, Kind::ProPawn);
        model_mates(position, remaining, attacker_pos, positions);
        position.unset(pos, Color::WHITE, Kind::ProPawn);
    }

    if pos.row() >= 8 {
        return;
    }
    let lower = Square::new(pos.col(), pos.row() + 1);
    if position.get(lower).is_none() && !position.col_has_pawn(Color::BLACK, pos.col()) {
        position.set(lower, Color::BLACK, Kind::Pawn);
        model_mates(position, remaining, attacker_pos, positions);
        position.unset(lower, Color::BLACK, Kind::Pawn);
    }
}
