use std::collections::VecDeque;
use std::rc::Rc;

use crate::memo::Memo;
use crate::nohash::NoHashMap;

use crate::position::position::PositionAux;
use crate::position::{previous, Movement, Position, PositionExt};

use super::Solution;

pub fn reconstruct_solutions(
    mate: &Position,
    memo_black_turn: &Memo,
    memo_white_turn: &Memo,
    solutions_upto: usize,
) -> Vec<Solution> {
    debug_assert!(memo_white_turn.contains_key(&mate.digest()));
    let step = *memo_white_turn.get(&mate.digest()).unwrap();
    let ctx = Context::new(memo_black_turn, memo_white_turn, step, solutions_upto);
    ctx.reconstruct_bfs(mate)
}

enum MovementList {
    Nil,
    Cons {
        cur: Movement,
        cdr: Rc<MovementList>,
    },
}

impl MovementList {
    fn nil() -> Rc<Self> {
        Self::Nil.into()
    }
    fn cons(cur: Movement, cdr: Rc<Self>) -> Rc<Self> {
        Self::Cons { cur, cdr }.into()
    }
    fn vec(mut self: &Rc<Self>) -> Vec<Movement> {
        let mut res = vec![];
        loop {
            match self.as_ref() {
                Self::Nil => return res,
                Self::Cons { cur, cdr } => {
                    res.push(cur.clone());
                    self = cdr;
                }
            }
        }
    }
}

struct Context<'a> {
    memo_black_turn: &'a Memo,
    memo_white_turn: &'a Memo,
    mate_in: u16,
    solutions_upto: usize,
}

impl<'a> Context<'a> {
    fn new(
        memo_black_turn: &'a Memo,
        memo_white_turn: &'a Memo,
        mate_in: u16,
        solutions_upto: usize,
    ) -> Self {
        Self {
            memo_black_turn,
            memo_white_turn,
            mate_in,
            solutions_upto,
        }
    }

    fn reconstruct_bfs(&self, mate_position: &Position) -> Vec<Solution> {
        let mut position_visit_count = NoHashMap::default();
        let mut queue: VecDeque<(Position, u16, Rc<MovementList>)> = VecDeque::new();
        queue.push_back((mate_position.clone(), self.mate_in, MovementList::nil()));
        let mut res = vec![];

        let mut undo_moves = vec![];
        while let Some((position, step, following_movements)) = queue.pop_front() {
            if res.len() >= self.solutions_upto {
                break;
            }
            if step == 0 {
                res.push(following_movements.vec());
                continue;
            }
            {
                let digest = position.digest();
                let visit_count = position_visit_count.entry(digest).or_insert(0);
                if *visit_count >= self.solutions_upto as u64 {
                    continue;
                }
                *visit_count += 1;
            }

            let memo_previous = if step % 2 == 0 {
                self.memo_white_turn
            } else {
                self.memo_black_turn
            };

            undo_moves.clear();
            previous(
                &mut PositionAux::new(position.clone()),
                step < self.mate_in,
                &mut undo_moves,
            );
            for undo_move in undo_moves.iter() {
                let mut prev_position = position.clone();
                let movement = prev_position.undo_move(undo_move);

                if memo_previous.get(&prev_position.digest()) == Some(&(step - 1)) {
                    queue.push_back((
                        prev_position,
                        step - 1,
                        MovementList::cons(movement, following_movements.clone()),
                    ));
                }
            }
        }
        res
    }
}
