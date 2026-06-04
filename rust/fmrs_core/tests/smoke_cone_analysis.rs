// Smoke best-cone analysis + ML dataset builder.
//
// Studies how the "best cone" (solution paths of the max-piece positions at
// each step) relates to the full backward-search frontier, and emits a labeled
// dataset for learning which frontier positions are "live" (lead to a deep
// max-piece best) -- the guidance signal needed to push toward the 40-piece
// goal (see analysis/smoke_cone/REPORT.md).
//
// Inputs (env):
//   FMRS_CONE_DATA      dir with best_step_<S>.txt (max-piece positions at step
//                       S) and optionally frontier_sample_<S>.txt (uniform
//                       frontier sample). Produced by a run with
//                       FMRS_PERSTEP_BEST_DIR / FMRS_FRONTIER_SAMPLE_DIR set.
//   FMRS_CONE_FRONTIER  file with lines "<step> <frontier_size>" (mem-trace).
//   FMRS_DATASET_OUT    if set, write a labeled CSV dataset there.
//
// Labels (per position): max_best_depth = deepest step D at which the position
// is an ancestor of a max-piece best-at-D (0 if none); best_piece_reachable =
// piece count of that best; live_deeper = max_best_depth > own step.
//
// No-op (passes) unless FMRS_CONE_DATA is set.

use fmrs_core::position::position::PositionAux;
use fmrs_core::solve::standard_solve::standard_solve;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::Write;

fn piece_count(p: &PositionAux) -> u32 {
    p.occupied_bb().count_ones()
}

fn canon_digest(p: &PositionAux) -> u64 {
    fmrs_core::search::canonicalize::canonical_digest_for_smoke(p)
}

fn parse_pos(line: &str) -> Option<PositionAux> {
    let line = line.trim();
    if line.is_empty() {
        return None;
    }
    let sfen = if line.starts_with("http") {
        fmrs_core::sfen::from_image_url(line).ok()?
    } else {
        line.to_string()
    };
    PositionAux::from_sfen(&sfen).ok()
}

fn read_positions(path: &str) -> Vec<PositionAux> {
    std::fs::read_to_string(path)
        .map(|t| t.lines().filter_map(parse_pos).collect())
        .unwrap_or_default()
}

#[test]
fn smoke_cone_analysis() {
    let Ok(dir) = std::env::var("FMRS_CONE_DATA") else {
        return;
    };
    let mut frontier: HashMap<u16, u64> = HashMap::new();
    if let Ok(fpath) = std::env::var("FMRS_CONE_FRONTIER") {
        if let Ok(txt) = std::fs::read_to_string(&fpath) {
            for line in txt.lines() {
                let mut it = line.split_whitespace();
                if let (Some(Ok(s)), Some(Ok(n))) =
                    (it.next().map(str::parse), it.next().map(str::parse))
                {
                    frontier.insert(s, n);
                }
            }
        }
    }

    let mut targets: Vec<u16> = Vec::new();
    for entry in std::fs::read_dir(&dir).unwrap() {
        let name = entry.unwrap().file_name().into_string().unwrap();
        if let Some(rest) = name.strip_prefix("best_step_").and_then(|s| s.strip_suffix(".txt")) {
            if let Ok(s) = rest.parse::<u16>() {
                targets.push(s);
            }
        }
    }
    targets.sort_unstable();
    let deepest = *targets.last().unwrap();

    // Trace every per-step best's unique solution toward mate.
    //   cone_map[digest] = deepest target D whose best-cone contains digest
    //                      (= deepest max-piece best this position is an
    //                      ancestor of).
    //   best_piece_at[D] = max piece count at step D.
    let mut cone_map: HashMap<u64, u16> = HashMap::new();
    // value_map[digest] = max piece count of any DEEP endpoint reachable by
    // continuing the backward search from this position (a lower bound on the
    // position's true "reachable value"). Built by tracing both the per-step
    // bests and a subsample of deep frontier positions back toward mate and
    // propagating the endpoint's piece count to every position on the path.
    // This is the regression target a beam wants to learn.
    let mut value_map: HashMap<u64, u32> = HashMap::new();
    let mut best_piece_at: BTreeMap<u16, u32> = BTreeMap::new();
    let mut live: BTreeMap<u16, HashSet<u64>> = BTreeMap::new();
    let mut live_deeper: BTreeMap<u16, HashSet<u64>> = BTreeMap::new();
    let mut per_target_at_step: BTreeMap<u16, BTreeMap<u16, usize>> = BTreeMap::new();
    let mut piece_at_step: BTreeMap<u16, (u32, u32)> = BTreeMap::new();

    // Cap on per-step bests traced (deep steps can have 100k+ max-piece
    // positions; tracing all is prohibitive). Strided subsample.
    let best_trace_cap: usize = std::env::var("FMRS_BEST_TRACE_CAP")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(usize::MAX);
    for &s_target in &targets {
        let all = read_positions(&format!("{dir}/best_step_{s_target}.txt"));
        let stride = (all.len() / best_trace_cap.max(1)).max(1);
        let positions: Vec<PositionAux> = all.iter().step_by(stride).take(best_trace_cap).cloned().collect();
        if let Some(p0) = positions.first() {
            best_piece_at.insert(s_target, piece_count(p0));
        }
        let mut this_target: BTreeMap<u16, HashSet<u64>> = BTreeMap::new();
        for pos in &positions {
            let sols = standard_solve(pos.clone(), 2, true).unwrap().solutions();
            assert_eq!(sols.len(), 1, "not unique at step {s_target}");
            let sol = &sols[0];
            let n = sol.len() as u16;
            let mut p = pos.clone();
            let endpoint_piece = piece_count(pos);
            let mut record = |step: u16, p: &PositionAux| {
                let d = canon_digest(p);
                this_target.entry(step).or_default().insert(d);
                live.entry(step).or_default().insert(d);
                if s_target > step {
                    live_deeper.entry(step).or_default().insert(d);
                }
                let e = cone_map.entry(d).or_insert(0);
                *e = (*e).max(s_target);
                let v = value_map.entry(d).or_insert(0);
                *v = (*v).max(endpoint_piece);
                if s_target == deepest {
                    let pc = piece_count(p);
                    let pe = piece_at_step.entry(step).or_insert((u32::MAX, 0));
                    pe.0 = pe.0.min(pc);
                    pe.1 = pe.1.max(pc);
                }
            };
            record(n, &p);
            for (k, m) in sol.iter().enumerate() {
                p.do_move(m);
                record(n - 1 - k as u16, &p);
            }
        }
        per_target_at_step.insert(
            s_target,
            this_target.iter().map(|(s, set)| (*s, set.len())).collect(),
        );
    }

    // Enrichment: trace a subsample of DEEP frontier positions back toward mate
    // and propagate each endpoint's piece count into value_map. This labels many
    // more positions with a reachable-value lower bound than the max-piece spine
    // alone. Steps >= FMRS_TRACE_DEEP_FROM (default 21), up to FMRS_TRACE_CAP
    // (default 4000) endpoints per step.
    let trace_from: u16 = std::env::var("FMRS_TRACE_DEEP_FROM")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(21);
    let trace_cap: usize = std::env::var("FMRS_TRACE_CAP")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(4000);
    for s in (trace_from..=deepest).filter(|s| s % 2 == 1) {
        let sample = read_positions(&format!("{dir}/frontier_sample_{s}.txt"));
        if sample.is_empty() {
            continue;
        }
        let stride = (sample.len() / trace_cap).max(1);
        for pos in sample.iter().step_by(stride).take(trace_cap) {
            let sols = standard_solve(pos.clone(), 2, true).unwrap().solutions();
            if sols.len() != 1 {
                continue;
            }
            let endpoint_piece = piece_count(pos);
            let mut p = pos.clone();
            let mut prop = |p: &PositionAux| {
                let v = value_map.entry(canon_digest(p)).or_insert(0);
                *v = (*v).max(endpoint_piece);
            };
            prop(&p);
            for m in &sols[0] {
                p.do_move(m);
                prop(&p);
            }
        }
    }

    // Report 1: deepest-target cone vs frontier.
    eprintln!("\n=== CONE(deepest={deepest}) distinct vs frontier (odd steps) ===");
    eprintln!("step  cone  frontier  cone/frontier");
    let deep_cone = &per_target_at_step[&deepest];
    for s in (1..=deepest).rev().filter(|s| s % 2 == 1) {
        let c = deep_cone.get(&s).copied().unwrap_or(0);
        let f = frontier.get(&s).copied().unwrap_or(0);
        let frac = if f > 0 { c as f64 / f as f64 * 100.0 } else { 0.0 };
        eprintln!("{s:>3}  {c:>5}  {f:>9}  {frac:.4}%");
    }

    // Report 2: live vs frontier.
    eprintln!("\n=== LIVE vs frontier (odd steps; canonical-deduped) ===");
    eprintln!("step  live  live_deep  frontier  live/frontier  livedeep/frontier");
    for s in (1..=deepest).rev().filter(|s| s % 2 == 1) {
        let l = live.get(&s).map(|x| x.len()).unwrap_or(0);
        let ld = live_deeper.get(&s).map(|x| x.len()).unwrap_or(0);
        let f = frontier.get(&s).copied().unwrap_or(0);
        let frac = if f > 0 { l as f64 / f as f64 * 100.0 } else { 0.0 };
        let frac_d = if f > 0 { ld as f64 / f as f64 * 100.0 } else { 0.0 };
        eprintln!("{s:>3}  {l:>5}  {ld:>8}  {f:>9}  {frac:>9.4}%  {frac_d:>9.4}%");
    }

    // Report 3: piece count along the deepest cone.
    eprintln!("\n=== piece count along deepest cone (min..max) ===");
    for (s, (lo, hi)) in piece_at_step.iter().rev() {
        eprintln!("step {s:>3}  pieces {lo}..{hi}");
    }

    // Dataset emission.
    let Ok(out) = std::env::var("FMRS_DATASET_OUT") else {
        return;
    };
    // Exact value-DP labels (direction 2): if an edge-value file is given
    // (digest u64 + value u8, from analysis/smoke_cone/edge_value_dp.py), use it
    // as best_piece_reachable — exact within the recorded DAG, vs the trace
    // lower bound. Keyed by canonical_digest_for_smoke (same as value_map).
    let mut edge_value: HashMap<u64, u32> = HashMap::new();
    if let Ok(epath) = std::env::var("FMRS_EDGE_VALUE_FILE") {
        if let Ok(buf) = std::fs::read(&epath) {
            for ch in buf.chunks_exact(9) {
                let d = u64::from_le_bytes(ch[0..8].try_into().unwrap());
                edge_value.insert(d, ch[8] as u32);
            }
        }
        eprintln!("loaded {} edge-DP values", edge_value.len());
    }
    let mut rows = 0u64;
    let mut pos_rows = 0u64;
    let mut seen: HashSet<u64> = HashSet::new();
    let mut f = std::io::BufWriter::new(std::fs::File::create(&out).unwrap());
    writeln!(
        f,
        "step,piece_count,live_deeper,max_best_depth,best_piece_reachable,sfen"
    )
    .unwrap();

    // Candidate rows: per-step bests (the discriminative population) + frontier
    // samples (broad negatives). Dedup by canonical digest.
    for s in (1..=deepest).filter(|s| s % 2 == 1) {
        let mut cands = read_positions(&format!("{dir}/best_step_{s}.txt"));
        cands.extend(read_positions(&format!("{dir}/frontier_sample_{s}.txt")));
        for p in &cands {
            let d = canon_digest(p);
            if !seen.insert(d) {
                continue;
            }
            let max_depth = cone_map.get(&d).copied().unwrap_or(0);
            let live_deeper = (max_depth > s) as u8;
            // Reachable value (regression target): max deep-endpoint piece count
            // reachable from this position. Lower bound = own piece count (a
            // position trivially "reaches" its own count; deeper descendants
            // only add pieces), raised by traced deep endpoints.
            let best_piece = value_map
                .get(&d)
                .copied()
                .unwrap_or(0)
                .max(edge_value.get(&d).copied().unwrap_or(0))
                .max(piece_count(p));
            writeln!(
                f,
                "{},{},{},{},{},{}",
                s,
                piece_count(p),
                live_deeper,
                max_depth,
                best_piece,
                p.sfen()
            )
            .unwrap();
            rows += 1;
            if live_deeper == 1 {
                pos_rows += 1;
            }
        }
    }
    eprintln!("\n=== DATASET written to {out}: {rows} rows, {pos_rows} live_deeper positives ===");
}

// Confluence analysis for a SET of solved positions (e.g. the 39-piece best
// set). Traces every position's unique solution toward mate and, aligned by
// distance-from-mate `d` (d=0 is the mate), counts how many DISTINCT canonical
// positions the set occupies. Where the count collapses to 1, all solutions
// pass through a single common position -- a confluence/gateway whose backward
// cone is a promising place to push the search deeper.
//
// Input: FMRS_CONFLUENCE_SFENS = file with one SFEN or fmrs image-URL per line.
// No-op unless set.
#[test]
fn smoke_confluence() {
    let Ok(path) = std::env::var("FMRS_CONFLUENCE_SFENS") else {
        return;
    };
    let positions = read_positions(&path);
    assert!(!positions.is_empty(), "no positions parsed from {path}");
    eprintln!("confluence: {} input positions", positions.len());

    // d (plies-to-mate) -> set of canonical digests across all solutions.
    let mut by_dist: BTreeMap<u16, HashSet<u64>> = BTreeMap::new();
    // piece count -> set of canonical digests (pieces are ~monotone along the
    // smoke solution, so this is an alternative natural alignment).
    let mut by_pieces: BTreeMap<u32, HashSet<u64>> = BTreeMap::new();
    // d -> the single digest when fully converged (for printing the gateway).
    let mut sample_at: BTreeMap<u16, PositionAux> = BTreeMap::new();

    let mut lengths: Vec<u16> = Vec::new();
    let mut non_unique = 0usize;
    for pos in &positions {
        let sols = standard_solve(pos.clone(), 2, true).unwrap().solutions();
        if sols.len() != 1 {
            non_unique += 1;
            continue;
        }
        let sol = &sols[0];
        lengths.push(sol.len() as u16);
        let mut p = pos.clone();
        let mut rec = |d: u16, p: &PositionAux| {
            by_dist.entry(d).or_default().insert(canon_digest(p));
            by_pieces.entry(piece_count(p)).or_default().insert(canon_digest(p));
            sample_at.entry(d).or_insert_with(|| p.clone());
        };
        // d = remaining plies to mate. Root has d = sol.len(); mate has d = 0.
        rec(sol.len() as u16, &p);
        for (k, m) in sol.iter().enumerate() {
            p.do_move(m);
            rec(sol.len() as u16 - 1 - k as u16, &p);
        }
    }
    lengths.sort_unstable();
    let lmin = lengths.first().copied().unwrap_or(0);
    let lmax = lengths.last().copied().unwrap_or(0);
    eprintln!(
        "solved unique: {} (non-unique skipped: {}), solution plies: min={} max={}",
        lengths.len(), non_unique, lmin, lmax
    );

    eprintln!("\n=== distinct canonical positions by distance-from-mate d ===");
    eprintln!("  d  distinct   (d=0 is mate; root is at d=plies)");
    let mut first_branch: Option<u16> = None; // smallest d with distinct>1
    let mut last_converged: Option<u16> = None; // largest d with distinct==1
    for (&d, set) in by_dist.iter() {
        eprintln!("{:4}  {:7}", d, set.len());
        if set.len() == 1 {
            last_converged = Some(last_converged.map_or(d, |x| x.max(d)));
        } else if first_branch.is_none() {
            first_branch = Some(d);
        }
    }
    // The gateway: the deepest d (furthest from mate) at which all solutions are
    // still a single common position before branching as d grows.
    let mut gateway_d = 0u16;
    for (&d, set) in by_dist.iter() {
        if set.len() == 1 {
            gateway_d = gateway_d.max(d);
        } else {
            break; // first branch as d increases from 0
        }
    }
    eprintln!(
        "\nconfluence gateway: solutions share one position up to d={} from mate{}",
        gateway_d,
        match sample_at.get(&gateway_d) {
            Some(p) => format!(" -> {}", p.sfen()),
            None => String::new(),
        }
    );
    eprintln!(
        "first branch at d={:?}, last fully-converged d={:?}",
        first_branch, last_converged
    );

    eprintln!("\n=== distinct canonical positions by piece count ===");
    eprintln!("pieces  distinct");
    for (&pc, set) in by_pieces.iter() {
        eprintln!("{:6}  {:7}", pc, set.len());
    }
}

// Dump the unique solution path of a single position: per ply, (d, board+hand
// pieces, sfen). FMRS_DUMP_PATH_SFEN = the SFEN/URL. No-op unless set.
#[test]
fn smoke_dump_path() {
    let Ok(s) = std::env::var("FMRS_DUMP_PATH_SFEN") else { return; };
    let pos = parse_pos(&s).expect("parse");
    let sols = standard_solve(pos.clone(), 2, true).unwrap().solutions();
    assert_eq!(sols.len(), 1, "not unique");
    let sol = &sols[0];
    let mut p = pos.clone();
    let n = sol.len() as u16;
    eprintln!("PATHDUMP plies={n}");
    eprintln!("PATH d={} pieces={} {}", n, piece_count(&p), p.sfen());
    for (k, m) in sol.iter().enumerate() {
        p.do_move(m);
        eprintln!("PATH d={} pieces={} {}", n - 1 - k as u16, piece_count(&p), p.sfen());
    }
}

// Classify a set of solved positions into "essentially different" procedure
// classes. Two solutions are equivalent iff their unique mate sequences agree on
// ALL three ply-indexed projections:
//   (1) the defender (white) king's trajectory (square per position),
//   (2) the TYPE of piece the attacker (black) captures at each ply,
//   (3) the SQUARE at which the defender (white) captures at each ply.
// They are "essentially different" if they differ in at least one.
// Input: FMRS_ESSENTIAL_SFENS = file of SFEN/URL lines. Optional FMRS_ESSENTIAL_OUT
// = path to write one representative SFEN per class. No-op unless the env is set.
#[test]
fn smoke_essential_classes() {
    let Ok(path) = std::env::var("FMRS_ESSENTIAL_SFENS") else { return; };
    use fmrs_core::piece::{Color, Kind};
    use fmrs_core::position::Movement;
    use rayon::prelude::*;

    let positions = read_positions(&path);
    assert!(!positions.is_empty(), "no positions parsed from {path}");
    eprintln!("essential: {} input positions", positions.len());

    type Sig = (Vec<u16>, Vec<i32>, Vec<i32>); // (king_traj, atk_capture_kind, def_capture_square)

    let wk = |p: &PositionAux| -> u16 {
        p.bitboard(Color::WHITE, Kind::King).singleton().index() as u16
    };

    let results: Vec<Option<(Sig, String, u16)>> = positions
        .par_iter()
        .map(|pos| {
            let sols = standard_solve(pos.clone(), 2, true).ok()?.solutions();
            if sols.len() != 1 {
                return None;
            }
            let sol = &sols[0];
            let mut p = pos.clone();
            let mut king: Vec<u16> = vec![wk(&p)];
            let mut atk: Vec<i32> = Vec::with_capacity(sol.len());
            let mut def: Vec<i32> = Vec::with_capacity(sol.len());
            for m in sol {
                let mover = p.turn();
                let (cap_kind, cap_sq) = match m {
                    Movement::Move { dest, .. } => match p.get(*dest) {
                        Some((_c, k)) => (Some(k as i32), Some(dest.index() as i32)),
                        None => (None, None),
                    },
                    Movement::Drop(_, _) => (None, None),
                };
                if mover == Color::BLACK {
                    // attacker move: record captured TYPE (proj 2); no defender capture this ply
                    atk.push(cap_kind.unwrap_or(-1));
                    def.push(-1);
                } else {
                    // defender move: record captured SQUARE (proj 3); no attacker capture this ply
                    atk.push(-1);
                    def.push(cap_sq.unwrap_or(-1));
                }
                p.do_move(m);
                king.push(wk(&p));
            }
            Some(((king, atk, def), pos.sfen(), sol.len() as u16))
        })
        .collect();

    // Aggregate.
    let mut classes: HashMap<Sig, (usize, String)> = HashMap::new();
    let mut king_set: HashSet<Vec<u16>> = HashSet::new();
    let mut atk_set: HashSet<Vec<i32>> = HashSet::new();
    let mut def_set: HashSet<Vec<i32>> = HashSet::new();
    let mut nonuniq = 0usize;
    let mut plies: HashSet<u16> = HashSet::new();
    for r in &results {
        match r {
            Some((sig, sfen, n)) => {
                plies.insert(*n);
                king_set.insert(sig.0.clone());
                atk_set.insert(sig.1.clone());
                def_set.insert(sig.2.clone());
                let e = classes.entry(sig.clone()).or_insert((0, sfen.clone()));
                e.0 += 1;
            }
            None => nonuniq += 1,
        }
    }
    eprintln!(
        "solved unique: {} (non-unique skipped: {}), solution plies seen: {:?}",
        results.len() - nonuniq,
        nonuniq,
        {
            let mut v: Vec<_> = plies.into_iter().collect();
            v.sort_unstable();
            v
        }
    );
    eprintln!("distinct by projection (independent):");
    eprintln!("  (1) defender-king trajectories : {}", king_set.len());
    eprintln!("  (2) attacker capture-TYPE seqs : {}", atk_set.len());
    eprintln!("  (3) defender capture-SQUARE seqs: {}", def_set.len());
    eprintln!("=> ESSENTIALLY DIFFERENT classes (all three agree): {}", classes.len());

    // Sort classes by size desc, then by representative sfen for determinism.
    let mut reps: Vec<(usize, String)> =
        classes.into_iter().map(|(_, (cnt, sfen))| (cnt, sfen)).collect();
    reps.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));

    eprintln!("\n=== class representatives (size  sfen) ===");
    for (cnt, sfen) in &reps {
        eprintln!("{:6}  {}", cnt, sfen);
    }
    if let Ok(out) = std::env::var("FMRS_ESSENTIAL_OUT") {
        use std::io::Write;
        let mut f = std::fs::File::create(&out).unwrap();
        for (cnt, sfen) in &reps {
            writeln!(f, "{}\t{}", cnt, sfen).unwrap();
        }
        eprintln!("\nwrote {} representatives to {}", reps.len(), out);
    }
}

// Extract the "most beautiful" all-piece smoke from a set, by the lexicographic
// preference (all minimized):
//   1. first move is NOT a capture  (first_cap: 0 better than 1)
//   2. fewer promoted Silver/Knight/Lance on board:
//        2a. number of distinct kinds present among {+S,+N,+L}  (minimize)
//        2b. total count of {+S,+N,+L}                          (minimize)
//   3. fewer promoted Bishop/Rook on board:
//        3a. number of distinct kinds present among {+B,+R}     (minimize)
//        3b. total count of {+B,+R}                             (minimize)
// All positions tying at the optimum are emitted (prefixed "BEST\t<sfen>").
// Input: FMRS_BEAUTIFUL_SFENS = file of SFEN/URL. No-op unless set.
#[test]
fn smoke_beautiful() {
    let Ok(path) = std::env::var("FMRS_BEAUTIFUL_SFENS") else { return; };
    use fmrs_core::piece::{Color, Kind};
    use fmrs_core::position::Movement;
    use rayon::prelude::*;

    let positions = read_positions(&path);
    assert!(!positions.is_empty(), "no positions parsed from {path}");

    let cnt = |p: &PositionAux, k: Kind| -> u32 {
        p.bitboard(Color::BLACK, k).count_ones() + p.bitboard(Color::WHITE, k).count_ones()
    };
    let snl = [Kind::ProSilver, Kind::ProKnight, Kind::ProLance];
    let br = [Kind::ProBishop, Kind::ProRook];

    let scored: Vec<Option<([u8; 5], String)>> = positions
        .par_iter()
        .map(|pos| {
            let sols = standard_solve(pos.clone(), 2, true).ok()?.solutions();
            if sols.len() != 1 {
                return None;
            }
            let first_cap = match &sols[0][0] {
                Movement::Drop(_, _) => 0u8,
                Movement::Move { dest, .. } => {
                    if pos.get(*dest).is_some() {
                        1
                    } else {
                        0
                    }
                }
            };
            let snl_kinds = snl.iter().filter(|&&k| cnt(pos, k) > 0).count() as u8;
            let snl_total = snl.iter().map(|&k| cnt(pos, k)).sum::<u32>() as u8;
            let br_kinds = br.iter().filter(|&&k| cnt(pos, k) > 0).count() as u8;
            let br_total = br.iter().map(|&k| cnt(pos, k)).sum::<u32>() as u8;
            Some((
                [first_cap, snl_kinds, snl_total, br_kinds, br_total],
                pos.sfen(),
            ))
        })
        .collect();

    let mut items: Vec<([u8; 5], String)> = scored.into_iter().flatten().collect();
    let total = items.len();
    items.sort();
    let best = items[0].0;
    let ties: Vec<&([u8; 5], String)> = items.iter().filter(|(k, _)| *k == best).collect();

    eprintln!("scored unique positions: {}", total);
    eprintln!("key = [first_move_is_capture, SNLkinds, SNLtotal, BRkinds, BRtotal] (all minimized)");
    eprintln!("OPTIMUM key = {:?}", best);
    eprintln!("ties at optimum: {}", ties.len());
    // small context: how the population thins by criterion.
    let non_cap = items.iter().filter(|(k, _)| k[0] == 0).count();
    eprintln!(
        "  of {}: non-capturing first move = {}; then min SNLkinds among those = {}",
        total,
        non_cap,
        items
            .iter()
            .filter(|(k, _)| k[0] == 0)
            .map(|(k, _)| k[1])
            .min()
            .unwrap_or(0)
    );
    for (_, s) in &ties {
        eprintln!("BEST\t{}", s);
    }
}

// Second-stage tiebreaks applied to an already-filtered set (the 72 "beautiful"
// all-piece smokes). Lexicographic, all minimized:
//   t1: # of promoted Lance/Knight/Silver located OUTSIDE their color's enemy
//       camp (black enemy camp = ranks 1-3 = row 0..2; white = ranks 7-9 = row
//       6..8). In-camp promotions carry no penalty.
//   t2: total promoted pieces on board (incl. tokin/+P), both colors.
//   t3: first move is a tokin (+P) move? 1 if yes (worse) else 0.
//   t4: sum of Manhattan distance from the opponent (white/defender) king to
//       every piece on the board (smaller = pieces nearer the mated king).
// All positions tying at the optimum are emitted ("BEST2\t<sfen>").
// Input: FMRS_BEAUTIFUL2_SFENS = file of SFEN/URL. No-op unless set.
#[test]
fn smoke_beautiful2() {
    let Ok(path) = std::env::var("FMRS_BEAUTIFUL2_SFENS") else { return; };
    use fmrs_core::piece::{Color, Kind};
    use fmrs_core::position::{Movement, Square};
    use rayon::prelude::*;

    let positions = read_positions(&path);
    assert!(!positions.is_empty(), "no positions parsed from {path}");

    let scored: Vec<Option<([i32; 4], String)>> = positions
        .par_iter()
        .map(|pos| {
            let sols = standard_solve(pos.clone(), 2, true).ok()?.solutions();
            if sols.len() != 1 {
                return None;
            }
            let wk = pos.bitboard(Color::WHITE, Kind::King).singleton();
            let mut t1 = 0i32; // promoted L/N/S outside own enemy camp
            let mut t2 = 0i32; // total promoted pieces
            let mut t4 = 0i32; // sum manhattan(white king, piece)
            for sq in Square::iter() {
                if let Some((c, k)) = pos.get(sq) {
                    if (k as u8) >= 8 {
                        t2 += 1;
                    }
                    if matches!(k, Kind::ProLance | Kind::ProKnight | Kind::ProSilver) {
                        let in_camp = if c == Color::BLACK {
                            sq.row() <= 2
                        } else {
                            sq.row() >= 6
                        };
                        if !in_camp {
                            t1 += 1;
                        }
                    }
                    t4 += (wk.col() as i32 - sq.col() as i32).abs()
                        + (wk.row() as i32 - sq.row() as i32).abs();
                }
            }
            let t3 = match &sols[0][0] {
                Movement::Move { source, .. } => {
                    matches!(pos.get(*source), Some((_, Kind::ProPawn))) as i32
                }
                Movement::Drop(_, _) => 0,
            };
            Some(([t1, t2, t3, t4], pos.sfen()))
        })
        .collect();

    let mut items: Vec<([i32; 4], String)> = scored.into_iter().flatten().collect();
    let total = items.len();
    items.sort();
    let best = items[0].0;
    let ties: Vec<&([i32; 4], String)> = items.iter().filter(|(k, _)| *k == best).collect();
    eprintln!("input positions: {}", total);
    eprintln!("key = [LNS_promoted_outside_camp, total_promoted, first_move_is_tokin, sum_manhattan_to_white_king] (all minimized)");
    eprintln!("OPTIMUM key = {:?}", best);
    eprintln!("ties at optimum: {}", ties.len());
    // context: spread of each component over the input set
    for i in 0..4 {
        let mut vals: Vec<i32> = items.iter().map(|(k, _)| k[i]).collect();
        vals.sort_unstable();
        vals.dedup();
        eprintln!("  component[{}] distinct values present: {:?}", i, vals);
    }
    for (_, s) in &ties {
        eprintln!("BEST2\t{}", s);
    }
}
