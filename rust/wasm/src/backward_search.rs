use fmrs_core::{
    piece::Color, position::position::PositionAux,
    search::backward::BackwardSearch as BackwardSearchImpl,
};
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub struct BackwardSearch {
    inner: BackwardSearchImpl,
}

#[wasm_bindgen]
impl BackwardSearch {
    #[wasm_bindgen(constructor)]
    pub fn new(sfen: String) -> Self {
        let mut position = PositionAux::from_sfen(&sfen).unwrap();
        if position.checked_slow(Color::WHITE) {
            position.set_turn(Color::WHITE);
        }
        let inner = BackwardSearchImpl::new(&position).unwrap();
        Self { inner }
    }

    // Returns has next
    pub fn advance(&mut self) -> bool {
        self.inner.advance().unwrap()
    }

    pub fn step(&self) -> u32 {
        self.inner.step() as u32
    }

    pub fn sfen(&self) -> String {
        let (stone, positions) = self.inner.positions();
        let sfen = PositionAux::new(positions[0].clone(), stone).sfen();
        sfen
    }
}
