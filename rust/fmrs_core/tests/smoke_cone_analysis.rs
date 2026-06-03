// Smoke best-cone analysis.
//
// Studies how the "best cone" (solution paths of the max-piece positions at
// each step) relates to the full backward-search frontier, for the
// single-king-smoke ideal-backward run. The central question: of the frontier
// at a shallow step (e.g. step 11), what fraction is "live" -- i.e. an ancestor
// (on a solution path) of a max-piece "best" position at SOME deeper step?
//
// Inputs (env):
//   FMRS_CONE_DATA      dir of best_step_<S>.txt (URLs of max-piece positions at
//                       step S), produced by a run with FMRS_PERSTEP_BEST_DIR set.
//   FMRS_CONE_FRONTIER  file with lines "<step> <frontier_size>" (from mem-trace).
//
// Run: FMRS_CONE_DATA=analysis/smoke_cone/data \
//      FMRS_CONE_FRONTIER=analysis/smoke_cone/data/frontier.txt \
//      cargo test -p fmrs_core --test smoke_cone_analysis -- --nocapture
//
// No-op (passes) unless FMRS_CONE_DATA is set.

use fmrs_core::position::position::PositionAux;
use fmrs_core::solve::standard_solve::standard_solve;
use std::collections::{BTreeMap, HashMap, HashSet};

fn piece_count(p: &PositionAux) -> u32 {
    p.occupied_bb().count_ones()
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

#[test]
fn smoke_cone_analysis() {
    let Ok(dir) = std::env::var("FMRS_CONE_DATA") else {
        return;
    };
    // Load frontier sizes (optional).
    let mut frontier: HashMap<u16, u64> = HashMap::new();
    if let Ok(fpath) = std::env::var("FMRS_CONE_FRONTIER") {
        if let Ok(txt) = std::fs::read_to_string(&fpath) {
            for line in txt.lines() {
                let mut it = line.split_whitespace();
                if let (Some(s), Some(n)) = (it.next(), it.next()) {
                    if let (Ok(s), Ok(n)) = (s.parse(), n.parse()) {
                        frontier.insert(s, n);
                    }
                }
            }
        }
    }

    // Discover target steps (best_step_<S>.txt).
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

    // For each target step S: reconstruct the solution of each max-piece
    // position and record, per shallow step s, the set of digests on the path.
    // cone_by_target[S][s] = set of digests at step s on best-at-S paths.
    // Also track piece count seen at each step on the deepest target's paths.
    let mut live: BTreeMap<u16, HashSet<u64>> = BTreeMap::new(); // union over all S >= s
    let mut live_deeper: BTreeMap<u16, HashSet<u64>> = BTreeMap::new(); // union over S > s only
    let mut per_target_at_step: BTreeMap<u16, BTreeMap<u16, usize>> = BTreeMap::new();
    let mut best_count: BTreeMap<u16, usize> = BTreeMap::new();
    let mut piece_at_step: BTreeMap<u16, (u32, u32)> = BTreeMap::new(); // (min,max) over deepest cone
    let deepest = *targets.last().unwrap();

    for &s_target in &targets {
        let path = format!("{dir}/best_step_{s_target}.txt");
        let txt = std::fs::read_to_string(&path).unwrap();
        let positions: Vec<PositionAux> = txt.lines().filter_map(parse_pos).collect();
        best_count.insert(s_target, positions.len());
        let mut this_target: BTreeMap<u16, HashSet<u64>> = BTreeMap::new();
        for pos in &positions {
            let sols = standard_solve(pos.clone(), 2, true).unwrap().solutions();
            assert_eq!(sols.len(), 1, "not unique at step {s_target}");
            let sol = &sols[0];
            let n = sol.len() as u16;
            let mut p = pos.clone();
            let mut record = |step: u16, p: &PositionAux| {
                // Match the frontier's dedup key: the run uses
                // --canonicalize-attacker-goldish, so the frontier counts
                // canonical (goldish-collapsed) classes. Use the same canonical
                // digest, else raw digests over-count vs frontier.
                let d = fmrs_core::search::canonicalize::canonical_digest_for_smoke(p);
                this_target.entry(step).or_default().insert(d);
                live.entry(step).or_default().insert(d);
                if s_target > step {
                    live_deeper.entry(step).or_default().insert(d);
                }
                if s_target == deepest {
                    let pc = piece_count(p);
                    let e = piece_at_step.entry(step).or_insert((u32::MAX, 0));
                    e.0 = e.0.min(pc);
                    e.1 = e.1.max(pc);
                }
            };
            record(n, &p);
            for (k, m) in sol.iter().enumerate() {
                p.do_move(m);
                record(n - 1 - k as u16, &p);
            }
        }
        let mut at: BTreeMap<u16, usize> = BTreeMap::new();
        for (s, set) in &this_target {
            at.insert(*s, set.len());
        }
        per_target_at_step.insert(s_target, at);
    }

    // Report 1: deepest-target cone vs frontier (the original table).
    eprintln!("\n=== CONE(deepest={deepest}) distinct vs frontier (odd steps) ===");
    eprintln!("step  cone  frontier  cone/frontier");
    let deep_cone = &per_target_at_step[&deepest];
    for s in (1..=deepest).rev().filter(|s| s % 2 == 1) {
        let c = deep_cone.get(&s).copied().unwrap_or(0);
        let f = frontier.get(&s).copied().unwrap_or(0);
        let frac = if f > 0 { c as f64 / f as f64 * 100.0 } else { 0.0 };
        eprintln!("{s:>3}  {c:>5}  {f:>9}  {frac:.4}%");
    }

    // Report 2: LIVE set (union over all targets >= s) vs frontier.
    // = how many step-s frontier positions are an ancestor of a max-piece best
    //   at SOME deeper-or-equal step.
    // live      = ancestor of a max-piece best at SOME step >= s (incl. self).
    // live_deep  = ancestor of a max-piece best at SOME step STRICTLY > s
    //              (= "its descendants appear as best deeper"; the user's Q).
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

    // Report 3: piece-count trajectory along the deepest cone (smoke shape).
    eprintln!("\n=== piece count along deepest cone (min..max over cone) ===");
    eprintln!("step  min_pieces  max_pieces");
    for (s, (lo, hi)) in piece_at_step.iter().rev() {
        eprintln!("{s:>3}  {lo:>10}  {hi:>10}");
    }
}
