use std::ops::Range;

use anyhow::bail;
use log::info;

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
        if search.step % 40 == 0 {
            info!(
                "backward step={} count={}",
                search.step,
                search.positions.len()
            );
        }
    }

    let mut positions = search
        .positions
        .iter()
        .filter(|p| !p.pawn_drop())
        .map(|p| PositionAux::new(p.clone(), *initial_position.stone()))
        .collect::<Vec<_>>();

    if !black_position || search.step % 2 == 1 || search.step == 0 {
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
            if search
                .prev_memo
                .get(&digest)
                .map_or(false, |x| x.is_uniquely(search.step - 1))
            {
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
    memo: NoHashMap<StepRange>,
    prev_memo: NoHashMap<StepRange>,
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
            StepRange::exact(solution.len() as u16),
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
                if ans.is_uniquely(self.step + 1) {
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

const INF_START: u16 = u16::MAX - 2;
const INF_END: u16 = u16::MAX - 1;

fn solutions(
    position: &mut PositionAux,
    memo: &mut NoHashMap<StepRange>,
    next_memo: &mut NoHashMap<StepRange>,
    mate_in: u16,
) -> StepRange {
    let mut ans = StepRange::unknown();
    if let Some(a) = memo.get(&position.digest()) {
        if !a.needs_investigation(mate_in) {
            return a.clone();
        }
        ans = a.clone();
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

    let mut hint = StepRange::unknown();
    if is_mate {
        hint = StepRange::exact(0);
    } else if movements.is_empty() {
        hint = StepRange::unsolvable();
    } else if mate_in == 0 {
        hint = StepRange::non_zero();
    }
    ans = ans.intersection(&hint);
    if !ans.needs_investigation(mate_in) {
        memo.insert(position.digest(), ans.clone());
        return ans;
    }

    let mut res = StepRange::unsolvable();

    for m in movements.iter() {
        let mut np = position.clone();
        np.do_move(m);

        let a = solutions(&mut np, next_memo, memo, mate_in - 1).inc();
        debug_assert!(!a.needs_investigation(mate_in));

        res.update_with_child(&a);

        if res.definitely_shorter_or_non_unique(mate_in) {
            res.shortest.start = 1;
            res.next.start = 1;
            break;
        }
    }

    res = res.intersection(&ans);

    debug_assert!(
        !res.needs_investigation(mate_in),
        "{:?} {:?} {:?} {}",
        res,
        hint,
        position,
        mate_in
    );

    memo.insert(position.digest(), res.clone());
    res
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct StepRange {
    // Second shortest solution range
    next: Range<u16>,
    // Shortest solution range
    shortest: Range<u16>,
}

fn intersection(a: &Range<u16>, b: &Range<u16>) -> Range<u16> {
    let res = a.start.max(b.start)..a.end.min(b.end);
    if res.is_empty() {
        Range::default()
    } else {
        res
    }
}

fn definitely_shorter(r: &Range<u16>, step: u16) -> bool {
    intersection(r, &(step..INF_END)).is_empty()
}

fn definitely_longer(r: &Range<u16>, step: u16) -> bool {
    intersection(r, &(0..step + 1)).is_empty()
}

fn exactly(r: &Range<u16>, step: u16) -> bool {
    r.start == step && r.end == step + 1
}

impl StepRange {
    fn new(mut shortest: Range<u16>, mut next: Range<u16>) -> Self {
        debug_assert!(shortest.start <= next.start);
        debug_assert!(shortest.end <= next.end);

        shortest.start = shortest.start.min(INF_START);
        shortest.end = shortest.end.min(INF_END);
        next.start = next.start.min(INF_START);
        next.end = next.end.min(INF_END);

        StepRange { shortest, next }
    }

    fn exact(step: u16) -> Self {
        Self::new(step..step + 1, step + 1..INF_END)
    }

    fn unsolvable() -> Self {
        Self::new(INF_START..INF_END, INF_START..INF_END)
    }

    fn unknown() -> Self {
        Self::new(0..INF_END, 0..INF_END)
    }

    fn non_zero() -> Self {
        Self::new(1..INF_END, 1..INF_END)
    }

    fn inc(&self) -> Self {
        Self::new(
            self.shortest.start + 1..self.shortest.end + 1,
            self.next.start + 1..self.next.end + 1,
        )
    }

    fn needs_investigation(&self, mate_in: u16) -> bool {
        if definitely_shorter(&self.shortest, mate_in) {
            return false;
        }
        if definitely_longer(&self.shortest, mate_in) {
            return false;
        }
        if exactly(&self.shortest, mate_in) {
            debug_assert!(!definitely_shorter(&self.next, mate_in));
            if definitely_longer(&self.next, mate_in) {
                return false;
            } else if exactly(&self.next, mate_in) {
                return false;
            }
        }
        true
    }

    fn intersection(&self, hint: &StepRange) -> StepRange {
        Self::new(
            intersection(&self.shortest, &hint.shortest),
            intersection(&self.next, &hint.next),
        )
    }

    fn update_with_child(&mut self, c: &StepRange) {
        for &Range { start, end } in [&c.shortest, &c.next] {
            if start < self.shortest.start {
                self.next.start = self.shortest.start;
                self.shortest.start = start;
            } else if start < self.next.start {
                self.next.start = start;
            }

            if end < self.shortest.end {
                self.next.end = self.shortest.end;
                self.shortest.end = end;
            } else if end < self.next.end {
                self.next.end = end;
            }
        }
    }

    fn is_uniquely(&self, step: u16) -> bool {
        exactly(&self.shortest, step) && definitely_longer(&self.next, step)
    }

    fn definitely_shorter_or_non_unique(&self, mate_in: u16) -> bool {
        definitely_shorter(&self.shortest, mate_in)
            || exactly(&self.shortest, mate_in) && exactly(&self.next, mate_in)
    }
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
            (
                "6ppp/6P2/9/9/9/5OOOO/5O2k/5O1PR/5O2P w - 1",
                (0, vec!["6ppp/6P2/9/9/9/5OOOO/5O2k/5O1PR/5O2P w - 1"]),
            ),
        ] {
            let initial_position = PositionAux::from_sfen(sfen).unwrap();
            let (step, mut positions) = backward_search(&initial_position, true).unwrap();

            assert_eq!(step, want_step, "{:?}", initial_position);

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
