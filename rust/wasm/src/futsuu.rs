use std::sync::Arc;

use fmrs_core::{
    converter,
    futsuu::{
        perfection::{
            format_futsuu_move_ja, ConventionalVerifier, FlawKind, PerfectionConfig,
            PerfectionStatus,
        },
        pv_to_movements, FmrsPos, SearchConfig, Solver,
    },
    sfen,
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
    perfection_status: String,
    shortcut_ply: u32,
    shortcut_total_steps: u32,
    shortcut_move_kif: String,
    intended_move_kif: String,
    minor_flaw_count: u32,
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

    pub fn perfection_status(&self) -> String {
        self.perfection_status.clone()
    }

    pub fn shortcut_ply(&self) -> u32 {
        self.shortcut_ply
    }

    pub fn shortcut_total_steps(&self) -> u32 {
        self.shortcut_total_steps
    }

    pub fn shortcut_move_kif(&self) -> String {
        self.shortcut_move_kif.clone()
    }

    pub fn intended_move_kif(&self) -> String {
        self.intended_move_kif.clone()
    }

    pub fn minor_flaw_count(&self) -> u32 {
        self.minor_flaw_count
    }
}

pub fn solve_futsuu_core(
    problem_sfen: &str,
    hash_mb: usize,
    check_perfection: bool,
    progress_cb: Option<Arc<dyn Fn(u64) + Send + Sync>>,
) -> Result<FutsuuSolveResult, String> {
    let mut position = decode_and_validate_futsuu_position(problem_sfen)?;
    let root = FmrsPos::from_pos_aux_root(position.clone());

    let clamped_hash = hash_mb.clamp(8, 256);
    let cfg = SearchConfig {
        hash_mb: clamped_hash,
        threads: 1,
        simple_mate: false,
        use_ni_cache: true,
        use_ml_ordering: true,
        ..Default::default()
    };

    let mut solver = Solver::new(cfg);
    solver.set_progress_callback(progress_cb.clone());
    let res = if check_perfection {
        solver.solve_unbounded_pn(&root)
    } else {
        solver.solve(&root)
    };
    if res.error {
        return Err("内部エラーにより探索を継続できませんでした。".to_string());
    }

    let mut perfection_status = String::new();
    let mut shortcut_ply = 0u32;
    let mut shortcut_total_steps = 0u32;
    let mut shortcut_move_kif = String::new();
    let mut intended_move_kif = String::new();
    let mut minor_flaw_count = 0u32;
    let mut total_nodes = res.nodes;

    let (mate_status, steps, redundant, kif, moves_sfen) = match res.mate {
        Some(true) => {
            if check_perfection {
                // Drop main solver table before allocating verifier tables inside 32-bit WASM memory.
                drop(solver);
                let mut verifier = ConventionalVerifier::new(PerfectionConfig {
                    hash_mb: (clamped_hash / 2).clamp(16, 64),
                    solve_node_limit: 2_000_000,
                    check_node_limit_per_move: 200_000,
                    search_level: 1,
                    fail_fast: true,
                    endgame_transposition_rem_sh: 5,
                });
                verifier.set_progress_callback(progress_cb);
                let orig_pv = res.pv.clone();
                let orig_steps = res.mate_steps;
                let rep = verifier.verify_solved(&root, res);
                total_nodes = rep.solve_nodes + rep.check_nodes;

                if matches!(rep.status, PerfectionStatus::NoMate) {
                    (0, 0, false, String::new(), String::new())
                } else {
                    let final_pv = if !rep.pv.is_empty() { rep.pv } else { orig_pv };
                    let final_steps = if rep.mate_steps > 0 {
                        rep.mate_steps
                    } else {
                        orig_steps
                    };
                    let mut final_redundant = false;
                    let main_movements = pv_to_movements(&final_pv);
                    let mut solutions = vec![main_movements.clone()];

                    match rep.status {
                        PerfectionStatus::Perfect => {
                            perfection_status = "perfect".to_string();
                            minor_flaw_count = rep.minor_flaw_count as u32;
                        }
                        PerfectionStatus::Shortcut {
                            first_ply,
                            alt_move,
                            intended_move,
                            kind,
                        } => {
                            perfection_status = match kind {
                                FlawKind::EarlyShortcut => "early_shortcut".to_string(),
                                FlawKind::EqualLengthShortcut => "equal_shortcut".to_string(),
                            };
                            shortcut_ply = first_ply as u32 + 1;
                            minor_flaw_count = rep.minor_flaw_count as u32;
                            let mut pos_at_ply = position.clone();
                            for m in main_movements.iter().take(first_ply as usize) {
                                pos_at_ply.do_move(m);
                            }
                            shortcut_move_kif = format_futsuu_move_ja(&pos_at_ply, alt_move);
                            intended_move_kif = format_futsuu_move_ja(&pos_at_ply, intended_move);
                            if !rep.shortcut_pv.is_empty() {
                                let sc_movements = pv_to_movements(&rep.shortcut_pv);
                                shortcut_total_steps =
                                    fmrs_core::futsuu::solver::effective_tsume_pv_plies(
                                        &root,
                                        &rep.shortcut_pv,
                                    ) as u32;
                                solutions.push(sc_movements);
                            }
                        }
                        PerfectionStatus::RedundantHand => {
                            final_redundant = true;
                            perfection_status = "redundant".to_string();
                        }
                        PerfectionStatus::Timeout | PerfectionStatus::NoMate => {
                            perfection_status = "timeout".to_string();
                            minor_flaw_count = rep.minor_flaw_count as u32;
                        }
                    }

                    let kif = converter::convert_to_kif(&mut position, &solutions);
                    let moves_sfen = main_movements
                        .iter()
                        .map(sfen::encode_move)
                        .collect::<Vec<_>>()
                        .join(" ");
                    (1, final_steps as u32, final_redundant, kif, moves_sfen)
                }
            } else {
                let movements = pv_to_movements(&res.pv);
                let kif =
                    converter::convert_to_kif(&mut position, std::slice::from_ref(&movements));
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
        }
        Some(false) => (0, 0, false, String::new(), String::new()),
        None => (-1, 0, false, String::new(), String::new()),
    };

    Ok(FutsuuSolveResult {
        mate_status,
        steps,
        redundant,
        nodes: total_nodes as f64,
        kif,
        sfen: position.sfen(),
        moves_sfen,
        perfection_status,
        shortcut_ply,
        shortcut_total_steps,
        shortcut_move_kif,
        intended_move_kif,
        minor_flaw_count,
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
    hash_mb: u32,
    check_perfection: Option<bool>,
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

    solve_futsuu_core(
        &problem_sfen,
        hash_mb as usize,
        check_perfection.unwrap_or(false),
        progress_cb,
    )
}

#[cfg(test)]
mod tests {
    use super::solve_futsuu_core;

    #[test]
    fn test_solve_futsuu_3ply_and_kif() {
        let sfen = "3sks3/9/4S4/9/9/B8/9/9/9 b S2rb4g4n4l18p 1";
        let res = solve_futsuu_core(sfen, 16, false, None).unwrap();
        assert!(res.is_mate());
        assert_eq!(res.steps(), 3);
        assert!(!res.redundant());
        assert!(res.kif().contains("手数----指手"));
        assert!(!res.moves_sfen().is_empty());
        assert_eq!(res.perfection_status(), "");
    }

    #[test]
    fn test_solve_futsuu_perfection_and_shortcut_kif_fork() {
        // 1. Complete work -> "perfect"
        let perfect_sfen = "7n1/7kP/8s/7+B1/9/9/9/9/9 b R6Prb4g3s3n4l11p 1";
        let ok_res = solve_futsuu_core(perfect_sfen, 32, true, None).unwrap();
        assert!(ok_res.is_mate());
        assert_eq!(ok_res.steps(), 37);
        assert_eq!(ok_res.perfection_status(), "perfect");

        // 2. Position with EarlyShortcut -> "early_shortcut" + branched KIF ("変化：")
        let shortcut_sfen = "4p3k/4l4/7R1/5R2S/9/9/9/9/9 b P2b4g3s4n3l16p 1";
        let sc_res = solve_futsuu_core(shortcut_sfen, 32, true, None).unwrap();
        assert!(sc_res.is_mate());
        assert_eq!(sc_res.perfection_status(), "early_shortcut");
        assert_eq!(sc_res.shortcut_ply(), 1);
        assert!(sc_res.shortcut_total_steps() > 0 && sc_res.shortcut_total_steps() < sc_res.steps());
        assert!(
            sc_res.kif().contains("変化：1手"),
            "Expected KIF fork in:\n{}",
            sc_res.kif()
        );
    }

    #[test]
    fn test_solve_futsuu_no_mate_drop_pawn_mate() {
        let sfen = "9/9/5lp2/5lk2/5l3/9/5N3/7L1/9 b P2r2b4g4s3n16p 1";
        let res = solve_futsuu_core(sfen, 16, false, None).unwrap();
        assert!(res.is_no_mate());
    }

    #[test]
    fn test_solve_futsuu_rejects_checked_white_and_stones() {
        assert_eq!(
            solve_futsuu_core("4k4/4G4/9/9/9/9/9/9/9 b - 1", 16, false, None)
                .err()
                .unwrap(),
            "受方玉にすでに王手がかかっています。"
        );
        assert_eq!(
            solve_futsuu_core("4k4/9/4O4/9/9/9/9/9/9 b G 1", 16, false, None)
                .err()
                .unwrap(),
            "普通詰将棋では石（壁駒）は使用できません。"
        );
        assert_eq!(
            solve_futsuu_core("4k4/9/4P4/9/9/9/9/9/9 w G 1", 16, false, None)
                .err()
                .unwrap(),
            "普通詰将棋は先手（攻方）番の局面のみ対応しています。"
        );
    }

    #[test]
    fn test_solve_futsuu_perfection_futile_and_dirty_escape_regressions() {
        // King-adjacent 打無駄合 (`P*5b` collapsed -> 1-ply "perfect", not 3-ply "redundant")
        let res_futile =
            solve_futsuu_core("3lkl3/3p1p3/3G4R/9/9/9/9/9/9 b p 1", 16, true, None).unwrap();
        assert!(res_futile.is_mate());
        assert_eq!(res_futile.steps(), 1);
        assert!(!res_futile.redundant());
        assert_eq!(res_futile.perfection_status(), "perfect");

        // Longer dirty King escape (`2. 5c6b` -> 5-ply "redundant", not 3-ply "perfect")
        let res_dirty = solve_futsuu_core(
            "1n1n4R/1pp1p1n1p/1Sn1k1p2/9/5G3/9/9/9/9 b GS 1",
            16,
            true,
            None,
        )
        .unwrap();
        assert!(res_dirty.is_mate());
        assert_eq!(res_dirty.steps(), 5);
        assert!(res_dirty.redundant());
        assert_eq!(res_dirty.perfection_status(), "redundant");

        // EqualLengthShortcut with effective Tsume-Shogi shortcut_total_steps and KIF `変化：` fork
        let res_eq =
            solve_futsuu_core("9/6k2/4n4/3L5/4+P2K1/9/9/9/9 b 2BS 1", 32, true, None).unwrap();
        assert!(res_eq.is_mate());
        assert_eq!(res_eq.steps(), 13);
        assert_eq!(res_eq.perfection_status(), "equal_shortcut");
        assert_eq!(res_eq.shortcut_total_steps(), 13);
        assert!(res_eq.kif().contains("変化："), "{}", res_eq.kif());
    }
}
