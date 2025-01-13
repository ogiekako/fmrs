use std::collections::VecDeque;
use std::rc::Rc;

use crate::memo::MemoTrait;
use crate::nohash::NoHashMap64;

use crate::piece::Color;
use crate::position::position::PositionAux;
use crate::position::{previous, Movement, UndoMove};

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
        PositionAux::undo_digest(self, undo_move)
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
    initial_position_digest: u64,
    mate: &P,
    memo_white_turn: &M,
    solutions_upto: usize,
) -> Vec<Solution> {
    if solutions_upto == 0 {
        return vec![];
    }
    debug_assert!(memo_white_turn.contains_key(&mate.digest()));
    let step = memo_white_turn.get(&mate.digest()).unwrap();
    let ctx = Context::new(
        initial_position_digest,
        memo_white_turn,
        step,
        solutions_upto,
    );
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
    initial_position_digest: u64,
    // memo_black_turn: &'a M,
    memo_white_turn: &'a M,
    mate_in: u16,
    solutions_upto: usize,
}

impl<'a, M: MemoTrait> Context<'a, M> {
    fn new(
        initial_position_digest: u64,
        // memo_black_turn: &'a M,
        memo_white_turn: &'a M,
        mate_in: u16,
        solutions_upto: usize,
    ) -> Self {
        Self {
            initial_position_digest,
            // memo_black_turn,
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

        let mut black_unmoves = vec![];
        let mut white_unmoves = vec![];
        while let Some((generic_white_position, step, following_movements)) = queue.pop_front() {
            debug_assert_eq!(generic_white_position.to_position().turn(), Color::WHITE);

            if res.len() >= self.solutions_upto {
                break;
            }
            if step == 0 {
                if generic_white_position.digest() != self.initial_position_digest {
                    continue;
                }
                res.push(following_movements.vec());
                continue;
            }
            {
                let digest = generic_white_position.digest();
                let visit_count = position_visit_count.entry(digest).or_insert(0);
                if *visit_count >= self.solutions_upto as u64 {
                    continue;
                }
                *visit_count += 1;
            }

            black_unmoves.clear();
            let mut white_position = generic_white_position.to_position();
            previous(&mut white_position, step < self.mate_in, &mut black_unmoves);

            for black_unmove in black_unmoves.iter() {
                if res.len() >= self.solutions_upto {
                    break;
                }
                let mut black_position = white_position.clone();
                let black_move = black_position.undo_move(black_unmove);

                if black_position.checked_slow(Color::WHITE) {
                    continue;
                }

                let following_movements =
                    MovementList::cons(black_move, following_movements.clone());

                if step == 1 {
                    if self.initial_position_digest
                        == generic_white_position.undo_digest(black_unmove)
                    {
                        res.push(following_movements.vec());
                    }
                    continue;
                }

                white_unmoves.clear();
                previous(&mut black_position, true, &mut white_unmoves);

                let generic_black_position = generic_white_position.undone(black_unmove);
                for white_unmove in white_unmoves.iter() {
                    let digest = generic_black_position.undo_digest(white_unmove);
                    if self.memo_white_turn.get(&digest) != Some(step - 2) {
                        continue;
                    }
                    let wp = generic_black_position.undone(white_unmove);
                    let white_move = black_position.clone().undo_move(white_unmove);
                    queue.push_back((
                        wp,
                        step - 2,
                        MovementList::cons(white_move, following_movements.clone()),
                    ));
                }
            }
        }
        res
    }
}
