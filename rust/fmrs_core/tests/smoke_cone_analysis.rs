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
            let best_piece = value_map.get(&d).copied().unwrap_or(0).max(piece_count(p));
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
