//! Benchmark CLI for the normal tsume-shogi solver (`fmrs_core::futsuu`),
//! comparing `ShtsumePos` (reference shtsume backend) and `FmrsPos` (`fmrs` core backend)
//! across standard 1T deterministic and parallel long-mate suites.

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

use fmrs_core::futsuu::tbase::TBase;
use fmrs_core::futsuu::{
    FmrsPos, Move, PositionBackend, SearchConfig, SearchResult, ShtsumePos, Solver,
};
use serde::Serialize;

fn check_pv_legality<P: PositionBackend>(root_sfen: &str, pv: &[Move]) -> Result<(), String> {
    if pv.is_empty() || pv.len().is_multiple_of(2) {
        return Err(format!("invalid pv len={}", pv.len()));
    }
    let mut tb = TBase::new(1);
    let mut pos = P::from_sfen(root_sfen).map_err(|e| e.to_string())?;
    for (ply, &mv) in pv.iter().enumerate() {
        let legal: Vec<Move> = if ply % 2 == 0 {
            pos.generate_check(&mut tb)
                .into_iter()
                .flat_map(|it| it.all_moves())
                .collect()
        } else {
            pos.generate_evasion_simple(&mut tb)
                .into_iter()
                .flat_map(|it| it.all_moves())
                .collect()
        };
        if !legal.contains(&mv) {
            return Err(format!("illegal move {} at ply {}", mv.to_sfen(), ply + 1));
        }
        pos = pos.move_forward(mv);
    }
    let evasions = pos.generate_evasion_simple(&mut tb);
    if !evasions.is_empty() {
        return Err(format!(
            "final position has {} evasions remaining",
            evasions.len()
        ));
    }
    Ok(())
}

struct Problem {
    name: &'static str,
    note: &'static str,
    sfen: &'static str,
    is_long: bool,
}

const STD_PROBLEMS: &[Problem] = &[
    Problem {
        name: "sample_classic_3",
        note: "3手 (shtsume sample)",
        sfen: "3sks3/9/4S4/9/9/B8/9/9/9 b S2rb4g4n4l18p 1",
        is_long: false,
    },
    Problem {
        name: "yane_2_5",
        note: "5手 (YaneuraOu test)",
        sfen: "7nl/7k1/6p2/6S1p/9/9/9/9/9 b GS2r2b3g2s3n3l16p 1",
        is_long: false,
    },
    Problem {
        name: "lances_11",
        note: "11手 (shtsume wiki / YaneuraOu)",
        sfen: "4k4/9/PPPPPPPPP/9/9/9/9/9/9 b B4L2rb4g4s4n9p 1",
        is_long: false,
    },
    Problem {
        name: "zuko_05_21",
        note: "21手 (将棋図巧 第5番)",
        sfen: "n+B1sS4/1R1g5/1Ls6/2k6/2n6/3L5/R8/9/9 b B2P3gs2n2l16p 1",
        is_long: false,
    },
    Problem {
        name: "yane_5_fg",
        note: "実戦詰 (YaneuraOu Floodgate #5)",
        sfen: "6lnk/6+Rbl/2n4pp/7s1/1p2P2NP/p1P2PPP1/1P4GS1/6GK1/LNr5L b B2G2S6Pp 1",
        is_long: false,
    },
    Problem {
        name: "yane_7_fg",
        note: "実戦詰 (YaneuraOu Floodgate #7)",
        sfen: "l7l/2+Rbk4/3rp4/2p3pPs/p2P1p2p/2P1G4/P1N1PPN2/2GK2G2/L7L b B2S6Pgs2n 1",
        is_long: false,
    },
    Problem {
        name: "yane_10_fg_23",
        note: "23手 (YaneuraOu Floodgate #10)",
        sfen: "l2s3nl/3g1p+R+R1/p1k5p/2pPp4/1p1p5/5Sp2/PPP1PP2P/3G5/L1K4NL b BG2S2Pbg2np 1",
        is_long: false,
    },
    Problem {
        name: "yane_13_fg",
        note: "実戦詰 (YaneuraOu Floodgate #13)",
        sfen: "lng3+R2/2kgs4/ppp6/1B1pp4/7B1/2P2pLp1/PP1PP3P/1S1K2p2/LN5GL b RG2SP2n3p 1",
        is_long: false,
    },
    Problem {
        name: "zuko_01_69",
        note: "69手 (将棋図巧 第1番)",
        sfen: "1pG1B4/Gs+P6/pP7/n1ls5/3k5/nL4+r1b/1+p1p+R4/1S7/2N6 b SP2gn2l11p 1",
        is_long: true,
    },
    Problem {
        name: "zuko_99_kemuri_117",
        note: "117手 (将棋図巧 第99番 煙詰)",
        sfen: "k1+P4n1/2L+P2sL1/r4+P+P1P/+BpP+Pl1+Rg1/NP1S+PP+p1g/2L+p1g+P1+P/Ps1G1+P1N1/1sN1P4/B8 b - 1",
        is_long: true,
    },
    Problem {
        name: "zuko_100_kotobuki_611",
        note: "611手 (将棋図巧 第100番 寿)",
        sfen: "g3p3+r/3lPG3/1s4ppp/1l1B4P/L2b1kPl1/3+p3+R+p/3+p2+s1S/+NP+p1gS3/1g+p2P3 b 4P3n 1",
        is_long: true,
    },
];

/// Ultra-long problems included in `--suite long`, `--suite all`, `--heavy`, or `--problem microcosmos`.
const HEAVY_PROBLEMS: &[Problem] = &[Problem {
    name: "microcosmos_1525",
    note: "1525手 (橋本孝治 ミクロコスモス)",
    sfen: "g1+P1k1+P+P+L/1p3P3/+R+p2pp1pl/1NNsg+p2+R/+b+nL+P1+p3/1P3ssP1/2P1+Ps2N/4+P1P1L/+B5G1g b - 1",
    is_long: true,
}];

#[derive(Serialize)]
struct BackendMetric {
    mate: Option<bool>,
    steps: u16,
    redundant: bool,
    nodes: u64,
    elapsed_secs: f64,
    knps: f64,
    gc_num: u16,
}

impl From<&SearchResult> for BackendMetric {
    fn from(r: &SearchResult) -> Self {
        Self {
            mate: r.mate,
            steps: r.mate_steps,
            redundant: r.redundant,
            nodes: r.nodes,
            elapsed_secs: (r.elapsed_secs * 10_000.0).round() / 10_000.0,
            knps: (r.nps() / 100.0).round() / 10.0,
            gc_num: r.gc_num,
        }
    }
}

#[derive(Serialize)]
struct ProblemMetric {
    name: &'static str,
    note: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    shtsume: Option<BackendMetric>,
    #[serde(skip_serializing_if = "Option::is_none")]
    fmrs: Option<BackendMetric>,
}

#[derive(Serialize)]
struct BenchReport {
    suite: String,
    hash_mb: usize,
    threads: usize,
    timeout_secs: f64,
    sizeof_shtsume_pos: usize,
    sizeof_fmrs_pos: usize,
    total_shtsume_secs: f64,
    total_fmrs_secs: f64,
    total_shtsume_nodes: u64,
    total_fmrs_nodes: u64,
    agg_shtsume_knps: f64,
    agg_fmrs_knps: f64,
    results: Vec<ProblemMetric>,
}

fn fmt_mate(mate: Option<bool>, steps: u16, redundant: bool) -> String {
    match mate {
        Some(true) => {
            if redundant {
                format!("M{}(余)", steps)
            } else {
                format!("M{}", steps)
            }
        }
        Some(false) => "NoMate".to_string(),
        None => "Timeout".to_string(),
    }
}

fn main() {
    std::thread::Builder::new()
        .stack_size(64 * 1024 * 1024)
        .spawn(run_cli)
        .expect("failed to spawn solver thread")
        .join()
        .expect("solver thread panicked");
}

fn run_cli() {
    let args: Vec<String> = std::env::args().collect();
    let mut suite = "std".to_string();
    let mut hash_mb: usize = 256;
    let mut threads: usize = 0;
    let mut timeout_secs: f64 = 30.0;
    let mut search_level: u8 = 0;
    let mut backend_filter: Option<String> = None;
    let mut problem_filter: Option<String> = None;
    let mut adhoc_sfen: Option<String> = None;
    let mut include_heavy = false;
    let mut show_pv = false;
    let mut use_ml = true;
    let mut use_ni = true;
    let mut use_cycle_skip = false;
    let mut simple_mate = false;
    let mut json_path: Option<String> = None;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--suite" => {
                i += 1;
                suite = args[i].clone();
            }
            "--hash-mb" => {
                i += 1;
                hash_mb = args[i].parse().expect("invalid --hash-mb");
            }
            "--threads" | "-j" => {
                i += 1;
                threads = args[i].parse().expect("invalid --threads");
            }
            "--timeout" => {
                i += 1;
                timeout_secs = args[i].parse().expect("invalid --timeout");
            }
            "--search-level" => {
                i += 1;
                search_level = args[i].parse().expect("invalid --search-level");
            }
            "--backend" => {
                i += 1;
                backend_filter = Some(args[i].clone());
            }
            "--problem" => {
                i += 1;
                problem_filter = Some(args[i].clone());
            }
            "--sfen" => {
                i += 1;
                adhoc_sfen = Some(args[i].clone());
            }
            "--heavy" => {
                include_heavy = true;
            }
            "--pv" => {
                show_pv = true;
            }
            "--no-ml" => {
                use_ml = false;
            }
            "--no-ni" => {
                use_ni = false;
            }
            "--cycle-skip" => {
                use_cycle_skip = true;
            }
            "--simple" => {
                simple_mate = true;
            }
            "--json" => {
                i += 1;
                json_path = Some(args[i].clone());
            }
            _ => {}
        }
        i += 1;
    }

    let cfg = SearchConfig {
        hash_mb,
        threads,
        search_level,
        time_limit_secs: Some(timeout_secs),
        use_ml_ordering: use_ml,
        use_ni_cache: use_ni,
        use_cycle_skip,
        simple_mate,
        ..Default::default()
    };
    let eff_threads = cfg.resolved_threads();

    let run_shtsume = match backend_filter.as_deref() {
        None | Some("both") | Some("shtsume") => true,
        Some("fmrs") => false,
        Some(other) => panic!("unknown --backend value: {other} (expected both|fmrs|shtsume)"),
    };
    let run_fmrs = match backend_filter.as_deref() {
        None | Some("both") | Some("fmrs") => true,
        Some("shtsume") => false,
        Some(other) => panic!("unknown --backend value: {other} (expected both|fmrs|shtsume)"),
    };

    let adhoc_storage: Vec<Problem> = adhoc_sfen
        .as_deref()
        .map(|s| {
            vec![Problem {
                name: "adhoc_sfen",
                note: "adhoc",
                sfen: Box::leak(s.to_string().into_boxed_str()),
                is_long: false,
            }]
        })
        .unwrap_or_default();

    let mut combined_problems: Vec<&Problem> = Vec::new();
    let effective_suite = if let Some(s) = adhoc_storage.first() {
        combined_problems.push(s);
        "adhoc".to_string()
    } else if problem_filter.is_some() {
        combined_problems.extend(STD_PROBLEMS.iter());
        combined_problems.extend(HEAVY_PROBLEMS.iter());
        "filtered".to_string()
    } else {
        match suite.as_str() {
            "std" => {
                combined_problems.extend(STD_PROBLEMS.iter());
                if include_heavy {
                    combined_problems.extend(HEAVY_PROBLEMS.iter());
                    "all".to_string()
                } else {
                    "std".to_string()
                }
            }
            "long" => {
                combined_problems.extend(STD_PROBLEMS.iter().filter(|p| p.is_long));
                combined_problems.extend(HEAVY_PROBLEMS.iter());
                "long".to_string()
            }
            "all" => {
                combined_problems.extend(STD_PROBLEMS.iter());
                combined_problems.extend(HEAVY_PROBLEMS.iter());
                "all".to_string()
            }
            other => panic!("unknown --suite value: {other} (expected std|long|all)"),
        }
    };

    println!(
        "=== fmrs::futsuu Benchmark (suite={}, threads={}, hash={}MB, timeout={}s, sizeof(ShtsumePos)={}B, sizeof(FmrsPos)={}B) ===",
        effective_suite,
        eff_threads,
        hash_mb,
        timeout_secs,
        std::mem::size_of::<ShtsumePos>(),
        std::mem::size_of::<FmrsPos>()
    );
    println!(
        "{:<24} | {:<10} | {:>10} | {:>8} | {:>10} | {:<10} | {:>10} | {:>8} | {:>10} | {:>7}",
        "Problem",
        "SH Res",
        "SH Nodes",
        "SH Time",
        "SH kNPS",
        "FMRS Res",
        "FMRS Nodes",
        "FMRS Time",
        "FMRS kNPS",
        "Speedup"
    );
    println!("{}", "-".repeat(132));

    let mut solver_sh = if run_shtsume {
        Some(Solver::new(cfg.clone()))
    } else {
        None
    };
    let mut solver_fmrs = if run_fmrs {
        Some(Solver::new(cfg))
    } else {
        None
    };

    let mut report_results = Vec::new();
    let mut total_sh_secs = 0.0;
    let mut total_fm_secs = 0.0;
    let mut total_sh_nodes = 0u64;
    let mut total_fm_nodes = 0u64;

    for prob in combined_problems {
        if let Some(ref flt) = problem_filter {
            if !prob.name.contains(flt.as_str()) {
                continue;
            }
        }

        let res_sh = solver_sh.as_mut().map(|s| {
            let pos = ShtsumePos::from_sfen(prob.sfen).expect("shtsume sfen parse failed");
            s.solve(&pos)
        });
        let res_fmrs = solver_fmrs.as_mut().map(|s| {
            let pos = FmrsPos::from_sfen(prob.sfen).expect("fmrs sfen parse failed");
            s.solve(&pos)
        });

        if let Some(r) = &res_sh {
            total_sh_secs += r.elapsed_secs;
            total_sh_nodes += r.nodes;
        }
        if let Some(r) = &res_fmrs {
            total_fm_secs += r.elapsed_secs;
            total_fm_nodes += r.nodes;
        }

        let (sh_res, sh_nodes, sh_time, sh_knps) = match &res_sh {
            Some(r) => (
                fmt_mate(r.mate, r.mate_steps, r.redundant),
                format!("{}", r.nodes),
                format!("{:.3}s", r.elapsed_secs),
                format!("{:.1}", r.nps() / 1000.0),
            ),
            None => (
                "-".to_string(),
                "-".to_string(),
                "-".to_string(),
                "-".to_string(),
            ),
        };
        let (fm_res, fm_nodes, fm_time, fm_knps) = match &res_fmrs {
            Some(r) => (
                fmt_mate(r.mate, r.mate_steps, r.redundant),
                format!("{}", r.nodes),
                format!("{:.3}s", r.elapsed_secs),
                format!("{:.1}", r.nps() / 1000.0),
            ),
            None => (
                "-".to_string(),
                "-".to_string(),
                "-".to_string(),
                "-".to_string(),
            ),
        };
        let speedup = match (&res_sh, &res_fmrs) {
            (Some(s), Some(f)) if f.elapsed_secs > 0.0 => {
                format!("{:.2}x", s.elapsed_secs / f.elapsed_secs)
            }
            _ => "-".to_string(),
        };

        println!(
            "{:<24} | {:<10} | {:>10} | {:>8} | {:>10} | {:<10} | {:>10} | {:>8} | {:>10} | {:>7}",
            prob.name,
            sh_res,
            sh_nodes,
            sh_time,
            sh_knps,
            fm_res,
            fm_nodes,
            fm_time,
            fm_knps,
            speedup
        );

        if show_pv {
            if let (Some(r), Some(s)) = (&res_sh, &solver_sh) {
                let v = check_pv_legality::<ShtsumePos>(prob.sfen, &r.pv);
                let head: Vec<String> = r.pv.iter().take(10).map(|m| m.to_sfen()).collect();
                let tail: Vec<String> = r
                    .pv
                    .iter()
                    .rev()
                    .take(6)
                    .rev()
                    .map(|m| m.to_sfen())
                    .collect();
                println!(
                    "  SH   PV (len={}, gc={}, bn={:.3}s/{}, mt={:.3}s/{}, valid={:?}): {} ... {}",
                    r.pv.len(),
                    r.gc_num,
                    s.last_bn_secs,
                    s.last_bn_nodes,
                    s.last_make_tree_secs,
                    r.nodes.saturating_sub(s.last_bn_nodes),
                    v,
                    head.join(" "),
                    tail.join(" ")
                );
            }
            if let (Some(r), Some(s)) = (&res_fmrs, &solver_fmrs) {
                let v = check_pv_legality::<FmrsPos>(prob.sfen, &r.pv);
                let head: Vec<String> = r.pv.iter().take(10).map(|m| m.to_sfen()).collect();
                let tail: Vec<String> = r
                    .pv
                    .iter()
                    .rev()
                    .take(6)
                    .rev()
                    .map(|m| m.to_sfen())
                    .collect();
                println!(
                    "  FMRS PV (len={}, gc={}, bn={:.3}s/{}, mt={:.3}s/{}, valid={:?}): {} ... {}",
                    r.pv.len(),
                    r.gc_num,
                    s.last_bn_secs,
                    s.last_bn_nodes,
                    s.last_make_tree_secs,
                    r.nodes.saturating_sub(s.last_bn_nodes),
                    v,
                    head.join(" "),
                    tail.join(" ")
                );
                if let Some(cs) = s.cycle_stats() {
                    println!("  FMRS CYCLE: {:?}", cs);
                }
                if let Some(ns) = s.ni_stats() {
                    println!("  FMRS NI: {:?}", ns);
                }
            }
        }

        report_results.push(ProblemMetric {
            name: prob.name,
            note: prob.note,
            shtsume: res_sh.as_ref().map(BackendMetric::from),
            fmrs: res_fmrs.as_ref().map(BackendMetric::from),
        });
    }

    let agg_sh_knps = if total_sh_secs > 0.0 {
        (total_sh_nodes as f64 / total_sh_secs / 100.0).round() / 10.0
    } else {
        0.0
    };
    let agg_fm_knps = if total_fm_secs > 0.0 {
        (total_fm_nodes as f64 / total_fm_secs / 100.0).round() / 10.0
    } else {
        0.0
    };

    println!("{}", "-".repeat(132));
    println!(
        "{:<24} | {:<10} | {:>10} | {:>7.3}s | {:>10.1} | {:<10} | {:>10} | {:>7.3}s | {:>10.1} | {:>6.2}x",
        "TOTAL",
        "-",
        total_sh_nodes,
        total_sh_secs,
        agg_sh_knps,
        "-",
        total_fm_nodes,
        total_fm_secs,
        agg_fm_knps,
        if total_fm_secs > 0.0 && total_sh_secs > 0.0 {
            total_sh_secs / total_fm_secs
        } else {
            0.0
        }
    );

    if let Some(path) = json_path {
        let report = BenchReport {
            suite: effective_suite,
            hash_mb,
            threads: eff_threads,
            timeout_secs,
            sizeof_shtsume_pos: std::mem::size_of::<ShtsumePos>(),
            sizeof_fmrs_pos: std::mem::size_of::<FmrsPos>(),
            total_shtsume_secs: (total_sh_secs * 10_000.0).round() / 10_000.0,
            total_fmrs_secs: (total_fm_secs * 10_000.0).round() / 10_000.0,
            total_shtsume_nodes: total_sh_nodes,
            total_fmrs_nodes: total_fm_nodes,
            agg_shtsume_knps: agg_sh_knps,
            agg_fmrs_knps: agg_fm_knps,
            results: report_results,
        };
        let serialized = serde_json::to_string_pretty(&report).expect("json serialize failed");
        if path == "-" {
            println!("{}", serialized);
        } else {
            std::fs::write(&path, serialized).expect("failed to write json report");
        }
    }
}
