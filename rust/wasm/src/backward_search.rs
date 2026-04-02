use fmrs_core::{
    piece::Color, position::position::PositionAux,
    search::backward::BackwardSearch as BackwardSearchImpl,
    solve::one_way::one_way_mate_steps,
};
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub struct BackwardSearch {
    inner: BackwardSearchImpl,
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
        let inner = BackwardSearchImpl::new(&position).unwrap();
        Self { inner, one_way_mate_mode }
    }

    // Returns has next
    pub fn advance(&mut self) -> bool {
        if self.one_way_mate_mode {
            self.inner.advance_upto_with_filter(10, |core, stone| {
                let mut p = PositionAux::new(core.clone(), stone);
                if p.checked_slow(Color::WHITE) {
                    p.set_turn(Color::WHITE);
                }
                one_way_mate_steps(&mut p, &mut vec![]).is_some()
            }).unwrap()
        } else {
            self.inner.advance_upto(10).unwrap()
        }
    }

    pub fn step(&self) -> u32 {
        self.inner.step() as u32
    }

    pub fn sfen(&self) -> String {
        let (stone, positions) = self.inner.positions();

        PositionAux::new(positions[0].clone(), stone).sfen()
    }
}
