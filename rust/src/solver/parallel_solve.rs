use std::sync::{Arc, Mutex};

use fmrs_core::memo::Memo;

use fmrs_core::position::{advance::advance, Digest, Position, PositionExt};

use fmrs_core::solve::{reconstruct_solutions, Solution};

pub(super) fn parallel_solve(
    position: Position,
    _progress: futures::channel::mpsc::UnboundedSender<usize>,
    solutions_upto: usize,
) -> anyhow::Result<Vec<Solution>> {
    let step = 0;
    let mut memo = Memo::default();
    memo.insert(position.digest(), step);
    let memo_next = Memo::default();
    let all_positions = vec![position];

    let task = Task::new(
        all_positions,
        memo,
        memo_next,
        Mutex::new(None).into(),
        solutions_upto,
        Mutex::new(1).into(),
        0,
    );
    let mut res = task.solve(step)?;
    res.sort();
    Ok(res)
}

#[cfg(test)]
const TRIGGER_PARALLEL_SOLVE: usize = 2;
#[cfg(not(test))]
const TRIGGER_PARALLEL_SOLVE: usize = 2_000_000;

lazy_static::lazy_static! {
    static ref NTHREAD: usize = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1).min(32);
}

struct Task {
    all_positions: Vec<Position>,
    memo: Memo,
    memo_next: Memo,
    mate_in: Arc<Mutex<Option<u32>>>,
    solutions_upto: usize,
    active_thread_count: Arc<Mutex<usize>>,
    generation: usize,
}

impl Task {
    fn new(
        all_positions: Vec<Position>,
        memo: Memo,
        memo_next: Memo,
        mate_in: Arc<Mutex<Option<u32>>>,
        solutions_upto: usize,
        active_thread_count: Arc<Mutex<usize>>,
        generation: usize,
    ) -> Self {
        Self {
            all_positions,
            memo,
            memo_next,
            mate_in,
            solutions_upto,
            active_thread_count,
            generation,
        }
    }

    fn spawn_limit(&self, available_memory: usize) -> usize {
        let queue_size = self.all_positions.len() * std::mem::size_of::<Position>();
        if available_memory < queue_size {
            return 0;
        }
        let memo_size = (self.memo.len() + self.memo_next.len())
            * (std::mem::size_of::<Digest>() + std::mem::size_of::<usize>())
            * 2;
        (available_memory - queue_size) / memo_size
    }

    fn solve(mut self, start_step: u32) -> anyhow::Result<Vec<Solution>> {
        let mut mate_positions = vec![];
        let mut all_next_positions = vec![];
        let mut movements = vec![];

        for step in start_step.. {
            let threads_to_spawn = {
                let mut g = self.active_thread_count.lock().unwrap();

                let available_memory =
                    sysinfo::System::new_all().available_memory() as usize * 1024;
                let spawn_limit = self.spawn_limit(available_memory);

                if spawn_limit > 1
                    && step > start_step + 5
                    && self.all_positions.len() >= TRIGGER_PARALLEL_SOLVE
                    && *g < *NTHREAD
                {
                    let threads_to_spawn = (*NTHREAD + 1 - *g).min(spawn_limit);
                    *g += threads_to_spawn - 1;
                    threads_to_spawn.into()
                } else {
                    None
                }
            };
            if let Some(n) = threads_to_spawn {
                let chunk_size = (self.all_positions.len() + n - 1) / n;
                let mut handles = vec![];
                for chunk in self.all_positions.chunks(chunk_size) {
                    let all_positions = chunk.to_vec();
                    let task = Task::new(
                        all_positions,
                        self.memo.clone(),
                        self.memo_next.clone(),
                        self.mate_in.clone(),
                        self.solutions_upto,
                        self.active_thread_count.clone(),
                        self.generation + 1,
                    );
                    handles.push(std::thread::spawn(move || task.solve(step)));
                }
                let (mate_in, solutions_upto) = (self.mate_in, self.solutions_upto);

                let mut all_solutions = vec![];
                for handle in handles {
                    all_solutions.append(&mut handle.join().unwrap()?);
                }
                if all_solutions.is_empty() {
                    return Ok(all_solutions);
                }
                let mate_in = mate_in.lock().unwrap().unwrap();
                let mut shortest_solutions = vec![];
                for solution in all_solutions {
                    if solution.len() == mate_in as usize {
                        shortest_solutions.push(solution);
                    }
                }
                shortest_solutions.sort();
                return Ok(shortest_solutions
                    .into_iter()
                    .take(solutions_upto)
                    .collect());
            }

            let mate_bound = self.mate_in.lock().unwrap().unwrap_or(u32::MAX);
            if step > mate_bound {
                return Ok(vec![]);
            }

            while let Some(mut position) = self.all_positions.pop() {
                let is_mate = advance(
                    &mut position,
                    &mut self.memo_next,
                    step + 1,
                    &Default::default(),
                    &mut movements,
                )?;

                // if step < mate_bound {
                //     all_next_positions.append(&mut new_next_positions);
                // }
                if is_mate {
                    mate_positions.push(position);

                    let mut g = self.mate_in.lock().unwrap();
                    if g.is_none() || g.unwrap() > step {
                        *g = Some(step);
                    }
                    continue;
                }
                while let Some(movement) = movements.pop() {
                    let mut next_position = position.clone();
                    next_position.do_move(&movement);
                    all_next_positions.push(next_position);
                }
            }
            if !mate_positions.is_empty() || all_next_positions.is_empty() {
                break;
            }

            std::mem::swap(&mut self.memo, &mut self.memo_next);
            std::mem::swap(&mut self.all_positions, &mut all_next_positions);
        }

        {
            *self.active_thread_count.lock().unwrap() -= 1;
        }
        let res = std::thread::Builder::new()
            .stack_size(512 * 1024 * 1024)
            .spawn(move || {
                let mut res = vec![];
                for mate_position in mate_positions {
                    res.append(&mut reconstruct_solutions(
                        &mate_position,
                        &self.memo_next,
                        &self.memo,
                        self.solutions_upto - res.len(),
                    ));
                }
                res
            })?
            .join()
            .unwrap();
        Ok(res)
    }
}
