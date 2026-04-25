use std::collections::{HashMap, VecDeque};
use std::fmt;
use std::rc::Rc;

use crate::memo::{MemoStub, MemoTrait};
use crate::nohash::{NoHashMap64, NoHashSet64};

use crate::piece::Color;
use crate::position::position::PositionAux;
use crate::position::{previous, previous_with_digest, Movement};

use super::Solution;

pub struct Reconstructor {
    initial_position_digests: NoHashSet64,
    mates: Vec<PositionAux>,
    memo_white_turn: Box<dyn MemoTrait>,
    solutions_upto: usize,
}

impl PartialEq for Reconstructor {
    fn eq(&self, other: &Self) -> bool {
        self.initial_position_digests == other.initial_position_digests
            && self.mates.len() == other.mates.len()
            && self
                .mates
                .iter()
                .zip(other.mates.iter())
                .all(|(a, b)| a.digest() == b.digest())
            && self.solutions_upto == other.solutions_upto
    }
}

impl Eq for Reconstructor {}

impl Reconstructor {
    pub fn no_solution() -> Self {
        Self {
            initial_position_digests: Default::default(),
            mates: vec![],
            memo_white_turn: Box::new(MemoStub),
            solutions_upto: 0,
        }
    }

    pub fn new(
        initial_position_digests: NoHashSet64,
        mates: Vec<PositionAux>,
        memo_white_turn: Box<dyn MemoTrait>,
        solutions_upto: usize,
    ) -> Self {
        Self {
            initial_position_digests,
            mates,
            memo_white_turn,
            solutions_upto,
        }
    }

    pub fn mates(&self) -> &[PositionAux] {
        &self.mates
    }

    pub fn mate_in(&self) -> Option<u16> {
        self.mates
            .first()
            .map(|m| self.memo_white_turn.get(&m.digest()).unwrap())
    }

    pub fn cached_positions(&self) -> usize {
        self.memo_white_turn.len()
    }

    pub fn solutions(&self) -> Vec<Solution> {
        if self.solutions_upto == 0 {
            return vec![];
        }
        let mut res = vec![];

        for mate in self.mates.iter() {
            if res.len() >= self.solutions_upto {
                break;
            }
            let mate_in = self.memo_white_turn.get(&mate.digest()).unwrap();
            let ctx = Context::new(
                &self.initial_position_digests,
                self.memo_white_turn.as_ref(),
                mate_in,
                self.solutions_upto - res.len(),
            );
            res.extend(ctx.reconstruct_bfs(mate));
        }
        res
    }

    pub fn solution_count_exact_decimal(&self) -> String {
        let mut ctx = CountContext::new(
            &self.initial_position_digests,
            self.memo_white_turn.as_ref(),
        );
        let mut total = BigCount::zero();
        for mate in self.mates.iter() {
            let mate_in = self.memo_white_turn.get(&mate.digest()).unwrap();
            total.add_assign(&ctx.count(mate, mate_in, false));
        }
        total.to_string()
    }

    pub fn is_empty(&self) -> bool {
        self.mates.is_empty()
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct BigCount {
    // Base 1e9, little-endian.
    limbs: Vec<u32>,
}

impl BigCount {
    const BASE: u64 = 1_000_000_000;

    fn zero() -> Self {
        Self { limbs: vec![] }
    }

    fn one() -> Self {
        Self { limbs: vec![1] }
    }

    fn add_assign(&mut self, rhs: &Self) {
        if rhs.limbs.is_empty() {
            return;
        }
        if self.limbs.len() < rhs.limbs.len() {
            self.limbs.resize(rhs.limbs.len(), 0);
        }
        let mut carry = 0u64;
        for i in 0..rhs.limbs.len() {
            let sum = self.limbs[i] as u64 + rhs.limbs[i] as u64 + carry;
            self.limbs[i] = (sum % Self::BASE) as u32;
            carry = sum / Self::BASE;
        }
        for i in rhs.limbs.len()..self.limbs.len() {
            if carry == 0 {
                break;
            }
            let sum = self.limbs[i] as u64 + carry;
            self.limbs[i] = (sum % Self::BASE) as u32;
            carry = sum / Self::BASE;
        }
        if carry != 0 {
            self.limbs.push(carry as u32);
        }
    }
}

impl fmt::Display for BigCount {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Some((&last, rest)) = self.limbs.split_last() else {
            return write!(f, "0");
        };
        write!(f, "{last}")?;
        for limb in rest.iter().rev() {
            write!(f, "{limb:09}")?;
        }
        Ok(())
    }
}

struct CountContext<'a> {
    initial_position_digests: &'a NoHashSet64,
    memo_white_turn: &'a dyn MemoTrait,
    memo_count: HashMap<(u64, u16, bool), BigCount>,
}

impl<'a> CountContext<'a> {
    fn new(initial_position_digests: &'a NoHashSet64, memo_white_turn: &'a dyn MemoTrait) -> Self {
        Self {
            initial_position_digests,
            memo_white_turn,
            memo_count: HashMap::new(),
        }
    }

    fn count(
        &mut self,
        white_position: &PositionAux,
        step: u16,
        allow_drop_pawn: bool,
    ) -> BigCount {
        debug_assert_eq!(white_position.turn(), Color::WHITE);

        let digest = white_position.digest();
        if let Some(cached) = self.memo_count.get(&(digest, step, allow_drop_pawn)) {
            return cached.clone();
        }

        let res = if step == 0 {
            if self.initial_position_digests.contains(&digest) {
                BigCount::one()
            } else {
                BigCount::zero()
            }
        } else {
            let mut res = BigCount::zero();
            let mut black_unmoves = vec![];
            let mut white_position = white_position.clone();
            previous(&mut white_position, allow_drop_pawn, &mut black_unmoves);

            for black_unmove in black_unmoves.iter() {
                let mut black_position = white_position.clone();
                black_position.undo_move(black_unmove);

                if black_position.checked_slow(Color::WHITE) {
                    continue;
                }

                if step == 1 {
                    if self
                        .initial_position_digests
                        .contains(&black_position.digest())
                    {
                        res.add_assign(&BigCount::one());
                    }
                    continue;
                }

                let mut white_unmoves = vec![];
                previous_with_digest(&mut black_position, true, |white_unmove, prev_digest| {
                    if self.memo_white_turn.get(&prev_digest) == Some(step - 2) {
                        white_unmoves.push(white_unmove);
                    }
                });

                for white_unmove in white_unmoves {
                    let mut prev_white_position = black_position.clone();
                    prev_white_position.undo_move(&white_unmove);
                    res.add_assign(&self.count(&prev_white_position, step - 2, true));
                }
            }
            res
        };

        self.memo_count
            .insert((digest, step, allow_drop_pawn), res.clone());
        res
    }
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

struct Context<'a> {
    initial_position_digests: &'a NoHashSet64,
    // memo_black_turn: &'a M,
    memo_white_turn: &'a dyn MemoTrait,
    mate_in: u16,
    solutions_upto: usize,
}

impl<'a> Context<'a> {
    fn new(
        initial_position_digests: &'a NoHashSet64,
        // memo_black_turn: &'a M,
        memo_white_turn: &'a dyn MemoTrait,
        mate_in: u16,
        solutions_upto: usize,
    ) -> Self {
        Self {
            initial_position_digests,
            // memo_black_turn,
            memo_white_turn,
            mate_in,
            solutions_upto,
        }
    }

    fn reconstruct_bfs(&self, mate_position: &PositionAux) -> Vec<Solution> {
        let mut position_visit_count = NoHashMap64::default();
        let mut queue: VecDeque<(PositionAux, u16, Rc<MovementList>)> = VecDeque::new();
        queue.push_back((mate_position.clone(), self.mate_in, MovementList::nil()));
        let mut res = vec![];

        let mut black_unmoves = vec![];
        while let Some((mut white_position, step, following_movements)) = queue.pop_front() {
            debug_assert_eq!(white_position.turn(), Color::WHITE);

            if res.len() >= self.solutions_upto {
                break;
            }
            if step == 0 {
                if self
                    .initial_position_digests
                    .contains(&white_position.digest())
                {
                    res.push(following_movements.vec());
                }
                continue;
            }
            {
                let digest = white_position.digest();
                let visit_count = position_visit_count.entry(digest).or_insert(0);
                if *visit_count >= self.solutions_upto as u64 {
                    continue;
                }
                *visit_count += 1;
            }

            black_unmoves.clear();
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
                    if self
                        .initial_position_digests
                        .contains(&black_position.digest())
                    {
                        res.push(following_movements.vec());
                    }
                    continue;
                }

                let mut white_unmoves = vec![];
                previous_with_digest(&mut black_position, true, |white_unmove, digest| {
                    if self.memo_white_turn.get(&digest) == Some(step - 2) {
                        white_unmoves.push(white_unmove);
                    }
                });

                for white_unmove in white_unmoves {
                    let mut prev_white_position = black_position.clone();
                    let white_move = prev_white_position.undo_move(&white_unmove);
                    queue.push_back((
                        prev_white_position,
                        step - 2,
                        MovementList::cons(white_move, following_movements.clone()),
                    ));
                }
            }
        }
        res
    }
}
