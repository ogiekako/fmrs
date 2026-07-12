use fmrs_core::{
    piece::Color,
    position::position::PositionAux,
    search::backward::{backward_initial_variants, BackwardSearch as BackwardSearchImpl},
};
use wasm_bindgen::prelude::*;

// UI のキャンセル応答性を保ちつつ、JS 側のタイマー・再描画コストが
// 探索本体を上回らない程度の局面数を一度に処理する。
const ADVANCE_BATCH_SIZE: usize = 1_000;

#[wasm_bindgen]
pub struct BackwardSearch {
    inners: Vec<BackwardSearchImpl>,
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
        Self { inners }
    }

    // Returns has next
    pub fn advance(&mut self) -> bool {
        self.inners.iter_mut().any(advance_inner)
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

fn advance_inner(inner: &mut BackwardSearchImpl) -> bool {
    inner.advance_upto(ADVANCE_BATCH_SIZE).unwrap()
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

#[cfg(test)]
mod tests {
    use super::BackwardSearch;

    #[test]
    fn advance_uses_a_large_enough_batch_for_the_web_case() {
        let mut search = BackwardSearch::new(
            "RO3+P3/1OP1P1P2/1OO4+P+P/1O3P1S1/1L3OS2/1L1G1pkN1/GLS1R3B/1P1B1OO2/+p5KNP b P2nl6p 1"
                .to_owned(),
            true,
        );

        assert!(search.advance());
        assert!(search.advance());
        assert!(search.step() >= 23);
    }
}
