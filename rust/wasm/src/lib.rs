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

/// アプリ本体は TS 版 (app/src/model/check.ts) を使う。これはその正解として
/// テスト (app/test/check.test.ts) から呼ばれる。
#[wasm_bindgen]
pub fn is_white_in_check(sfen: &str) -> bool {
    let Ok(mut position) = fmrs_core::position::position::PositionAux::from_sfen(sfen) else {
        return false;
    };
    // 後手玉が無いと checked_slow は panic する。wasm では panic のたびに
    // スタックが戻らず、繰り返すとインスタンスが壊れる。
    if position
        .bitboard(
            fmrs_core::piece::Color::WHITE,
            fmrs_core::piece::Kind::King,
        )
        .is_empty()
    {
        return false;
    }
    position.checked_slow(fmrs_core::piece::Color::WHITE)
}

#[wasm_bindgen]
pub fn check_one_way_mate(sfen: &str) -> Option<OneWayMateResult> {
    let mut position = fmrs_core::position::position::PositionAux::from_sfen(sfen).ok()?;
    if position.checked_slow(fmrs_core::piece::Color::WHITE) {
        position.set_turn(fmrs_core::piece::Color::WHITE);
    }
    match fmrs_core::solve::one_way::one_way_mate_steps(&mut position, &mut vec![]) {
        Ok(s) => Some(OneWayMateResult {
            is_one_way: true,
            steps: s as u32,
        }),
        Err(s) => Some(OneWayMateResult {
            is_one_way: false,
            steps: s as u32,
        }),
    }
}

#[wasm_bindgen]
pub fn moves_to_kif(sfen: &str, moves_sfen: &str) -> Option<String> {
    let mut position = fmrs_core::position::position::PositionAux::from_sfen(sfen).ok()?;
    let moves = fmrs_core::sfen::decode_moves(moves_sfen).ok()?;
    Some(fmrs_core::converter::convert_to_kif(
        &mut position,
        &[moves],
    ))
}

#[cfg(test)]
mod tests {
    use super::moves_to_kif;

    #[test]
    fn test_moves_to_kif() {
        let sfen = "9/9/3b5/2n1+P4/3k5/6K2/9/3+r5/9 b BL 1";
        let moves = "B*9h 6h9h L*6i 9h6h 6i6h 6e5f R*5e";
        let kif = moves_to_kif(sfen, moves).expect("moves_to_kif failed");
        assert!(kif.contains("９八角打"));
        assert!(kif.contains("５五飛打"));
    }
}
