use std::collections::VecDeque;

use crate::memo::Memo;
use crate::nohash::NoHashMap;

use crate::position::{previous, Position, PositionExt};

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

struct Context<'a> {
    memo_black_turn: &'a Memo,
    memo_white_turn: &'a Memo,
    mate_in: u32,
    solutions_upto: usize,
}

impl<'a> Context<'a> {
    fn new(
        memo_black_turn: &'a Memo,
        memo_white_turn: &'a Memo,
        mate_in: u32,
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
        let mut queue = VecDeque::new();
        queue.push_back((mate_position.clone(), self.mate_in, vec![]));
        let mut res = vec![];
        while let Some((mut position, step, mut solution_rev)) = queue.pop_front() {
            if res.len() >= self.solutions_upto {
                break;
            }
            if step == 0 {
                res.push(solution_rev.into_iter().rev().collect::<Vec<_>>());
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

            #[cfg(debug_assertions)]
            let mut positions: std::collections::HashMap<u64, Position> =
                std::collections::HashMap::new();

            for undo_move in previous(position.clone(), step < self.mate_in) {
                let movement = position.undo_move(&undo_move);

                #[cfg(debug_assertions)]
                {
                    debug_assert!(
                        !positions.contains_key(&position.digest()),
                        "{:?} {:?} {} {} {}",
                        positions.get(&position.digest()).unwrap(),
                        position,
                        positions.get(&position.digest()).unwrap() == &position,
                        position.digest(),
                        positions.get(&position.digest()).as_ref().unwrap().digest(),
                    );
                    positions.insert(position.digest(), position.clone());
                    pretty_assertions::assert_eq!(
                        position,
                        {
                            let sfen = position.sfen();
                            Position::from_sfen(&sfen).unwrap()
                        },
                        "{:?}",
                        position
                    );
                }

                if memo_previous.get(&position.digest()) == Some(&(step - 1)) {
                    solution_rev.push(movement);
                    queue.push_back((position.clone(), step - 1, solution_rev.clone())); // TODO: avoid O(n^2) operation
                    solution_rev.pop().unwrap();
                }
                position.do_move(&movement);
            }
        }
        res
    }
}
