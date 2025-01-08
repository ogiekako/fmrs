use std::collections::VecDeque;
use std::rc::Rc;

use crate::memo::MemoTrait;
use crate::nohash::NoHashMap64;

use crate::position::position::PositionAux;
use crate::position::{previous, Movement, PositionExt, UndoMove};

use super::Solution;

pub trait PositionTrait: Clone {
    fn digest(&self) -> u64;
    fn undo_digest(&self, undo_move: &UndoMove) -> u64;
    fn undone(&self, undo_move: &UndoMove) -> Self;
    fn to_position(&self) -> PositionAux;
}

impl PositionTrait for PositionAux {
    fn digest(&self) -> u64 {
        self.digest()
    }
    fn undo_digest(&self, undo_move: &UndoMove) -> u64 {
        PositionExt::undo_digest(self.core(), undo_move)
    }
    fn undone(&self, token: &UndoMove) -> Self {
        let mut p = Self::new(self.core().clone(), *self.stone());
        p.undo_move(token);
        p
    }
    fn to_position(&self) -> PositionAux {
        self.clone()
    }
}

pub fn reconstruct_solutions<M: MemoTrait, P: PositionTrait>(
    mate: &P,
    memo_black_turn: &M,
    memo_white_turn: &M,
    solutions_upto: usize,
) -> Vec<Solution> {
    if solutions_upto == 0 {
        return vec![];
    }
    debug_assert!(memo_white_turn.contains_key(&mate.digest()));
    let step = memo_white_turn.get(&mate.digest()).unwrap();
    let ctx = Context::new(memo_black_turn, memo_white_turn, step, solutions_upto);
    ctx.reconstruct_bfs(mate)
}

#[derive(Debug)]
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
                    res.push(*cur);
                    self = cdr;
                }
            }
        }
    }

    fn is_nil(&self) -> bool {
        matches!(self, Self::Nil)
    }
}

impl Drop for MovementList {
    fn drop(&mut self) {
        loop {
            let MovementList::Cons { cdr, .. } = self else {
                return;
            };
            if cdr.is_nil() {
                return;
            }
            let cdr = std::mem::replace(cdr, MovementList::Nil.into());
            let Ok(cdr) = Rc::try_unwrap(cdr) else { return };
            *self = cdr;
        }
    }
}

struct Context<'a, M: MemoTrait> {
    memo_black_turn: &'a M,
    memo_white_turn: &'a M,
    mate_in: u16,
    solutions_upto: usize,
}

impl<'a, M: MemoTrait> Context<'a, M> {
    fn new(
        memo_black_turn: &'a M,
        memo_white_turn: &'a M,
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

    fn reconstruct_bfs<P: PositionTrait>(&self, mate_position: &P) -> Vec<Solution> {
        let mut position_visit_count = NoHashMap64::default();
        let mut queue: VecDeque<(P, u16, Rc<MovementList>)> = VecDeque::new();
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

            let memo_previous = if (self.mate_in - step) % 2 == 0 {
                self.memo_black_turn
            } else {
                self.memo_white_turn
            };

            undo_moves.clear();

            let mut position_aux = position.to_position();
            previous(&mut position_aux, step < self.mate_in, &mut undo_moves);

            for undo_move in undo_moves.iter() {
                let digest = position.undo_digest(undo_move);

                if memo_previous.get(&digest) == Some(step - 1) {
                    let mut prev_position = position.to_position();
                    let movement = prev_position.undo_move(undo_move);
                    queue.push_back((
                        position.undone(undo_move),
                        step - 1,
                        MovementList::cons(movement, following_movements.clone()),
                    ));
                }
            }
        }
        res
    }
}
