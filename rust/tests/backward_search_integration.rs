//! Integration tests for the backward search command pipeline.
//!
//! Runs the `fmrs` binary as a subprocess on small inputs and asserts the
//! reported best result. Runs in well under 1 second per test so they're
//! suitable for `cargo test`.
//!
//! Designed to catch correctness regressions in the backward search heuristics
//! (memo cache, killer heuristic, etc.) introduced during optimization work.

use std::process::Command;
use std::time::Duration;

/// Run the `fmrs` binary with the given args and return (stdout, stderr).
/// Panics if the binary fails to execute or exits with non-zero status.
fn run_fmrs(args: &[&str], timeout: Duration) -> (String, String) {
    let binary = env!("CARGO_BIN_EXE_fmrs");
    let start = std::time::Instant::now();
    let output = Command::new(binary)
        .args(args)
        .output()
        .expect("failed to spawn fmrs");
    let elapsed = start.elapsed();
    assert!(
        elapsed < timeout,
        "fmrs took {:?} (limit {:?}) for args {:?}",
        elapsed,
        timeout,
        args
    );
    assert!(
        output.status.success(),
        "fmrs exited with {:?}; stderr:\n{}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr),
    );
    (
        String::from_utf8(output.stdout).expect("stdout not utf8"),
        String::from_utf8(output.stderr).expect("stderr not utf8"),
    )
}

/// Extract the result header from stderr (where progress/summary are written)
/// and all result lines from stdout (image URLs, printed in SFEN-sorted order,
/// one per line). Format:
///   stderr: ... best_pieces=N: positions=M succeeded_seeds=K
///   stdout: <URL_1>\n<URL_2>\n... (SFEN-sorted)
fn extract_best_result(stdout: &str, stderr: &str) -> (String, Vec<String>) {
    let header = stderr
        .lines()
        .rfind(|l| l.starts_with("best_pieces="))
        .unwrap_or_else(|| panic!("no 'best_pieces=' line in stderr:\n{}", stderr))
        .to_string();
    let sfens: Vec<String> = stdout
        .lines()
        .filter(|l| !l.trim().is_empty() && !l.starts_with("progress:"))
        .map(|l| l.to_string())
        .collect();
    (header, sfens)
}

/// Small backward search: same seed as the heavy bench but max-step=5.
/// Exercises the full advance_parallel_filtered pipeline (phase 1 + 2,
/// killer heuristic, square_kinds cache, empty memo, SoA FlatShard) at a size
/// that runs in well under a second. With dynamic inner-parallel, a single
/// seed and `--parallel 8` makes `dynamic_inner = parallel/remaining = 8`,
/// which routes through advance_parallel_filtered when the frontier crosses
/// FRONTIER_PARALLEL_THRESHOLD.
#[test]
fn backward_search_seed_max_step_5() {
    let (stdout, stderr) = run_fmrs(
        &[
            "single-king-smoke",
            "ideal-backward",
            "--max-step",
            "5",
            "--parallel",
            "8",
            "--no-pawn",
            "--max-promoted-pct",
            "20",
            "--max-promoted-pct-after-step",
            "5",
            "--seed-result-log",
            "/dev/null",
            "--seed-sfen",
            "4k4/4+N4/9/9/9/4L4/9/9/9 w 2r2b4g4s3n3l18p 1",
            "--allowed-kinds",
            "pawn,lance,knight,silver,gold",
        ],
        Duration::from_secs(10),
    );

    let (header, sfens) = extract_best_result(&stdout, &stderr);
    assert_eq!(
        header, "best_pieces=5 best_steps=5: positions=36 succeeded_seeds=1",
        "unexpected header in stderr:\n{}",
        stderr
    );
    assert_eq!(sfens.len(), 36, "unexpected SFEN count");
    // Output is image URLs (SFEN with spaces → underscores), printed in
    // SFEN-sorted order. Lock down first/last as a deterministic correctness check.
    assert_eq!(
        sfens[0], "https://ogiekako.github.io/fmrs/2k1G4/4G4/9/9/3N5/4L4/9/9/9_b_2r2b2g4s3n3l18p_1",
        "unexpected first sorted URL"
    );
    assert_eq!(
        sfens[sfens.len() - 1],
        "https://ogiekako.github.io/fmrs/4GG3/9/5k3/9/5N3/4L4/9/9/9_b_2r2b2g4s3n3l18p_1",
        "unexpected last sorted URL"
    );
}

/// Split mode (`--split-start-step` / `--split-chunk-size`) must be exact: it
/// runs the BFS to the split step, then processes the frontier in independent
/// chunks one at a time. The merged result must equal the full (non-split) BFS
/// for the same search — same best header and same output (URL) set — for any
/// split step / chunk size. Covers chunk size 1 (maximum chunk count),
/// multi-position chunks, and a different split step.
#[test]
fn backward_search_split_matches_nonsplit() {
    let base: Vec<&str> = vec![
        "single-king-smoke",
        "ideal-backward",
        "--max-step",
        "5",
        "--parallel",
        "4",
        "--no-pawn",
        "--max-promoted-pct",
        "20",
        "--max-promoted-pct-after-step",
        "5",
        "--seed-result-log",
        "/dev/null",
        "--seed-sfen",
        "4k4/4+N4/9/9/9/4L4/9/9/9 w 2r2b4g4s3n3l18p 1",
        "--allowed-kinds",
        "pawn,lance,knight,silver,gold",
    ];

    let (baseline_header, mut baseline_urls) = {
        let (stdout, stderr) = run_fmrs(&base, Duration::from_secs(20));
        extract_best_result(&stdout, &stderr)
    };
    baseline_urls.sort();
    assert_eq!(
        baseline_header,
        "best_pieces=5 best_steps=5: positions=36 succeeded_seeds=1"
    );

    // (split_start_step, split_chunk_size)
    for (start, chunk) in [("3", "1"), ("3", "4"), ("1", "2")] {
        let mut args = base.clone();
        args.extend_from_slice(&["--split-start-step", start, "--split-chunk-size", chunk]);
        let (stdout, stderr) = run_fmrs(&args, Duration::from_secs(20));
        // Confirm the split path actually engaged (rather than silently running
        // a normal search) so the equivalence check is meaningful.
        assert!(
            stderr.lines().any(|l| l.starts_with("split seed=")),
            "expected a 'split seed=' line for start={start} chunk={chunk}; stderr:\n{stderr}"
        );
        let (header, mut urls) = extract_best_result(&stdout, &stderr);
        urls.sort();
        assert_eq!(
            header, baseline_header,
            "split header mismatch (start={start} chunk={chunk})"
        );
        assert_eq!(
            urls, baseline_urls,
            "split output set mismatch (start={start} chunk={chunk})"
        );
    }
}

/// `--memo-retain-from-step` only governs the cross-step memo cache (a
/// transposition cache for uniqueness verification), never correctness:
/// forcing the memo to be discarded every step (a threshold above the search
/// depth) must yield the identical result as the default. Uses max-step 11 so
/// the search crosses the default retention threshold (10), where the discard
/// vs. carry-forward policy actually differs.
#[test]
fn backward_search_memo_retain_from_step_is_exact() {
    let base: Vec<&str> = vec![
        "single-king-smoke",
        "ideal-backward",
        "--max-step",
        "11",
        "--parallel",
        "8",
        "--no-pawn",
        "--max-promoted-pct",
        "20",
        "--max-promoted-pct-after-step",
        "5",
        "--seed-result-log",
        "/dev/null",
        "--seed-sfen",
        "4k4/4+N4/9/9/9/4L4/9/9/9 w 2r2b4g4s3n3l18p 1",
        "--allowed-kinds",
        "pawn,lance,knight,silver,gold",
    ];

    let (default_header, mut default_urls) = {
        let (stdout, stderr) = run_fmrs(&base, Duration::from_secs(60));
        extract_best_result(&stdout, &stderr)
    };
    default_urls.sort();

    // A threshold above --max-step forces the memo to be discarded every step.
    let mut discard = base.clone();
    discard.extend_from_slice(&["--memo-retain-from-step", "999"]);
    let (discard_header, mut discard_urls) = {
        let (stdout, stderr) = run_fmrs(&discard, Duration::from_secs(60));
        extract_best_result(&stdout, &stderr)
    };
    discard_urls.sort();

    assert_eq!(
        default_header, discard_header,
        "--memo-retain-from-step changed the best header"
    );
    assert_eq!(
        default_urls, discard_urls,
        "--memo-retain-from-step changed the output set"
    );
}

/// The mid-ply uniqueness prune (on by default) must be frontier-preserving:
/// a non-unique intermediate (even) ply can't yield a unique output (odd) ply,
/// so dropping such mids early only removes candidates Phase 2 would reject
/// anyway. Output (best header + URL set) must be identical with and without
/// `--no-mid-uniqueness-prune`. Uses max-step 7 so the search exercises the
/// mid-ply prune path.
#[test]
fn backward_search_mid_uniqueness_prune_is_exact() {
    let base: Vec<&str> = vec![
        "single-king-smoke",
        "ideal-backward",
        "--max-step",
        "7",
        "--parallel",
        "8",
        "--no-pawn",
        "--max-promoted-pct",
        "34",
        "--max-promoted-pct-after-step",
        "4",
        "--seed-result-log",
        "/dev/null",
        "--seed-sfen",
        "4k4/4+N4/9/9/9/4L4/9/9/9 w 2r2b4g4s3n3l18p 1",
    ];

    // Default (mid-prune on).
    let (on_header, mut on_urls) = {
        let (stdout, stderr) = run_fmrs(&base, Duration::from_secs(60));
        extract_best_result(&stdout, &stderr)
    };
    on_urls.sort();

    // Disabled via --no-mid-uniqueness-prune.
    let mut off = base.clone();
    off.push("--no-mid-uniqueness-prune");
    let (off_header, mut off_urls) = {
        let (stdout, stderr) = run_fmrs(&off, Duration::from_secs(60));
        extract_best_result(&stdout, &stderr)
    };
    off_urls.sort();

    assert_eq!(
        on_header, off_header,
        "--no-mid-uniqueness-prune changed the best header"
    );
    assert_eq!(
        on_urls, off_urls,
        "--no-mid-uniqueness-prune changed the output set"
    );
    assert!(!on_urls.is_empty(), "expected a non-empty result");
}

/// Same seed without `--allowed-kinds` constraint, slightly different config.
/// Ensures the heuristics work with the default piece-kind set as well.
#[test]
fn backward_search_seed_default_kinds_max_step_5() {
    let (stdout, stderr) = run_fmrs(
        &[
            "single-king-smoke",
            "ideal-backward",
            "--max-step",
            "5",
            "--parallel",
            "8",
            "--no-pawn",
            "--max-promoted-pct",
            "34",
            "--max-promoted-pct-after-step",
            "4",
            "--seed-result-log",
            "/dev/null",
            "--seed-sfen",
            "4k4/4+N4/9/9/9/4L4/9/9/9 w 2r2b4g4s3n3l18p 1",
        ],
        Duration::from_secs(10),
    );

    let (header, sfens) = extract_best_result(&stdout, &stderr);
    assert_eq!(
        header, "best_pieces=5 best_steps=5: positions=584 succeeded_seeds=1",
        "unexpected header in stderr:\n{}",
        stderr
    );
    assert_eq!(sfens.len(), 584, "unexpected SFEN count");
}
