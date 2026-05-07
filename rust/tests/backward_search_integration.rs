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
/// and all SFENs from stdout (sorted, one per line). Format:
///   stderr: ... best_pieces=N: positions=M succeeded_seeds=K
///   stdout: <SFEN_1>\n<SFEN_2>\n... (sorted)
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
/// that runs in well under a second.
#[test]
fn backward_search_seed_max_step_5() {
    let (stdout, stderr) = run_fmrs(
        &[
            "single-king-smoke",
            "ideal-backward",
            "--max-step",
            "5",
            "--inner-parallel",
            "8",
            "--parallel",
            "1",
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
        header, "best_pieces=5: positions=36 succeeded_seeds=1",
        "unexpected header in stderr:\n{}",
        stderr
    );
    assert_eq!(sfens.len(), 36, "unexpected SFEN count");
    // Output is sorted. Lock down first/last as a deterministic correctness check.
    assert_eq!(
        sfens[0], "2k1G4/4G4/9/9/3N5/4L4/9/9/9 b 2r2b2g4s3n3l18p 1",
        "unexpected first sorted SFEN"
    );
    assert_eq!(
        sfens[sfens.len() - 1],
        "4GG3/9/5k3/9/5N3/4L4/9/9/9 b 2r2b2g4s3n3l18p 1",
        "unexpected last sorted SFEN"
    );
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
            "--inner-parallel",
            "8",
            "--parallel",
            "1",
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
        header, "best_pieces=5: positions=584 succeeded_seeds=1",
        "unexpected header in stderr:\n{}",
        stderr
    );
    assert_eq!(sfens.len(), 584, "unexpected SFEN count");
}
