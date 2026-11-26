use std::sync::Arc;

use fmrs_core::{
    converter, sfen,
    shtsume::{pv_to_movements, FmrsPos, SearchConfig, Solver},
    validate::decode_and_validate_futsuu_position,
};
use wasm_bindgen::prelude::wasm_bindgen;

use crate::utils::set_panic_hook;

#[wasm_bindgen]
pub struct FutsuuSolveResult {
    /// 1 = mate, 0 = no mate, -1 = unknown / suspended
    mate_status: i8,
    steps: u32,
    redundant: bool,
    nodes: f64,
    kif: String,
    sfen: String,
    moves_sfen: String,
}

#[wasm_bindgen]
impl FutsuuSolveResult {
    pub fn is_mate(&self) -> bool {
        self.mate_status == 1
    }

    pub fn is_no_mate(&self) -> bool {
        self.mate_status == 0
    }

    pub fn is_suspended(&self) -> bool {
        self.mate_status < 0
    }

    pub fn steps(&self) -> u32 {
        self.steps
    }

    pub fn redundant(&self) -> bool {
        self.redundant
    }

    pub fn nodes(&self) -> f64 {
        self.nodes
    }

    pub fn kif(&self) -> String {
        self.kif.clone()
    }

    pub fn sfen(&self) -> String {
        self.sfen.clone()
    }

    pub fn moves_sfen(&self) -> String {
        self.moves_sfen.clone()
    }
}

pub fn solve_futsuu_core(
    problem_sfen: &str,
    simple_mate: bool,
    hash_mb: usize,
    progress_cb: Option<Arc<dyn Fn(u64) + Send + Sync>>,
) -> Result<FutsuuSolveResult, String> {
    let mut position = decode_and_validate_futsuu_position(problem_sfen)?;
    let root = FmrsPos::from_pos_aux_root(position.clone());

    let cfg = SearchConfig {
        hash_mb: hash_mb.clamp(8, 256),
        threads: 1,
        simple_mate,
        use_ni_cache: true,
        use_ml_ordering: true,
        ..Default::default()
    };

    let mut solver = Solver::new(cfg);
    solver.set_progress_callback(progress_cb);
    let res = solver.solve(&root);
    if res.error {
        return Err("内部エラーにより探索を継続できませんでした。".to_string());
    }

    let (mate_status, steps, redundant, kif, moves_sfen) = match res.mate {
        Some(true) => {
            let movements = pv_to_movements(&res.pv);
            let kif = converter::convert_to_kif(&mut position, std::slice::from_ref(&movements));
            let moves_sfen = movements
                .iter()
                .map(sfen::encode_move)
                .collect::<Vec<_>>()
                .join(" ");
            (
                1,
                res.mate_steps as u32,
                res.redundant,
                kif,
                moves_sfen,
            )
        }
        Some(false) => (0, 0, false, String::new(), String::new()),
        None => (-1, 0, false, String::new(), String::new()),
    };

    Ok(FutsuuSolveResult {
        mate_status,
        steps,
        redundant,
        nodes: res.nodes as f64,
        kif,
        sfen: position.sfen(),
        moves_sfen,
    })
}

#[cfg(target_arch = "wasm32")]
struct SendSyncJsCallback(js_sys::Function);

// SAFETY: `wasm32-unknown-unknown` without the `atomics` feature is strictly single-threaded.
#[cfg(target_arch = "wasm32")]
unsafe impl Send for SendSyncJsCallback {}
#[cfg(target_arch = "wasm32")]
unsafe impl Sync for SendSyncJsCallback {}

#[wasm_bindgen]
pub fn solve_futsuu(
    problem_sfen: String,
    simple_mate: bool,
    hash_mb: u32,
    on_progress: Option<js_sys::Function>,
) -> Result<FutsuuSolveResult, String> {
    set_panic_hook();

    #[cfg(target_arch = "wasm32")]
    let progress_cb: Option<Arc<dyn Fn(u64) + Send + Sync>> = on_progress.map(|f| {
        let cb = SendSyncJsCallback(f);
        Arc::new(move |nodes: u64| {
            let _ = cb.0.call1(
                &wasm_bindgen::JsValue::NULL,
                &wasm_bindgen::JsValue::from_f64(nodes as f64),
            );
        }) as Arc<dyn Fn(u64) + Send + Sync>
    });

    #[cfg(not(target_arch = "wasm32"))]
    let progress_cb: Option<Arc<dyn Fn(u64) + Send + Sync>> = {
        let _ = on_progress;
        None
    };

    solve_futsuu_core(&problem_sfen, simple_mate, hash_mb as usize, progress_cb)
}

#[cfg(test)]
mod tests {
    use super::solve_futsuu_core;

    #[test]
    fn test_solve_futsuu_3ply_and_kif() {
        let sfen = "3sks3/9/4S4/9/9/B8/9/9/9 b S2rb4g4n4l18p 1";
        let res = solve_futsuu_core(sfen, false, 16, None).unwrap();
        assert!(res.is_mate());
        assert_eq!(res.steps(), 3);
        assert!(!res.redundant());
        assert!(res.kif().contains("手数----指手"));
        assert!(!res.moves_sfen().is_empty());
    }

    #[test]
    fn test_solve_futsuu_no_mate_drop_pawn_mate() {
        let sfen = "9/9/5lp2/5lk2/5l3/9/5N3/7L1/9 b P2r2b4g4s3n16p 1";
        let res = solve_futsuu_core(sfen, false, 16, None).unwrap();
        assert!(res.is_no_mate());
    }

    #[test]
    fn test_solve_futsuu_rejects_checked_white_and_stones() {
        assert_eq!(
            solve_futsuu_core("4k4/4G4/9/9/9/9/9/9/9 b - 1", false, 16, None)
                .err()
                .unwrap(),
            "受方玉にすでに王手がかかっています。"
        );
        assert_eq!(
            solve_futsuu_core("4k4/9/4O4/9/9/9/9/9/9 b G 1", false, 16, None)
                .err()
                .unwrap(),
            "普通詰将棋では石（壁駒）は使用できません。"
        );
    }
}
