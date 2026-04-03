mod backward_search;
mod solver;
mod utils;

use wasm_bindgen::prelude::*;

#[wasm_bindgen]
extern "C" {
    fn alert(s: &str);

    #[wasm_bindgen(js_namespace = console)]
    pub fn log(s: &str);
}

#[wasm_bindgen]
pub struct OneWayMateResult {
    pub is_one_way: bool,
    pub steps: u32,
}

#[wasm_bindgen]
pub fn check_one_way_mate(sfen: &str) -> Option<OneWayMateResult> {
    let mut position = fmrs_core::position::position::PositionAux::from_sfen(sfen).ok()?;
    if position.checked_slow(fmrs_core::piece::Color::WHITE) {
        position.set_turn(fmrs_core::piece::Color::WHITE);
    }
    match fmrs_core::solve::one_way::one_way_mate_steps(&mut position, &mut vec![]) {
        Ok(s) => Some(OneWayMateResult { is_one_way: true, steps: s as u32 }),
        Err(s) => Some(OneWayMateResult { is_one_way: false, steps: s as u32 }),
    }
}
