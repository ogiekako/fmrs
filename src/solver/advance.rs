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

const NPROC: usize = 16;

pub(super) fn advance<'a>(
    memo: &'a HashMap<Digest, usize>,
    memo_next: &'a mut HashMap<Digest, usize>,
    current: Vec<Position>,
    step: usize,
) -> anyhow::Result<State> {
    let ctx = Context::new(memo, memo_next, current, step);
    ctx.advance()?;
    let state = ctx.state.into_inner()?;
    Ok(
        if !state.mate_positions.is_empty() || state.next_positions.is_empty() {
            State::Mate(state.mate_positions)
        } else {
            State::Intermediate(state.next_positions)
        },
    )
}
struct Context<'a> {
    memo: Arc<&'a HashMap<Digest, usize>>,
    current: Vec<Position>,
    step: usize,
    state: Arc<Mutex<MutableState<'a>>>,
}

struct MutableState<'a> {
    memo_next: &'a mut HashMap<Digest, usize>,
    mate_positions: Vec<Position>,
    next_positions: Vec<Position>,
}

impl<'a> Context<'a> {
    fn new(
        memo: &'a HashMap<Digest, usize>,
        memo_next: &'a mut HashMap<Digest, usize>,
        current: Vec<Position>,
        step: usize,
    ) -> Self {
        let memo = Arc::new(memo);
        let state = Arc::new(Mutex::new(MutableState {
            memo_next,
            mate_positions: vec![],
            next_positions: vec![],
        }));
        Self {
            memo,
            current,
            step,
            state,
        }
    }
    fn advance(&'a self) -> anyhow::Result<State> {
        let memo = Arc::new(memo);
        let memo_next = Arc::new(Mutex::new(memo_next));
        let mut mate_positions = vec![];
        let mut next_positions = vec![];
        for position in current.iter() {
            debug_assert!(memo.get(&digest(position)).is_some());

            let mut movable = false;

            let advanced = position::advance(position)?;
            {
                let mut memo_next_guard = memo_next.lock().unwrap();
                for next_position in advanced {
                    movable = true;
                    if !mate_positions.is_empty() {
                        break;
                    }
                    let digest = digest(&next_position);
                    if memo_next_guard.contains_key(&digest) {
                        continue;
                    }
                    memo_next_guard.insert(digest, step);
                    next_positions.push(next_position);
                }
            }
            if !movable && position.turn() == Color::White && !position.pawn_drop() {
                // Checkmate
                mate_positions.push(position.clone());
            }
        }
        Ok(if mate_positions.is_empty() && !next_positions.is_empty() {
            State::Intermediate(next_positions)
        } else {
            State::Mate(mate_positions)
        })
    }
}
