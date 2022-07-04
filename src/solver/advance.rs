use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use crate::{
    piece::Color,
    position::{self, Position},
    solver::solve::digest,
};

use super::solve::Digest;

pub(super) enum State {
    Intermediate(Vec<Position>),
    Mate(Vec<Position>),
}

const NPROC: usize = 4;

pub(super) fn advance<'a>(
    memo_next: Mutex<HashMap<Digest, usize>>,
    current: Vec<Position>,
    step: usize,
) -> anyhow::Result<State> {
    let ctx = Context::new(memo_next, step);
    ctx.advance(current)?;
    let state: MutableState = Arc::try_unwrap(ctx.mutable_state)
        .map_err(|_| anyhow::anyhow!("BUG"))?
        .into_inner()
        .map_err(|_| anyhow::anyhow!("BUG"))?;
    Ok(
        if !state.mate_positions.is_empty() || state.next_positions.is_empty() {
            State::Mate(state.mate_positions)
        } else {
            State::Intermediate(state.next_positions)
        },
    )
}
struct Context {
    step: usize,
    memo_next: Arc<HashMap<Digest, usize>>,
    advanced_positions: Arc<Mutex<Positions>>,
}

struct Positions {
    intermediates: Vec<Position>,
    mates: Vec<Position>,
}

struct MutableState {
    memo_next: Mutex<HashMap<Digest, usize>>,
    result: 
}

impl Context {
    fn new(memo_next: Mutex<HashMap<Digest, usize>>, step: usize) -> Self {
        let state = Arc::new(Mutex::new(MutableState {
            memo_next,
            mate_positions: vec![],
            next_positions: vec![],
        }));
        Self {
            step,
            mutable_state: state,
        }
    }
    fn advance(&self, current: Vec<Position>) -> anyhow::Result<()> {
        let Self {
            step,
            mutable_state,
        } = self;

        let current = Arc::new(current);

        let chunk_size = (current.len() + NPROC - 1) / NPROC;

        for i in 0..NPROC {
            let current = current.clone();
            let mutable_state = mutable_state.clone();
            let range = (i * chunk_size)..((i + 1) * chunk_size).min(current.len());
            std::thread::spawn(move || {
                for j in range {
                    let mut movable = false;
                    let position = &current[j];
                    let advanced = position::advance(position).unwrap();
                    {
                        // let mut mutable_state = mutable_state.lock().unwrap();
                        // for next_position in advanced {
                        //     movable = true;
                        //     if !mutable_state.mate_positions.is_empty() {
                        //         break;
                        //     }
                        //     let digest = digest(&next_position);
                        //     if mutable_state.memo_next.contains_key(&digest) {
                        //         continue;
                        //     }
                        //     mutable_state.memo_next.insert(digest, self.step);
                        //     mutable_state.next_positions.push(next_position);
                        // }
                        // if !movable && position.turn() == Color::White && !position.pawn_drop() {
                        //     // Checkmate
                        //     mutable_state.mate_positions.push(position.clone());
                        // }
                    }
                }
            });
        }

        Ok(())
    }
}
