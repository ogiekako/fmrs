use fmrs_core::{
    piece::Color,
    position::position::PositionAux,
    search::backward::{backward_initial_variants, BackwardSearch as BackwardSearchImpl},
    solve::one_way::one_way_mate_steps,
};
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub struct BackwardSearch {
    inners: Vec<BackwardSearchImpl>,
    one_way_mate_mode: bool,
}

#[wasm_bindgen]
impl BackwardSearch {
    #[wasm_bindgen(constructor)]
    pub fn new(sfen: String, one_way_mate_mode: bool) -> Self {
        let mut position = PositionAux::from_sfen(&sfen).unwrap();
        if position.checked_slow(Color::WHITE) {
            position.set_turn(Color::WHITE);
        }
        let inners = backward_initial_variants(&position)
            .into_iter()
            .filter_map(|variant| BackwardSearchImpl::new(&variant, one_way_mate_mode).ok())
            .collect::<Vec<_>>();
        assert!(!inners.is_empty(), "failed to initialize backward search");
        Self {
            inners,
            one_way_mate_mode,
        }
    }

    // Returns has next
    pub fn advance(&mut self) -> bool {
        self.inners
            .iter_mut()
            .any(|inner| advance_inner(inner, self.one_way_mate_mode))
    }

    pub fn step(&self) -> u32 {
        self.best_index()
            .map_or(0, |index| self.inners[index].step() as u32)
    }

    pub fn sfen(&self) -> String {
        let index = self.best_index().unwrap();
        let (stone, positions) = self.inners[index].positions();

        PositionAux::new(positions[0].clone(), stone).sfen()
    }
}

fn advance_inner(inner: &mut BackwardSearchImpl, one_way_mate_mode: bool) -> bool {
    if one_way_mate_mode {
        inner
            .advance_upto_with_filter(10, |core, stone| {
                let mut p = PositionAux::new(core.clone(), stone);
                if p.checked_slow(Color::WHITE) {
                    p.set_turn(Color::WHITE);
                }
                one_way_mate_steps(&mut p, &mut vec![]).is_ok()
            })
            .unwrap()
    } else {
        inner.advance_upto(10).unwrap()
    }
}

impl BackwardSearch {
    fn best_index(&self) -> Option<usize> {
        self.inners
            .iter()
            .enumerate()
            .max_by_key(|(_, inner)| inner.step())
            .map(|(index, _)| index)
    }
}
