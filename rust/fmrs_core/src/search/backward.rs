use anyhow::bail;

use crate::{
    memo::MemoStub,
    nohash::NoHashMap,
    piece::Color,
    position::{
        advance::advance::advance_aux, position::PositionAux, previous, AdvanceOptions, BitBoard,
        Position,
    },
    solve::standard_solve::standard_solve,
};

pub fn backward_search(
    initial_position: &PositionAux,
    black_position: bool,
) -> anyhow::Result<(u16, Vec<PositionAux>)> {
    let mut search = BackwardSearch::new(initial_position)?;

    loop {
        if !search.advance()? {
            break;
        }
    }

    let mut positions = search
        .positions
        .iter()
        .filter(|p| !p.pawn_drop())
        .map(|p| PositionAux::new(p.clone(), *initial_position.stone()))
        .collect::<Vec<_>>();

    if !black_position || search.step % 2 == 1 {
        return Ok((search.step, positions));
    }

    let mut black_positions = vec![];
    for p in positions.iter_mut() {
        assert_eq!(p.turn(), Color::WHITE);
        let mut movements = vec![];
        advance_aux(
            p,
            &mut MemoStub::default(),
            0,
            &AdvanceOptions {
                no_memo: true,
                ..Default::default()
            },
            &mut movements,
        )?;
        for m in movements.iter() {
            let digest = p.moved_digest(&m);
            if search.prev_memo.get(&digest) == Some(&Step::Exact(search.step - 1, 1)) {
                let mut np = p.clone();
                np.do_move(m);
                black_positions.push(np);
            }
        }
    }
    Ok((search.step - 1, black_positions))
}

pub struct BackwardSearch {
    positions: Vec<Position>,
    prev_positions: Vec<Position>,
    memo: NoHashMap<Step>,
    prev_memo: NoHashMap<Step>,
    stone: Option<BitBoard>,
    step: u16,
}

impl BackwardSearch {
    pub fn new(initial_position: &PositionAux) -> anyhow::Result<Self> {
        let mut solution = standard_solve(initial_position.clone(), 2, true)?;
        if solution.len() != 1 {
            bail!("Not unique: {}", solution.len());
        }
        let solution = solution.remove(0);
        let mut p = initial_position.clone();
        for m in solution.iter() {
            p.do_move(m);
        }
        if !p.hands().is_empty(Color::BLACK) {
            bail!("Extra black pieces in checkmate");
        }

        let positions = vec![initial_position.core().clone()];

        let mut memo = NoHashMap::default();
        memo.insert(
            initial_position.digest(),
            Step::Exact(solution.len() as u16, 1),
        );

        Ok(BackwardSearch {
            positions,
            prev_positions: vec![],
            memo,
            prev_memo: Default::default(),
            stone: *initial_position.stone(),
            step: solution.len() as u16,
        })
    }

    pub fn advance(&mut self) -> anyhow::Result<bool> {
        self.prev_positions.clear();

        for core in self.positions.iter() {
            let mut position = PositionAux::new(core.clone(), self.stone);
            let mut undo_moves = vec![];
            previous(&mut position, self.step > 0, &mut undo_moves);

            for m in undo_moves.iter() {
                let mut pp = position.clone();
                pp.undo_move(m);

                if pp.turn().is_white() {
                    if !pp.checked_slow(Color::WHITE) || pp.checked_slow(Color::BLACK) {
                        continue;
                    }
                } else {
                    if pp.checked_slow(Color::WHITE) {
                        continue;
                    }
                }

                let ans = solutions(&mut pp, &mut self.prev_memo, &mut self.memo, self.step + 1);
                if ans == Step::Exact(self.step + 1, 1) {
                    #[cfg(debug_assertions)]
                    {
                        let sol = standard_solve(pp.clone(), 2, true).unwrap();
                        if sol.len() != 1 {
                            eprintln!("Not unique: {} {}", sol.len(), pp.sfen_url());
                            for sol in sol.iter() {
                                let m = &sol[0];
                                let mut p = pp.clone();
                                p.do_move(m);
                                eprintln!(
                                    "{} {} {:?} {:?}",
                                    self.step,
                                    p.sfen_url(),
                                    m,
                                    self.memo.get(&p.digest())
                                );
                            }
                            debug_assert_eq!(sol.len(), 1);
                        }
                    }

                    self.prev_positions.push(pp.core().clone());
                }
            }
        }

        if self.prev_positions.is_empty() {
            return Ok(false);
        }

        std::mem::swap(&mut self.positions, &mut self.prev_positions);
        std::mem::swap(&mut self.memo, &mut self.prev_memo);
        self.step += 1;

        Ok(true)
    }
}

const INF: u16 = u16::MAX - 1;

fn solutions(
    position: &mut PositionAux,
    memo: &mut NoHashMap<Step>,
    next_memo: &mut NoHashMap<Step>,
    mate_in: u16,
) -> Step {
    let mut ans = Step::MoreThan(INF);
    if let Some(a) = memo.get(&position.digest()) {
        let Step::MoreThan(step) = a else {
            return *a;
        };
        if mate_in <= *step {
            return *a;
        }
    }

    let mut movements = vec![];
    let is_mate = advance_aux(
        position,
        &mut MemoStub::default(),
        0,
        &AdvanceOptions {
            no_memo: true,
            ..Default::default()
        },
        &mut movements,
    )
    .unwrap();

    if is_mate {
        return Step::Exact(0, 1);
    } else if movements.is_empty() {
        return Step::MoreThan(INF);
    } else if mate_in == 0 {
        return Step::MoreThan(0);
    }

    for m in movements.iter() {
        let mut np = position.clone();
        np.do_move(m);

        let a = solutions(&mut np, next_memo, memo, mate_in - 1);
        match a {
            Step::MoreThan(x) => match ans {
                Step::MoreThan(y) => {
                    ans = Step::MoreThan((x + 1).min(y));
                }
                _ => (),
            },
            Step::Exact(s, m) => match ans {
                Step::MoreThan(step) => {
                    if s + 1 <= mate_in {
                        ans = Step::Exact(s + 1, m);
                    } else if s <= step {
                        ans = Step::MoreThan(s);
                    }
                }
                Step::Exact(step, n) => {
                    if s + 1 == step {
                        ans = Step::Exact(s + 1, (n + m).min(2));
                    } else if s + 1 < step {
                        ans = Step::Exact(s + 1, m);
                    } else {
                        // Do nothing
                    }
                }
            },
        }
    }

    memo.insert(position.digest(), ans);
    ans
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Step {
    Exact(u16, u8),
    MoreThan(u16),
}

#[cfg(test)]
mod tests {
    use crate::{position::position::PositionAux, search::backward::backward_search};

    #[test]
    fn test_backward_search() {
        for (sfen, (want_step, mut want_sfens)) in [
            (
                "9/9/9/9/9/6OOO/6O1k/6OO+P/8P w - 1",
                (1, vec!["9/9/9/9/9/6OOO/6O1k/6OO1/7+PP b - 1"]),
            ),
            (
                "9/9/9/7OO/7Ok/7OP/7O1/7O1/7OL w - 1",
                (3, vec!["9/9/9/7OO/7O1/7Ok/7O1/7OP/7OL b - 1"]),
            ),
            (
                "9/9/9/7OO/7Ok/7O1/7OP/7O1/7OL b - 1",
                (3, vec!["9/9/9/7OO/7O1/7Ok/7O1/7OP/7OL b - 1"]),
            ),
            (
                "9/9/9/9/9/5OOOO/5OR1k/5O1p1/5O2P w - 1",
                (
                    19,
                    vec![
                        "9/9/9/9/9/5OOOO/5O2+p/5Ok+p1/5O2R b - 1",
                        "9/9/9/9/9/5OOOO/5O2R/5Ok+p1/5O2+p b - 1",
                        "9/9/9/9/9/5OOOO/5O2p/5Ok+p1/5O2R b - 1",
                    ],
                ),
            ),
        ] {
            let initial_position = PositionAux::from_sfen(sfen).unwrap();
            let (step, mut positions) = backward_search(&initial_position, true).unwrap();

            assert_eq!(step, want_step);

            want_sfens.sort();
            let want_positions = want_sfens
                .iter()
                .map(|sfen| PositionAux::from_sfen(sfen).unwrap())
                .collect::<Vec<_>>();

            positions.sort_by(|a, b| a.clone().sfen().cmp(&b.clone().sfen()));

            assert_eq!(positions, want_positions)
        }
    }
}
