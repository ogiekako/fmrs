use anyhow::bail;
use fmrs_core::{
    piece::{Color, Kind},
    position::{advance, checked, Position, Square},
    sfen,
};
use log::info;
use nohash_hasher::{IntMap, IntSet};
use rand::{rngs::SmallRng, Rng, SeedableRng};

pub(super) fn generate_one_way_mate_with_sa(seed: u64, iteration: usize) -> anyhow::Result<()> {
    let mut g = Generator::new(seed, iteration, 2.0);
    let problem = g.generate();

    let steps = one_way_mate_steps(problem.clone());

    if let Some(steps) = steps {
        println!(
            "generated problem: {}",
            sfen::sfen_to_image_url(&sfen::encode_position(&problem))
        );
        println!("generated problem steps = {}", steps);
    } else {
        println!("failed to generate a problem");
    }

    Ok(())
}

struct Generator {
    position: Position,
    iteration: usize,
    rng: SmallRng,

    score: f64, // larger is better

    max_temp: f64,

    best_score: f64,
    best_position: Position,
}

impl Generator {
    fn new(seed: u64, iteration: usize, max_temp: f64) -> Self {
        let position = Position::from_sfen("4k4/9/9/9/9/9/9/9/4K4 b 2r2b4g4s4n4l18p 1").unwrap();
        Self {
            position: position.clone(),
            iteration,
            rng: SmallRng::seed_from_u64(seed),
            score: 0.0,
            max_temp,

            best_score: 0.0,
            best_position: position,
        }
    }

    fn generate(&mut self) -> Position {
        for iter in 0..self.iteration {
            if iter * 100 / self.iteration < (iter + 1) * 100 / self.iteration {
                info!(
                    "iter={} score={} best={} temp={:.3} position={}",
                    iter + 1,
                    self.score,
                    self.best_score,
                    self.temp(iter),
                    sfen::sfen_to_image_url(&sfen::encode_position(&self.position))
                );
            }
            self.step(iter);
        }
        self.best_position.clone()
    }

    fn step(&mut self, iter: usize) {
        let action = self.random_action();

        let Ok(undo_action) = action.try_apply(&mut self.position) else {
            return;
        };

        let new_score = score(self.position.clone());

        if self.accept(iter, new_score) {
            self.score = new_score;

            if self.score > self.best_score {
                self.best_score = self.score;
                self.best_position = self.position.clone();
            }

            return;
        }

        undo_action.try_apply(&mut self.position).unwrap();
    }

    fn accept(&mut self, iter: usize, new_score: f64) -> bool {
        let increase = new_score - self.score;
        if increase >= 0. {
            return true;
        }
        let temp = self.temp(iter);
        if temp <= 0. {
            return false;
        }
        self.rng.gen::<f64>() < (increase / temp).exp()
    }

    fn temp(&self, iter: usize) -> f64 {
        self.temp_power(iter, 2.5)
    }

    fn temp_power(&self, iter: usize, exp: f64) -> f64 {
        let t = self.time(iter);
        self.max_temp * t.powf(exp)
    }

    // 1 -> 0
    fn time(&self, iter: usize) -> f64 {
        1. - iter as f64 / self.iteration as f64
    }

    fn random_action(&mut self) -> Action {
        loop {
            match self.rng.gen_range(0..100) {
                0..=9 => return Action::Swap(self.rng.gen(), self.rng.gen()),
                10..=14 => {
                    return Action::FromHand(
                        self.rng.gen(),
                        self.rng.gen(),
                        self.rng.gen(),
                        self.rng.gen(),
                    )
                }
                20..=24 => return Action::ToHand(self.rng.gen(), Color::White),
                30..=30 => return Action::ToHand(self.rng.gen(), Color::Black),
                40..=49 => {
                    return Action::TwoActions(
                        Box::new(self.random_action()),
                        Box::new(self.random_action()),
                    )
                }
                50..=59 => return Action::Move(self.rng.gen(), self.rng.gen()),
                _ => (),
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Action {
    Move(Square, Square),
    Swap(Square, Square),
    FromHand(Color, Square, Color, Kind), // drop to an empty square
    ToHand(Square, Color),                // move a piece to hand
    TwoActions(Box<Action>, Box<Action>),
}

impl Action {
    fn try_apply(self, position: &mut Position) -> anyhow::Result<Action> {
        match self {
            Action::Move(from, to) => {
                if let Some((color, kind)) = position.get(from) {
                    if position.get(to).is_some() {
                        bail!("to is not empty");
                    }
                    position.unset(from, color, kind);
                    position.set(to, color, kind);
                    return Ok(Action::Move(to, from));
                } else {
                    bail!("from is empty");
                }
            }
            Action::Swap(a, b) => {
                match (position.get(a), position.get(b)) {
                    (None, None) => bail!("both are None"),
                    (None, Some((b_color, b_kind))) => {
                        position.unset(b, b_color, b_kind);
                        position.set(a, b_color, b_kind);
                    }
                    (Some((a_color, a_kind)), None) => {
                        position.unset(a, a_color, a_kind);
                        position.set(b, a_color, a_kind);
                    }
                    (Some((a_color, a_kind)), Some((b_color, b_kind))) => {
                        position.unset(a, a_color, a_kind);
                        position.unset(b, b_color, b_kind);
                        position.set(a, b_color, b_kind);
                        position.set(b, a_color, a_kind);
                    }
                }
                return Ok(Action::Swap(a, b));
            }
            Action::FromHand(hand_color, pos, color, kind) => {
                if position.get(pos).is_some() {
                    bail!("to is not empty");
                }
                let hands = position.hands_mut();
                let hand_kind = kind.maybe_unpromote();
                if hands.count(hand_color, hand_kind) == 0 {
                    bail!("no piece in hand");
                }
                hands.remove(hand_color, hand_kind);
                position.set(pos, color, kind);
                return Ok(Action::ToHand(pos, hand_color));
            }
            Action::ToHand(pos, hand_color) => {
                if let Some((color, kind)) = position.get(pos) {
                    if kind == Kind::King {
                        bail!("cannot take king");
                    }
                    let hand_kind = kind.maybe_unpromote();
                    position.hands_mut().add(hand_color, hand_kind);
                    position.unset(pos, color, kind);
                    Ok(Action::FromHand(hand_color, pos, color, kind))
                } else {
                    bail!("from is empty");
                }
            }
            Action::TwoActions(a, b) => {
                let undo_a = a.try_apply(position)?;
                match b.try_apply(position) {
                    Ok(undo_b) => {
                        return Ok(Action::TwoActions(Box::new(undo_b), Box::new(undo_a)))
                    }
                    Err(e) => {
                        undo_a.try_apply(position).unwrap();
                        return Err(e);
                    }
                }
            }
        }
    }
}

fn score(position: Position) -> f64 {
    one_way_mate_steps(position).map_or(0.0, |x| x as f64)
}

fn one_way_mate_steps(mut position: Position) -> Option<usize> {
    if checked(&position, Color::White) {
        return None;
    }

    let mut visited = IntSet::default();
    visited.insert(position.digest());

    // TODO: `advance` without cache.
    for step in (1..).step_by(2) {
        let (white_positions, _) = advance(&position, &mut IntMap::default(), step).unwrap();
        if white_positions.len() != 1 {
            return None;
        }

        let (mut black_positions, is_mate) =
            advance(&white_positions[0], &mut IntMap::default(), step + 1).unwrap();

        if is_mate && !white_positions[0].pawn_drop() {
            if !white_positions[0].hands().is_empty(Color::Black) {
                return None;
            }
            return (step as usize).into();
        }

        if black_positions.len() != 1 {
            return None;
        }
        position = black_positions.remove(0);

        if !visited.insert(position.digest()) {
            return None;
        }
    }
    unreachable!();
}
