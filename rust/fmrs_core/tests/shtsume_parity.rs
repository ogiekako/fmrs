use fmrs_core::shtsume::tbase::TBase;
use fmrs_core::shtsume::{FmrsPos, Move, PositionBackend, SearchConfig, ShtsumePos, Solver};

fn verify_pv<P: PositionBackend>(root: &P, pv: &[Move]) {
    assert!(!pv.is_empty(), "PV should not be empty for mate");
    assert_eq!(pv.len() % 2, 1, "mate PV length must be odd");
    let mut tb = TBase::new(1);
    let mut pos = root.clone();
    for (ply, &mv) in pv.iter().enumerate() {
        let legal: Vec<Move> = if ply % 2 == 0 {
            pos.generate_check(&mut tb)
                .into_iter()
                .flat_map(|item| item.all_moves())
                .collect()
        } else {
            pos.generate_evasion_simple(&mut tb)
                .into_iter()
                .flat_map(|item| item.all_moves())
                .collect()
        };
        assert!(
            legal.contains(&mv),
            "illegal PV move {} at ply {}",
            mv.to_sfen(),
            ply
        );
        pos = pos.move_forward(mv);
    }
    let evasions = pos.generate_evasion_simple(&mut tb);
    assert!(
        evasions.is_empty(),
        "PV did not end in checkmate (remaining evasions: {})",
        evasions.len()
    );
}

fn sorted_moves(items: &[fmrs_core::shtsume::types::MvItem]) -> Vec<String> {
    let mut out = Vec::new();
    for item in items {
        for mv in item.all_moves() {
            out.push(mv.to_sfen());
        }
    }
    out.sort();
    out
}

#[test]
fn test_check_generation_parity() {
    let positions = [
        "4R4/9/4P4/9/4k1P1R/9/2N1s4/1B7/4L4 b b4g3s3n3l16p 1",
        "4k4/9/PPPPPPPPP/9/9/9/9/9/9 b B4L2rb4g4s4n9p 1",
        "9/9/9/9/1+rn6/9/kP7/3s5/1K2G4 b Lr2b3g3s3n3l17p 1",
        "n+B1sS4/1R1g5/1Ls6/2k6/2n6/3L5/R8/9/9 b B2P3gs2n2l16p 1",
        "k1+P4n1/2L+P2sL1/r4+P+P1P/+BpP+Pl1+Rg1/NP1S+PP+p1g/2L+p1g+P1+P/Ps1G1+P1N1/1sN1P4/B8 b - 1",
        "g3p3+r/3lPG3/1s4ppp/1l1B4P/L2b1kPl1/3+p3+R+p/3+p2+s1S/+NP+p1gS3/1g+p2P3 b 4P3n 1",
    ];

    let mut tb1 = TBase::new(1);
    let mut tb2 = TBase::new(1);

    for sfen in positions {
        let p_fmrs = FmrsPos::from_sfen(sfen).unwrap();
        let p_sh = ShtsumePos::from_sfen(sfen).unwrap();

        let m_fmrs = sorted_moves(&p_fmrs.generate_check(&mut tb1));
        let m_sh = sorted_moves(&p_sh.generate_check(&mut tb2));
        assert_eq!(m_fmrs, m_sh, "check generation mismatch for {}", sfen);
    }
}

#[test]
fn test_solver_parity_small_and_medium() {
    let cases: &[(&str, &str, Option<bool>, Option<u16>)] = &[
        (
            "1ply_gold_drop",
            "7k1/9/7P1/9/9/9/9/9/9 b G2r2b3g4s4n4l17p 1",
            Some(true),
            Some(1),
        ),
        (
            "drop_pawn_mate_illegal",
            "9/9/5lp2/5lk2/5l3/9/5N3/7L1/9 b P2r2b4g4s3n16p 1",
            Some(false),
            None,
        ),
        (
            "forest-12-03_9_nomate",
            "9/9/9/9/1+rn6/9/kP7/3s5/1K2G4 b Lr2b3g3s3n3l17p 1",
            Some(false),
            None,
        ),
        (
            "sample_classic_3",
            "3sks3/9/4S4/9/9/B8/9/9/9 b S2rb4g4n4l18p 1",
            Some(true),
            Some(3),
        ),
        (
            "yane_2_5",
            "7nl/7k1/6p2/6S1p/9/9/9/9/9 b GS2r2b3g2s3n3l16p 1",
            Some(true),
            Some(5),
        ),
        (
            "lances_11",
            "4k4/9/PPPPPPPPP/9/9/9/9/9/9 b B4L2rb4g4s4n9p 1",
            Some(true),
            Some(11),
        ),
        (
            "zuko_05_21",
            "n+B1sS4/1R1g5/1Ls6/2k6/2n6/3L5/R8/9/9 b B2P3gs2n2l16p 1",
            Some(true),
            Some(21),
        ),
    ];

    let cfg = SearchConfig {
        hash_mb: 32,
        time_limit_secs: Some(15.0),
        ..Default::default()
    };

    for &(name, sfen, want_mate, want_steps) in cases {
        let p_fmrs = FmrsPos::from_sfen(sfen).unwrap();
        let p_sh = ShtsumePos::from_sfen(sfen).unwrap();

        let mut solver_fmrs = Solver::new(cfg.clone());
        let res_fmrs = solver_fmrs.solve(&p_fmrs);

        let mut solver_sh = Solver::new(cfg.clone());
        let res_sh = solver_sh.solve(&p_sh);

        assert_eq!(
            res_fmrs.mate, want_mate,
            "[{}] fmrs mate mismatch: {:?}",
            name, res_fmrs
        );
        assert_eq!(
            res_sh.mate, want_mate,
            "[{}] shtsume mate mismatch: {:?}",
            name, res_sh
        );
        if let Some(steps) = want_steps {
            assert_eq!(
                res_fmrs.mate_steps, steps,
                "[{}] fmrs steps mismatch: {:?}",
                name, res_fmrs
            );
            assert_eq!(
                res_sh.mate_steps, steps,
                "[{}] shtsume steps mismatch: {:?}",
                name, res_sh
            );
            verify_pv(&p_fmrs, &res_fmrs.pv);
            verify_pv(&p_sh, &res_sh.pv);
        }
    }
}

#[test]
fn test_simple_mate_mode() {
    let cases: &[(&str, &str, Option<bool>)] = &[
        (
            "1ply_gold_drop",
            "7k1/9/7P1/9/9/9/9/9/9 b G2r2b3g4s4n4l17p 1",
            Some(true),
        ),
        (
            "drop_pawn_mate_illegal",
            "9/9/5lp2/5lk2/5l3/9/5N3/7L1/9 b P2r2b4g4s3n16p 1",
            Some(false),
        ),
        (
            "forest-12-03_9_nomate",
            "9/9/9/9/1+rn6/9/kP7/3s5/1K2G4 b Lr2b3g3s3n3l17p 1",
            Some(false),
        ),
        (
            "sample_classic_3",
            "3sks3/9/4S4/9/9/B8/9/9/9 b S2rb4g4n4l18p 1",
            Some(true),
        ),
        (
            "lances_11",
            "4k4/9/PPPPPPPPP/9/9/9/9/9/9 b B4L2rb4g4s4n9p 1",
            Some(true),
        ),
        (
            "zuko_05_21",
            "n+B1sS4/1R1g5/1Ls6/2k6/2n6/3L5/R8/9/9 b B2P3gs2n2l16p 1",
            Some(true),
        ),
        (
            "yane_5_fg",
            "6lnk/6+Rbl/2n4pp/7s1/1p2P2NP/p1P2PPP1/1P4GS1/6GK1/LNr5L b B2G2S6Pp 1",
            Some(true),
        ),
    ];

    let cfg = SearchConfig {
        hash_mb: 32,
        time_limit_secs: Some(15.0),
        simple_mate: true,
        ..Default::default()
    };

    for &(name, sfen, want_mate) in cases {
        let p_fmrs = FmrsPos::from_sfen(sfen).unwrap();
        let p_sh = ShtsumePos::from_sfen(sfen).unwrap();

        let mut solver_fmrs = Solver::new(cfg.clone());
        let res_fmrs = solver_fmrs.solve(&p_fmrs);

        let mut solver_sh = Solver::new(cfg.clone());
        let res_sh = solver_sh.solve(&p_sh);

        assert!(!res_fmrs.error, "[{}] fmrs error", name);
        assert!(!res_sh.error, "[{}] shtsume error", name);
        assert_eq!(res_fmrs.mate, want_mate, "[{}] fmrs mate mismatch", name);
        assert_eq!(res_sh.mate, want_mate, "[{}] shtsume mate mismatch", name);

        if want_mate == Some(true) {
            assert_eq!(
                res_fmrs.pv.len(),
                res_fmrs.mate_steps as usize,
                "[{}] fmrs pv len != mate_steps",
                name
            );
            assert_eq!(
                res_sh.pv.len(),
                res_sh.mate_steps as usize,
                "[{}] shtsume pv len != mate_steps",
                name
            );
            verify_pv(&p_fmrs, &res_fmrs.pv);
            verify_pv(&p_sh, &res_sh.pv);
        }
    }
}

#[test]
fn test_parallel_solver_parity_and_soundness() {
    let cases: &[(&str, &str, Option<bool>, Option<u16>)] = &[
        (
            "1ply_gold_drop",
            "7k1/9/7P1/9/9/9/9/9/9 b G2r2b3g4s4n4l17p 1",
            Some(true),
            Some(1),
        ),
        (
            "drop_pawn_mate_illegal",
            "9/9/5lp2/5lk2/5l3/9/5N3/7L1/9 b P2r2b4g4s3n16p 1",
            Some(false),
            None,
        ),
        (
            "forest-12-03_9_nomate",
            "9/9/9/9/1+rn6/9/kP7/3s5/1K2G4 b Lr2b3g3s3n3l17p 1",
            Some(false),
            None,
        ),
        (
            "sample_classic_3",
            "3sks3/9/4S4/9/9/B8/9/9/9 b S2rb4g4n4l18p 1",
            Some(true),
            Some(3),
        ),
        (
            "lances_11",
            "4k4/9/PPPPPPPPP/9/9/9/9/9/9 b B4L2rb4g4s4n9p 1",
            Some(true),
            Some(11),
        ),
        (
            "zuko_05_21",
            "n+B1sS4/1R1g5/1Ls6/2k6/2n6/3L5/R8/9/9 b B2P3gs2n2l16p 1",
            Some(true),
            Some(21),
        ),
        (
            "yane_5_fg",
            "6lnk/6+Rbl/2n4pp/7s1/1p2P2NP/p1P2PPP1/1P4GS1/6GK1/LNr5L b B2G2S6Pp 1",
            Some(true),
            None,
        ),
        (
            "yane_13_fg",
            "lng3+R2/2kgs4/ppp6/1B1pp4/7B1/2P2pLp1/PP1PP3P/1S1K2p2/LN5GL b RG2SP2n3p 1",
            Some(true),
            None,
        ),
    ];

    for threads in [4usize, 8usize] {
        let mut solver_fmrs = Solver::new(SearchConfig {
            hash_mb: 64,
            time_limit_secs: Some(15.0),
            threads,
            ..Default::default()
        });
        let mut solver_sh = Solver::new(SearchConfig {
            hash_mb: 64,
            time_limit_secs: Some(15.0),
            threads,
            ..Default::default()
        });

        for &(name, sfen, want_mate, want_steps) in cases {
            let p_fmrs = FmrsPos::from_sfen(sfen).unwrap();
            let p_sh = ShtsumePos::from_sfen(sfen).unwrap();

            let res_fmrs = solver_fmrs.solve(&p_fmrs);
            let res_sh = solver_sh.solve(&p_sh);

            assert!(!res_fmrs.error, "[{} t={}] fmrs error", name, threads);
            assert!(!res_sh.error, "[{} t={}] shtsume error", name, threads);
            assert_eq!(
                res_fmrs.mate, want_mate,
                "[{} t={}] fmrs mate mismatch",
                name, threads
            );
            assert_eq!(
                res_sh.mate, want_mate,
                "[{} t={}] shtsume mate mismatch",
                name, threads
            );
            if let Some(steps) = want_steps {
                assert_eq!(
                    res_fmrs.mate_steps, steps,
                    "[{} t={}] fmrs steps mismatch",
                    name, threads
                );
                assert_eq!(
                    res_sh.mate_steps, steps,
                    "[{} t={}] shtsume steps mismatch",
                    name, threads
                );
            }
            if want_mate == Some(true) {
                verify_pv(&p_fmrs, &res_fmrs.pv);
                verify_pv(&p_sh, &res_sh.pv);
            }
        }
    }

    // Verify `use_1ply_mate: true` in parallel mode also emits a complete odd-length PV
    let p_fg5 =
        FmrsPos::from_sfen("6lnk/6+Rbl/2n4pp/7s1/1p2P2NP/p1P2PPP1/1P4GS1/6GK1/LNr5L b B2G2S6Pp 1")
            .unwrap();
    let mut s_1ply = Solver::new(SearchConfig {
        hash_mb: 64,
        use_1ply_mate: true,
        threads: 4,
        ..Default::default()
    });
    let r_1ply = s_1ply.solve(&p_fg5);
    assert_eq!(r_1ply.mate, Some(true));
    verify_pv(&p_fg5, &r_1ply.pv);
}

#[test]
fn test_parallel_simple_mate_mode() {
    let cases: &[(&str, &str, Option<bool>)] = &[
        (
            "drop_pawn_mate_illegal",
            "9/9/5lp2/5lk2/5l3/9/5N3/7L1/9 b P2r2b4g4s3n16p 1",
            Some(false),
        ),
        (
            "forest-12-03_9_nomate",
            "9/9/9/9/1+rn6/9/kP7/3s5/1K2G4 b Lr2b3g3s3n3l17p 1",
            Some(false),
        ),
        (
            "zuko_05_21",
            "n+B1sS4/1R1g5/1Ls6/2k6/2n6/3L5/R8/9/9 b B2P3gs2n2l16p 1",
            Some(true),
        ),
        (
            "yane_5_fg",
            "6lnk/6+Rbl/2n4pp/7s1/1p2P2NP/p1P2PPP1/1P4GS1/6GK1/LNr5L b B2G2S6Pp 1",
            Some(true),
        ),
    ];

    let mut solver_fmrs = Solver::new(SearchConfig {
        hash_mb: 64,
        time_limit_secs: Some(15.0),
        simple_mate: true,
        threads: 4,
        ..Default::default()
    });
    let mut solver_sh = Solver::new(SearchConfig {
        hash_mb: 64,
        time_limit_secs: Some(15.0),
        simple_mate: true,
        threads: 4,
        ..Default::default()
    });

    for &(name, sfen, want_mate) in cases {
        let p_fmrs = FmrsPos::from_sfen(sfen).unwrap();
        let p_sh = ShtsumePos::from_sfen(sfen).unwrap();

        let res_fmrs = solver_fmrs.solve(&p_fmrs);
        let res_sh = solver_sh.solve(&p_sh);

        assert!(!res_fmrs.error, "[{}] fmrs error", name);
        assert!(!res_sh.error, "[{}] shtsume error", name);
        assert_eq!(res_fmrs.mate, want_mate, "[{}] fmrs mate mismatch", name);
        assert_eq!(res_sh.mate, want_mate, "[{}] shtsume mate mismatch", name);
        if want_mate == Some(true) {
            assert_eq!(res_fmrs.pv.len(), res_fmrs.mate_steps as usize);
            assert_eq!(res_sh.pv.len(), res_sh.mate_steps as usize);
            verify_pv(&p_fmrs, &res_fmrs.pv);
            verify_pv(&p_sh, &res_sh.pv);
        }
    }
}

#[test]
fn test_simple_mate_sound_problems_never_shorter_than_std() {
    // Under `simple_mate` (`攻方最短・駒余り可`, `受方最長・無駄合可`), any sound (`完全作`)
    // tsume-shogi problem of standard length `L_std` must satisfy `mate_steps >= L_std`.
    let sound_cases: &[(&str, &str, u16)] = &[
        (
            "sample_classic_3",
            "3sks3/9/4S4/9/9/B8/9/9/9 b S2rb4g4n4l18p 1",
            3,
        ),
        (
            "lances_11",
            "4k4/9/PPPPPPPPP/9/9/9/9/9/9 b B4L2rb4g4s4n9p 1",
            11,
        ),
        (
            "zuko_05_21",
            "n+B1sS4/1R1g5/1Ls6/2k6/2n6/3L5/R8/9/9 b B2P3gs2n2l16p 1",
            21,
        ),
        (
            "zuko_99_kemuri_117",
            "k1+P4n1/2L+P2sL1/r4+P+P1P/+BpP+Pl1+Rg1/NP1S+PP+p1g/2L+p1g+P1+P/Ps1G1+P1N1/1sN1P4/B8 b - 1",
            117,
        ),
    ];

    for threads in [1usize, 4usize] {
        let mut solver_fmrs = Solver::new(SearchConfig {
            hash_mb: 128,
            time_limit_secs: Some(20.0),
            simple_mate: true,
            threads,
            ..Default::default()
        });
        let mut solver_sh = Solver::new(SearchConfig {
            hash_mb: 128,
            time_limit_secs: Some(20.0),
            simple_mate: true,
            threads,
            ..Default::default()
        });

        for &(name, sfen, min_steps) in sound_cases {
            let p_fmrs = FmrsPos::from_sfen(sfen).unwrap();
            let p_sh = ShtsumePos::from_sfen(sfen).unwrap();

            let res_fmrs = solver_fmrs.solve(&p_fmrs);
            let res_sh = solver_sh.solve(&p_sh);

            assert_eq!(res_fmrs.mate, Some(true), "[{} t={}] fmrs mate", name, threads);
            assert_eq!(res_sh.mate, Some(true), "[{} t={}] shtsume mate", name, threads);
            assert!(
                res_fmrs.mate_steps >= min_steps,
                "[{} t={}] fmrs simple_mate steps {} < std {}",
                name,
                threads,
                res_fmrs.mate_steps,
                min_steps
            );
            assert!(
                res_sh.mate_steps >= min_steps,
                "[{} t={}] shtsume simple_mate steps {} < std {}",
                name,
                threads,
                res_sh.mate_steps,
                min_steps
            );
            assert_eq!(res_fmrs.pv.len(), res_fmrs.mate_steps as usize);
            assert_eq!(res_sh.pv.len(), res_sh.mate_steps as usize);
            verify_pv(&p_fmrs, &res_fmrs.pv);
            verify_pv(&p_sh, &res_sh.pv);
        }
    }
}
