use fmrs_core::{piece::Color, position::Position, sfen};
use log::info;
use rand::{rngs::SmallRng, Rng, SeedableRng};

use super::{action::Action, solve::one_way_mate_steps};

pub(super) fn generate_one_way_mate_with_sa(seed: u64, iteration: usize) -> anyhow::Result<()> {
    let mut g = Generator::new(seed, iteration, 2.0);
    let problem = g.generate();

    let steps = one_way_mate_steps(&problem);

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

fn score(position: Position) -> f64 {
    one_way_mate_steps(&position).map_or(0.0, |x| x as f64)
}
