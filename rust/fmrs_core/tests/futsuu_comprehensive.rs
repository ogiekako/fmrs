//! Comprehensive regression test suite for normal Tsume-Shogi (`futsuu`):
//! validation, move generation parity, `双玉` pins & counter-checks, `打ち歩詰め` vs `突き歩詰め`,
//! futile interpositions (`無駄合` vs `中合`/`捨て合`), post-solve PV refinement, `NiCache`,
//! `ConventionalVerifier` perfection rules, KIF/JKF conversion, MIT/Apache-2.0 OSS edge cases,
//! and public-domain Edo-period classical compositions.
//!
//! All external test positions in this file originate exclusively from permissive MIT / Apache-2.0
//! repositories or public-domain Edo-period works (1734–1755), with explicit per-case attribution.

use fmrs_core::converter;
use fmrs_core::futsuu::perfection::{
    BoundedDfPn, ConventionalVerifier, FlawKind, PerfectionConfig, PerfectionStatus,
};
use fmrs_core::futsuu::solver::{analyze_tsume_pv, effective_tsume_pv_plies};
use fmrs_core::futsuu::tbase::TBase;
use fmrs_core::futsuu::{
    pv_to_movements, FmrsPos, Move, PositionBackend, SearchConfig, ShtsumePos, Solver,
};
use fmrs_core::sfen;
use fmrs_core::validate::decode_and_validate_futsuu_position;

fn verify_physical_mate_pv(root: &FmrsPos, pv: &[Move]) {
    assert!(
        !pv.is_empty() && pv.len() % 2 == 1,
        "PV must have positive odd length, got len={}",
        pv.len()
    );
    let mut tb = TBase::new(1);
    let mut cur = root.clone();
    for (ply, &mv) in pv.iter().enumerate() {
        tb.clear();
        let legal: Vec<Move> = if ply % 2 == 0 {
            cur.generate_check(&mut tb)
                .into_iter()
                .flat_map(|it| it.all_moves())
                .collect()
        } else {
            cur.generate_evasion_simple(&mut tb)
                .into_iter()
                .flat_map(|it| it.all_moves())
                .collect()
        };
        assert!(
            legal.contains(&mv),
            "Illegal move {} at ply {}",
            mv.to_sfen(),
            ply + 1
        );
        cur = cur.move_forward(mv);
    }
    tb.clear();
    assert!(
        cur.generate_evasion(&mut tb).0.is_empty(),
        "Final position after PV still has Tsume-Shogi evasions"
    );
}

#[test]
fn test_futsuu_validation_and_jkf_kif_conventions() {
    // 1. Reject White-to-move ("w") SFENs
    let err_w = decode_and_validate_futsuu_position("4k4/9/4P4/9/9/9/9/9/9 w G 1").unwrap_err();
    assert!(err_w.contains("先手（攻方）番"), "unexpected error: {err_w}");

    // 2. Reject initial position where White's King is already in check
    let err_chk =
        decode_and_validate_futsuu_position("7lk/7p1/7s1/6b2/9/9/3K5/9/8R b - 1").unwrap_err();
    assert!(
        err_chk.contains("王手がかかっています"),
        "unexpected error: {err_chk}"
    );

    // 3. Reject stones (`O`)
    let err_stone = decode_and_validate_futsuu_position("4k4/9/4O4/9/9/9/9/9/9 b G 1").unwrap_err();
    assert!(err_stone.contains("石"), "unexpected error: {err_stone}");

    // 4. Reject immobile pieces (`行きどころのない駒`: White knight on rank i = 9段目)
    // Attribution: hkijin/shtsume (MIT), Xcode scheme debug SFEN #5 (`4n4` on rank 9)
    let err_imm = decode_and_validate_futsuu_position(
        "4k4/9/PPPPPPPPP/9/9/9/9/9/4n4 b B4L2rb4g4s4n8p 1",
    )
    .unwrap_err();
    assert!(
        err_imm.contains("行きどころのない駒"),
        "unexpected error: {err_imm}"
    );

    // 5. JKF / KIF explicit unpromotion (`不成` / `promote: Some(false)`) in promotion zone
    let sfen_unprom = "3l1l3/3pkp3/3n1n3/4KS3/9/9/9/9/9 b G 1";
    let mut pos = sfen::decode_position(sfen_unprom).unwrap();
    let root = FmrsPos::from_sfen(sfen_unprom).unwrap();
    let mut tb = TBase::new(1);
    let unprom_mv = root
        .generate_check(&mut tb)
        .into_iter()
        .flat_map(|it| it.all_moves())
        .find(|m| m.to_sfen() == "4d5c")
        .expect("4d5c unpromoted check must be legal");
    let mvs = pv_to_movements(&[unprom_mv]);
    let jkf = converter::convert(&mut pos.clone(), std::slice::from_ref(&mvs));
    let first_mf = jkf.moves[1].move_.as_ref().expect("move format");
    assert_eq!(
        first_mf.promote,
        Some(false),
        "Unpromoted move entering promotion zone must set promote: Some(false)"
    );
    let kif = converter::convert_to_kif(&mut pos, std::slice::from_ref(&mvs));
    assert!(
        kif.contains("５三銀(44)"),
        "Expected unpromoted ５三銀(44) in KIF:\n{kif}"
    );
    assert!(
        !kif.contains("銀成"),
        "Unpromoted move must not render as 銀成 in KIF:\n{kif}"
    );
}

#[test]
fn test_futsuu_shogi_legality_edge_cases() {
    let cases: &[(&str, &str, Option<bool>, u16, PerfectionStatus)] = &[
        // 双玉 pins & discovered checks
        (
            "sougyoku_pinned_rook_off_ray",
            "3lkl3/3npn3/9/9/K3R3r/9/9/9/9 b - 1",
            Some(false),
            0,
            PerfectionStatus::NoMate,
        ),
        (
            "sougyoku_pinned_bishop_along_ray",
            "6lkl/6pbp/6G2/5B3/4K4/9/9/9/9 b - 1",
            Some(true),
            1,
            PerfectionStatus::RedundantHand,
        ),
        (
            "sougyoku_pinned_by_lance",
            "6lkl/8p/6G2/9/6K2/9/9/9/9 b - 1",
            Some(false),
            0,
            PerfectionStatus::NoMate,
        ),
        (
            "sougyoku_king_discovered_mate",
            "3lkl3/3p1p3/9/9/9/9/9/4K4/4R4 b - 1",
            Some(true),
            1,
            PerfectionStatus::Perfect,
        ),
        (
            "sougyoku_discovered_gyaku_oute_refutes",
            "7lk/7p1/7s1/6b2/9/9/3K5/9/7R1 b - 1",
            Some(false),
            0,
            PerfectionStatus::NoMate,
        ),
        (
            "sougyoku_root_sente_in_check_with_futile_inter_move",
            "3lkl3/3p1p3/9/9/9/9/9/4K3r/4R4 b - 1",
            Some(true),
            1,
            PerfectionStatus::Perfect,
        ),
        (
            "bug2_ni_cache_sougyoku_gyaku_oute",
            "3l2lll/Rg5kp/2ss2g1g/5N1S1/9/9/9/9/3K5 b 2N 1",
            Some(false),
            0,
            PerfectionStatus::NoMate,
        ),
        // 打ち歩詰め vs 突き歩詰め
        (
            "uchifuzume_pinned_capturer",
            "8l/6gll/4R2sk/7l1/9/5K1N1/9/9/9 b P 1",
            Some(false),
            0,
            PerfectionStatus::NoMate,
        ),
        (
            "uchifuzume_blocked_cage",
            "6lll/6pkp/6n1l/8G/9/9/9/9/9 b P 1",
            Some(false),
            0,
            PerfectionStatus::NoMate,
        ),
        (
            "tsukifuzume_legal_board_pawn",
            "9/6lll/6pkp/6n1l/6KP1/9/9/9/9 b - 1",
            Some(true),
            1,
            PerfectionStatus::Perfect,
        ),
        // Promotion & 二歩
        (
            "prom_sliding_out_of_zone",
            "7R1/9/9/8p/8k/7P1/9/9/9 b - 1",
            Some(true),
            1,
            PerfectionStatus::Perfect,
        ),
        (
            "prom_mandatory_pawn_rank1",
            "5l1kl/5GPpp/9/9/9/9/9/9/9 b - 1",
            Some(true),
            1,
            PerfectionStatus::Perfect,
        ),
        (
            "nifu_tokin_and_enemy_pawn_allow_drop",
            "3l1l3/3pkp3/3n1n3/4K4/9/4p4/9/9/4+P4 b GP 1",
            Some(true),
            3,
            PerfectionStatus::Perfect,
        ),
        (
            "knight_rank3_unpromoted_mate",
            "6lkl/6nnn/8P/9/7N1/9/9/9/9 b - 1",
            Some(true),
            1,
            PerfectionStatus::Perfect,
        ),
        // Perpetual check (連続王手の千日手)
        (
            "perpetual_check_no_mate",
            "4k4/7+R1/9/9/9/9/9/9/9 b - 1",
            Some(false),
            0,
            PerfectionStatus::NoMate,
        ),
        // Empty-board drop check parity (BUG-4)
        (
            "bug4_empty_board_drop_check",
            "3l1l3/3p1p3/2nk1n3/9/9/9/9/9/9 b G 1",
            Some(false),
            0,
            PerfectionStatus::NoMate,
        ),
    ];

    for &(name, sfen_str, want_mate, want_steps, ref want_perf) in cases {
        let valid = decode_and_validate_futsuu_position(sfen_str)
            .unwrap_or_else(|e| panic!("{name}: unexpected validation error: {e}"));
        let fpos = FmrsPos::from_pos_aux_root(valid);
        let spos = ShtsumePos::from_sfen(sfen_str).unwrap();

        // Check MoveGen parity at root
        let mut tb1 = TBase::new(1);
        let mut tb2 = TBase::new(1);
        let mut f_cks: Vec<String> = fpos
            .generate_check(&mut tb1)
            .into_iter()
            .flat_map(|i| i.all_moves())
            .map(|m| m.to_sfen())
            .collect();
        let mut s_cks: Vec<String> = spos
            .generate_check(&mut tb2)
            .into_iter()
            .flat_map(|i| i.all_moves())
            .map(|m| m.to_sfen())
            .collect();
        f_cks.sort();
        s_cks.sort();
        assert_eq!(f_cks, s_cks, "{name}: check MoveGen mismatch");

        let mut solver_f = Solver::new(SearchConfig {
            hash_mb: 16,
            threads: 1,
            use_ni_cache: true,
            ..Default::default()
        });
        let res_f = solver_f.solve(&fpos);
        assert!(!res_f.error, "{name}: FmrsPos solver error");
        assert_eq!(res_f.mate, want_mate, "{name}: FmrsPos mate mismatch");

        let mut solver_s = Solver::new(SearchConfig {
            hash_mb: 16,
            threads: 1,
            ..Default::default()
        });
        let res_s = solver_s.solve(&spos);
        assert!(!res_s.error, "{name}: ShtsumePos solver error");
        assert_eq!(res_s.mate, want_mate, "{name}: ShtsumePos mate mismatch");

        if want_mate == Some(true) {
            assert_eq!(res_f.mate_steps, want_steps, "{name}: FmrsPos steps");
            assert_eq!(res_s.mate_steps, want_steps, "{name}: ShtsumePos steps");
            verify_physical_mate_pv(&fpos, &res_f.pv);
        }

        let mut verifier = ConventionalVerifier::new(PerfectionConfig {
            hash_mb: 16,
            solve_node_limit: 200_000,
            check_node_limit_per_move: 50_000,
            search_level: 1,
            fail_fast: true,
            endgame_transposition_rem_sh: 5,
        });
        let rep = verifier.verify_solved(&fpos, res_f);
        assert_eq!(&rep.status, want_perf, "{name}: perfection mismatch");
        if want_mate == Some(true) {
            assert_eq!(rep.mate_steps, want_steps, "{name}: verifier steps");
        }
    }

    // Attribution: nozaq/shogi-rs (MIT), `uchifuzume_ng_3_pinned_gold_cannot_capture`
    // Verify `P*1b` is excluded as illegal 打ち歩詰め when g(2b) is diagonally pinned by B(3c),
    // while the board move `2c1b+` legally mates in 1 ply (leaving P in hand -> RedundantHand).
    let pinned_uchifuzume = "7nk/7g1/6BS1/9/9/9/9/9/9 b P 1";
    let p_pos = FmrsPos::from_sfen(pinned_uchifuzume).unwrap();
    let mut tb = TBase::new(1);
    let p_checks: Vec<String> = p_pos
        .generate_check(&mut tb)
        .into_iter()
        .flat_map(|i| i.all_moves())
        .map(|m| m.to_sfen())
        .collect();
    assert!(
        !p_checks.iter().any(|m| m == "P*1b"),
        "P*1b must be illegal 打ち歩詰め when 2b gold is pinned: {p_checks:?}"
    );
}

#[test]
fn test_futsuu_futile_interposition_and_post_solve_refinement() {
    // 1. BoundedDfPn at count >= 30 must NOT treat capturing 1-ply mates as clean (`m1.cap_koma == 0`)
    let cap_pos = FmrsPos::from_sfen("6lkl/6pbp/8G/5B3/4K4/9/9/g8/9 b - 1")
        .unwrap()
        .with_count(32);
    let mut bnd = BoundedDfPn::new();
    let (clean_at_32, _) = bnd.prove_tsume_mate_within(&cap_pos, 1, None, true, 4_096);
    assert_eq!(
        clean_at_32, None,
        "1-ply capturing mate 4d2b+ leaves captured Bishop in hand and must not be clean"
    );

    // 2. BUG-1A: Longer dirty King escape `2. 5c6b` (5p RedundantHand, never 3p Perfect)
    let bug_1a = FmrsPos::from_sfen("1n1n4R/1pp1p1n1p/1Sn1k1p2/9/5G3/9/9/9/9 b GS 1").unwrap();
    let mut solver = Solver::new(SearchConfig {
        hash_mb: 16,
        threads: 1,
        ..Default::default()
    });
    let r_1a = solver.solve(&bug_1a);
    assert_eq!((r_1a.mate, r_1a.mate_steps, r_1a.redundant), (Some(true), 5, true));
    let mut verifier = ConventionalVerifier::new(PerfectionConfig::default());
    let rep_1a = verifier.verify_solved(&bug_1a, r_1a);
    assert_eq!(rep_1a.status, PerfectionStatus::RedundantHand);
    assert_eq!(rep_1a.mate_steps, 5);

    // 3. BUG-1B: King-adjacent 合い利かず `N*2b` collapsed -> 5p EqualLengthShortcut with non-empty shortcut_pv
    let bug_1b = FmrsPos::from_sfen("3+B2k2/9/3+p2B2/3G5/9/9/9/9/9 b RSn 1").unwrap();
    let r_1b = solver.solve(&bug_1b);
    let rep_1b = verifier.verify_solved(&bug_1b, r_1b);
    assert!(
        matches!(
            rep_1b.status,
            PerfectionStatus::Shortcut {
                kind: FlawKind::EqualLengthShortcut,
                ..
            }
        ),
        "Expected EqualLengthShortcut on BUG-1B, got {:?}",
        rep_1b.status
    );
    assert_eq!(rep_1b.mate_steps, 5);
    assert!(!rep_1b.shortcut_pv.is_empty());
    verify_physical_mate_pv(&bug_1b, &rep_1b.shortcut_pv);

    // 4. BUG-3A (King-adjacent 打無駄合) & BUG-3B (pinned-defender 打無駄合) -> 1p Perfect
    for sfen_str in [
        "3lkl3/3p1p3/3G4R/9/9/9/9/9/9 b p 1",
        "3lkl3/3p1s3/3G5/1R5B1/9/9/9/9/9 b p 1",
    ] {
        let root = FmrsPos::from_sfen(sfen_str).unwrap();
        let r = solver.solve(&root);
        assert_eq!((r.mate, r.mate_steps, r.redundant), (Some(true), 1, false));
        let rep = verifier.verify_solved(&root, r);
        assert_eq!(rep.status, PerfectionStatus::Perfect);
        assert_eq!(rep.mate_steps, 1);
    }

    // 5. BUG-5: 13p EqualLengthShortcut with non-empty shortcut_pv and KIF `変化：` fork
    let bug_5_sfen = "9/6k2/4n4/3L5/4+P2K1/9/9/9/9 b 2BS 1";
    let bug_5 = FmrsPos::from_sfen(bug_5_sfen).unwrap();
    let r_5 = solver.solve(&bug_5);
    let rep_5 = verifier.verify_solved(&bug_5, r_5);
    assert!(matches!(
        rep_5.status,
        PerfectionStatus::Shortcut {
            kind: FlawKind::EqualLengthShortcut,
            ..
        }
    ));
    assert_eq!(rep_5.mate_steps, 13);
    assert!(!rep_5.shortcut_pv.is_empty());
    verify_physical_mate_pv(&bug_5, &rep_5.shortcut_pv);
    assert_eq!(effective_tsume_pv_plies(&bug_5, &rep_5.shortcut_pv), 13);
    let mut p5 = sfen::decode_position(bug_5_sfen).unwrap();
    let sols = vec![
        pv_to_movements(&rep_5.pv),
        pv_to_movements(&rep_5.shortcut_pv),
    ];
    let kif_5 = converter::convert_to_kif(&mut p5, &sols);
    assert!(kif_5.contains("変化："), "Missing KIF fork:\n{kif_5}");

    // 6. Clean short mate recovery on both FmrsPos and ShtsumePos + 移動中合 5p preservation
    for (sfen_str, want_steps) in [
        ("3l1l3/3pkp3/3n1n3/4S4/4R4/9/9/9/9 b Gp 1", 3u16),
        ("9/6k2/3S1g3/K8/6+N2/9/9/9/9 b R2G 1", 5u16),
        ("4k4/9/9/S3P3S/2rL1Lr2/9/9/9/9 b B 1", 5u16),
    ] {
        let fpos = FmrsPos::from_sfen(sfen_str).unwrap();
        let spos = ShtsumePos::from_sfen(sfen_str).unwrap();
        let rf = solver.solve(&fpos);
        let rs = solver.solve(&spos);
        assert_eq!((rf.mate, rf.mate_steps, rf.redundant), (Some(true), want_steps, false));
        assert_eq!((rs.mate, rs.mate_steps, rs.redundant), (Some(true), want_steps, false));
    }
}

#[test]
fn test_futsuu_perfection_conventions_and_oss_classics() {
    let mut solver = Solver::new(SearchConfig {
        hash_mb: 32,
        threads: 1,
        ..Default::default()
    });
    let mut verifier = ConventionalVerifier::new(PerfectionConfig {
        hash_mb: 32,
        solve_node_limit: 500_000,
        check_node_limit_per_move: 100_000,
        search_level: 1,
        fail_fast: true,
        endgame_transposition_rem_sh: 5,
    });

    // 1. Perfection conventions: symmetric dual, prom/unprom minor flaw, final-move minor flaw,
    //    5-ply convergent transposition (allowed) vs 7-ply transposition (EqualLengthShortcut)
    {
        // Symmetric root dual mate
        let sym = FmrsPos::from_sfen("3l1l3/3pkp3/3n1n3/4P4/9/9/9/9/9 b GN 1").unwrap();
        let rep = verifier.verify_solved(&sym, solver.solve(&sym));
        assert!(matches!(
            rep.status,
            PerfectionStatus::Shortcut {
                kind: FlawKind::EqualLengthShortcut,
                ..
            }
        ));

        // Same src/dst promote vs unpromote (`成・不成非限定` -> Perfect, minor_flaw_count == 1)
        let prom_minor = FmrsPos::from_sfen("3l1l3/3pkp3/3n1n3/4KS3/9/9/9/9/9 b G 1").unwrap();
        let rep = verifier.verify_solved(&prom_minor, solver.solve(&prom_minor));
        assert_eq!(rep.status, PerfectionStatus::Perfect);
        assert_eq!(rep.minor_flaw_count, 1);

        // Final-ply indeterminacy (`最終手非限定` -> Perfect, minor_flaw_count == 1)
        let fin_minor = FmrsPos::from_sfen("6lkl/6nnn/9/9/7N1/9/9/9/9 b - 1").unwrap();
        let rep = verifier.verify_solved(&fin_minor, solver.solve(&fin_minor));
        assert_eq!(rep.status, PerfectionStatus::Perfect);
        assert_eq!(rep.minor_flaw_count, 1);

        // 5-ply convergent endgame transposition (`rem_sh == 5` -> Perfect, minor_flaw_count == 1)
        let tr5 = FmrsPos::from_sfen("3lll3/3pkp3/3g1g3/4S4/9/9/9/9/9 b G2N 1").unwrap();
        let rep = verifier.verify_solved(&tr5, solver.solve(&tr5));
        assert_eq!(rep.status, PerfectionStatus::Perfect);
        assert_eq!(rep.mate_steps, 5);
        assert_eq!(rep.minor_flaw_count, 1);

        // 7-ply transposition (`rem_sh == 7 > 5` -> EqualLengthShortcut)
        let tr7 = FmrsPos::from_sfen("3l1l3/3pkp3/3g1g3/4S4/9/9/9/9/9 b 2G2N 1").unwrap();
        let rep = verifier.verify_solved(&tr7, solver.solve(&tr7));
        assert!(matches!(
            rep.status,
            PerfectionStatus::Shortcut {
                kind: FlawKind::EqualLengthShortcut,
                ..
            }
        ));
    }

    // 2. Attribution: tokuhirom/tanuki-tsume-shogi (MIT)
    // - `tanuki_m7_id22_02470367`: 7-ply unique mate with two Rooks
    // - `test_is_mate_in_3` (`9/8k/9/9/9/9/9/9/9 b 2G 1`): upstream false-mate test refuted (`NoMate`)
    // - `tanuki_m7_id27_22eb8c99`: upstream 7-ply puzzle refuted by 5-ply `EarlyShortcut` (`5. 1d2c`)
    {
        let tanuki_ok =
            FmrsPos::from_sfen("7k1/9/8R/8R/9/9/9/9/9 b 2b4g4s4n4l18p 1").unwrap();
        let rep_ok = verifier.verify_solved(&tanuki_ok, solver.solve(&tanuki_ok));
        assert_eq!(rep_ok.status, PerfectionStatus::Perfect);
        assert_eq!(rep_ok.mate_steps, 7);

        let tanuki_nomate = FmrsPos::from_sfen("9/8k/9/9/9/9/9/9/9 b 2G 1").unwrap();
        assert_eq!(solver.solve(&tanuki_nomate).mate, Some(false));

        let tanuki_early =
            FmrsPos::from_sfen("9/7pk/9/7sG/9/8N/8R/9/9 b r2b3g3s2n4l17p 1").unwrap();
        let rep_early = verifier.verify_solved(&tanuki_early, solver.solve(&tanuki_early));
        assert_eq!(rep_early.status, PerfectionStatus::Perfect);
        assert_eq!(rep_early.mate_steps, 5);
    }

    // 3. Attribution: kent-tokyo/sekirei (MIT OR Apache-2.0), `futile_interposition_nine_ply`
    // 7 plies under Tsume-Shogi futile-drop skipping vs 9 plies under raw `simple_mate`
    {
        let sekirei =
            FmrsPos::from_sfen("9/6p1k/7P1/6nB1/5R3/7L1/9/9/K8 b rb4g4s3n3l16p 1").unwrap();
        let r_std = solver.solve(&sekirei);
        assert_eq!((r_std.mate, r_std.mate_steps), (Some(true), 7));
        let mut simple_solver = Solver::new(SearchConfig {
            hash_mb: 16,
            simple_mate: true,
            ..Default::default()
        });
        let r_sim = simple_solver.solve(&sekirei);
        assert_eq!((r_sim.mate, r_sim.mate_steps), (Some(true), 9));
    }

    // 4. Public-domain Edo classics:
    // - 伊藤看寿『将棋図巧』(1755): 第5番 (21p Perfect, endgame 同手数変化駒余り allowed),
    //   第100番『寿』(611p preserving Dragon-lure 遠隔合/中合)
    // - 三代伊藤宗看『将棋無双』(1734): 第65番刊本原図・第79番 (historically documented computer-discovered 余詰)
    {
        let zuko_05 =
            FmrsPos::from_sfen("n+B1sS4/1R1g5/1Ls6/2k6/2n6/3L5/R8/9/9 b B2P3gs2n2l16p 1")
                .unwrap();
        let rep_z5 = verifier.verify_solved(&zuko_05, solver.solve(&zuko_05));
        assert_eq!(rep_z5.status, PerfectionStatus::Perfect);
        assert_eq!(rep_z5.mate_steps, 21);

        let mut solver_mt = Solver::new(SearchConfig {
            hash_mb: 64,
            threads: 4,
            ..Default::default()
        });
        let kotobuki = FmrsPos::from_sfen(
            "g3p3+r/3lPG3/1s4ppp/1l1B4P/L2b1kPl1/3+p3+R+p/3+p2+s1S/+NP+p1gS3/1g+p2P3 b 4P3n 1",
        )
        .unwrap();
        let r_koto = solver_mt.solve(&kotobuki);
        assert_eq!((r_koto.mate, r_koto.mate_steps, r_koto.redundant), (Some(true), 611, false));
        let (_, rem_eff, eff_unused) = analyze_tsume_pv(&kotobuki, &r_koto.pv);
        assert_eq!((rem_eff[0], eff_unused), (611, 0));

        for musou_sfen in [
            "9/9/5g3/5p1+p1/2+p3+p2/3pP3N/1nS1kB3/2+R3Ss1/1N1Ps3+R b BG2gn4l11p 1",
            "9/7B1/9/9/n1p6/+p8/4n4/+B1k1S4/N8 b G2S2r3gsn4l16p 1",
        ] {
            let musou = FmrsPos::from_sfen(musou_sfen).unwrap();
            let rep_m = verifier.verify_solved(&musou, solver.solve(&musou));
            assert!(matches!(rep_m.status, PerfectionStatus::Shortcut { .. }));
            assert!(!rep_m.shortcut_pv.is_empty());
            verify_physical_mate_pv(&musou, &rep_m.shortcut_pv);
        }
    }
}
