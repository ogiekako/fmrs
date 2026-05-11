use anyhow::Context as _;
use fmrs_core::{position::position::PositionAux, search::backward::BackwardSearchResumeState};
use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{BufRead, BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};

use super::smoke_constraints::SearchConstraints;

pub(super) const IDEAL_BACKWARD_SEED_LOG_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub(super) enum TerminationReason {
    /// 旧レコードや未設定。skipped 互換のため `from_skipped` で復元する。
    #[default]
    Unknown,
    /// `BackwardSearch::advance` が新しい局面を生成しなくなった (探索が自然終了)。
    Completed,
    /// `--max-step` (実効的には search_limit) に到達したため打ち切り。
    MaxStep,
    /// 旧 `--max-frontier` で frontier 上限を超え打ち切られた (機能廃止後は古い
    /// レコードを読む際にしか出現しない)。
    MaxFrontier,
    /// 別 seed が theoretical max piece count に到達したため大域停止。
    EarlyExit,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct SeedResultRecord {
    pub(super) version: u32,
    pub(super) max_step: Option<u16>,
    pub(super) max_frontier: Option<usize>,
    #[serde(default)]
    pub(super) constraints: SearchConstraints,
    pub(super) seed_index: usize,
    pub(super) seed_sfen: String,
    pub(super) best_step: u16,
    #[serde(default)]
    pub(super) best_piece_count: u32,
    pub(super) positions: usize,
    pub(super) representative_sfen: Option<String>,
    pub(super) skipped: bool,
    #[serde(default)]
    pub(super) peak_frontier_size: usize,
    #[serde(default)]
    pub(super) peak_memo_len: usize,
    #[serde(default)]
    pub(super) total_seen_positions: u64,
    #[serde(default)]
    pub(super) terminal_step: u16,
    #[serde(default)]
    pub(super) termination_reason: TerminationReason,
    #[serde(default)]
    pub(super) canonicalize_attacker_goldish: bool,
}

#[derive(Clone, Serialize, Deserialize)]
pub(super) struct SeedCheckpoint {
    pub(super) seed_index: usize,
    pub(super) seed_sfen: String,
    pub(super) max_step: Option<u16>,
    pub(super) max_frontier: Option<usize>,
    pub(super) constraints: SearchConstraints,
    pub(super) resume_state: BackwardSearchResumeState,
    pub(super) best_piece_count: u32,
    pub(super) best_sfens: Vec<String>,
    #[serde(default)]
    pub(super) canonicalize_attacker_goldish: bool,
    /// Binary-encoded frontier: 88 bytes per `Position`. Not serialized to JSON;
    /// populated when loading a `.ckpt` file and consumed when writing one.
    #[serde(skip)]
    pub(super) frontier_bytes: Vec<u8>,
    /// Binary-encoded best positions: 105 bytes per `PositionAux`. Not serialized
    /// to JSON; populated when loading a `.ckpt` file.
    #[serde(skip)]
    pub(super) best_position_bytes: Vec<u8>,
}

pub(super) fn condition_key(max_step: Option<u16>, constraints: SearchConstraints) -> String {
    use std::hash::{DefaultHasher, Hasher};
    // None を中央タプル要素として埋めることで、`max_frontier` フラグ廃止前の
    // None-run と同じ hash を再現する (古いチェックポイント・レコードとの互換)。
    let s = serde_json::to_string(&(max_step, Option::<usize>::None, &constraints))
        .expect("constraints serialize");
    let mut hasher = DefaultHasher::new();
    hasher.write(s.as_bytes());
    format!("{:016x}", hasher.finish())
}

pub(super) fn checkpoint_dir(seed_result_log: &Path) -> PathBuf {
    let mut dir = seed_result_log.as_os_str().to_owned();
    dir.push(".checkpoints");
    PathBuf::from(dir)
}

pub(super) fn checkpoint_path(
    seed_result_log: &Path,
    seed_index: usize,
    key: &str,
    canonicalize_attacker_goldish: bool,
) -> PathBuf {
    let suffix = canonical_path_suffix(canonicalize_attacker_goldish);
    checkpoint_dir(seed_result_log).join(format!("seed_{seed_index}_{key}{suffix}.json"))
}

fn canonical_path_suffix(canonicalize_attacker_goldish: bool) -> &'static str {
    if canonicalize_attacker_goldish {
        "_canon"
    } else {
        ""
    }
}

fn checkpoint_path_bin(
    seed_result_log: &Path,
    seed_index: usize,
    key: &str,
    canonicalize_attacker_goldish: bool,
) -> PathBuf {
    let suffix = canonical_path_suffix(canonicalize_attacker_goldish);
    checkpoint_dir(seed_result_log).join(format!("seed_{seed_index}_{key}{suffix}.ckpt"))
}

const CKPT_MAGIC: &[u8; 8] = b"FMRSCKPT";
const CKPT_VERSION: u32 = 1;

/// Write checkpoint as a binary `.ckpt` file.
///
/// Layout: magic(8) | version u32 LE | meta_json_len u32 LE | meta_json |
///         frontier_count u64 LE | frontier_bytes (88 B each) |
///         best_count u32 LE | best_bytes (105 B each)
///
/// `meta_json` is `SeedCheckpoint` serialised with empty `frontier_sfens` /
/// `best_sfens`; the actual positions are in the binary sections that follow.
fn write_seed_checkpoint_bin(path: &Path, checkpoint: &SeedCheckpoint) -> anyhow::Result<()> {
    let dir = path.parent().unwrap_or(Path::new("."));
    let tmp_path = dir.join(format!(
        ".{}.tmp",
        path.file_name().unwrap_or_default().to_string_lossy()
    ));

    let meta_json = serde_json::to_vec(checkpoint)?;
    let frontier_count = checkpoint.frontier_bytes.len() / 88;
    let best_count = checkpoint.best_position_bytes.len() / 105;

    let result = (|| -> anyhow::Result<()> {
        let mut f = BufWriter::new(fs::File::create(&tmp_path)?);
        f.write_all(CKPT_MAGIC)?;
        f.write_all(&(CKPT_VERSION).to_le_bytes())?;
        f.write_all(&(meta_json.len() as u32).to_le_bytes())?;
        f.write_all(&meta_json)?;
        f.write_all(&(frontier_count as u64).to_le_bytes())?;
        f.write_all(&checkpoint.frontier_bytes)?;
        f.write_all(&(best_count as u32).to_le_bytes())?;
        f.write_all(&checkpoint.best_position_bytes)?;
        Ok(())
    })();

    if result.is_err() {
        let _ = fs::remove_file(&tmp_path);
        return result;
    }
    fs::rename(&tmp_path, path)?;
    Ok(())
}

fn load_seed_checkpoint_bin(path: &Path) -> anyhow::Result<SeedCheckpoint> {
    let mut f = BufReader::new(fs::File::open(path)?);

    let mut magic = [0u8; 8];
    f.read_exact(&mut magic)?;
    anyhow::ensure!(&magic == CKPT_MAGIC, "bad magic in {}", path.display());

    let mut u32_buf = [0u8; 4];
    f.read_exact(&mut u32_buf)?;
    let version = u32::from_le_bytes(u32_buf);
    anyhow::ensure!(
        version == CKPT_VERSION,
        "unsupported .ckpt version {version}"
    );

    f.read_exact(&mut u32_buf)?;
    let meta_len = u32::from_le_bytes(u32_buf) as usize;
    let mut meta_json = vec![0u8; meta_len];
    f.read_exact(&mut meta_json)?;
    let mut cp: SeedCheckpoint = serde_json::from_slice(&meta_json)?;

    let mut u64_buf = [0u8; 8];
    f.read_exact(&mut u64_buf)?;
    let frontier_count = u64::from_le_bytes(u64_buf) as usize;
    let mut frontier_bytes = vec![0u8; frontier_count * 88];
    f.read_exact(&mut frontier_bytes)?;
    cp.frontier_bytes = frontier_bytes;

    f.read_exact(&mut u32_buf)?;
    let best_count = u32::from_le_bytes(u32_buf) as usize;
    let mut best_bytes = vec![0u8; best_count * 105];
    f.read_exact(&mut best_bytes)?;
    cp.best_position_bytes = best_bytes;

    Ok(cp)
}

pub(super) fn write_seed_checkpoint(
    seed_result_log: &Path,
    checkpoint: &SeedCheckpoint,
) -> anyhow::Result<()> {
    let dir = checkpoint_dir(seed_result_log);
    fs::create_dir_all(&dir)?;
    let key = condition_key(checkpoint.max_step, checkpoint.constraints);
    let path = checkpoint_path_bin(
        seed_result_log,
        checkpoint.seed_index,
        &key,
        checkpoint.canonicalize_attacker_goldish,
    );
    write_seed_checkpoint_bin(&path, checkpoint)
}

fn validate_checkpoint(
    cp: &SeedCheckpoint,
    seed_index: usize,
    seed_sfen: &str,
    canonicalize_attacker_goldish: bool,
) -> bool {
    cp.seed_index == seed_index
        && cp.seed_sfen == seed_sfen
        && cp.max_frontier.is_none()
        && cp.canonicalize_attacker_goldish == canonicalize_attacker_goldish
}

pub(super) fn load_seed_checkpoint(
    seed_result_log: &Path,
    seed_index: usize,
    seed_sfen: &str,
    max_step: Option<u16>,
    constraints: SearchConstraints,
    canonicalize_attacker_goldish: bool,
) -> Option<SeedCheckpoint> {
    let key = condition_key(max_step, constraints);

    // 1. Try binary .ckpt (current format).
    let bin_path = checkpoint_path_bin(
        seed_result_log,
        seed_index,
        &key,
        canonicalize_attacker_goldish,
    );
    if let Ok(cp) = load_seed_checkpoint_bin(&bin_path) {
        if validate_checkpoint(&cp, seed_index, seed_sfen, canonicalize_attacker_goldish) {
            return Some(cp);
        }
    }

    // 2. Try legacy JSON (keyed format: seed_N_KEY.json).
    let json_path = checkpoint_path(
        seed_result_log,
        seed_index,
        &key,
        canonicalize_attacker_goldish,
    );
    if let Ok(file) = fs::File::open(&json_path) {
        if let Ok(cp) = serde_json::from_reader::<_, SeedCheckpoint>(BufReader::new(file)) {
            if validate_checkpoint(&cp, seed_index, seed_sfen, canonicalize_attacker_goldish) {
                let _ = write_seed_checkpoint_bin(&bin_path, &cp);
                return Some(cp);
            }
        }
    }

    if canonicalize_attacker_goldish {
        return None;
    }

    // 3. Oldest legacy: seed_{i}.json without condition key.
    let legacy_path = checkpoint_dir(seed_result_log).join(format!("seed_{seed_index}.json"));
    let file = fs::File::open(&legacy_path).ok()?;
    let cp: SeedCheckpoint = serde_json::from_reader(BufReader::new(file)).ok()?;
    if cp.seed_index == seed_index
        && cp.seed_sfen == seed_sfen
        && cp.max_step == max_step
        && cp.max_frontier.is_none()
        && cp.constraints == constraints
        && !cp.canonicalize_attacker_goldish
    {
        let _ = write_seed_checkpoint_bin(&bin_path, &cp);
        Some(cp)
    } else {
        None
    }
}

pub(super) fn remove_seed_checkpoint(
    seed_result_log: &Path,
    seed_index: usize,
    max_step: Option<u16>,
    constraints: SearchConstraints,
    canonicalize_attacker_goldish: bool,
) {
    let key = condition_key(max_step, constraints);
    let _ = fs::remove_file(checkpoint_path_bin(
        seed_result_log,
        seed_index,
        &key,
        canonicalize_attacker_goldish,
    ));
    // Also remove any legacy JSON checkpoint that might still exist.
    let _ = fs::remove_file(checkpoint_path(
        seed_result_log,
        seed_index,
        &key,
        canonicalize_attacker_goldish,
    ));
}

pub(super) fn load_seed_result_log(
    path: &Path,
    max_step: Option<u16>,
    constraints: SearchConstraints,
    canonicalize_attacker_goldish: bool,
) -> anyhow::Result<FxHashMap<usize, SeedResultRecord>> {
    let Ok(file) = fs::File::open(path) else {
        return Ok(FxHashMap::default());
    };
    let mut records = FxHashMap::default();
    for (line_index, line) in BufReader::new(file).lines().enumerate() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let Ok(record) = serde_json::from_str::<SeedResultRecord>(&line) else {
            eprintln!(
                "warning: ignoring malformed seed result log line {} in {}",
                line_index + 1,
                path.display()
            );
            continue;
        };
        if record.version == IDEAL_BACKWARD_SEED_LOG_VERSION
            && record.max_step == max_step
            && record.max_frontier.is_none()
            && record.constraints == constraints
            && record.canonicalize_attacker_goldish == canonicalize_attacker_goldish
        {
            records.insert(record.seed_index, record);
        }
    }
    Ok(records)
}

pub(super) fn trajectory_log_path(seed_result_log: &Path) -> PathBuf {
    if seed_result_log == Path::new("/dev/null") {
        return seed_result_log.to_owned();
    }
    let mut p = seed_result_log.as_os_str().to_owned();
    p.push(".trajectory.jsonl");
    PathBuf::from(p)
}

pub(super) fn open_seed_result_log(path: &Path) -> anyhow::Result<fs::File> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .with_context(|| format!("failed to open {}", path.display()))
}

pub(super) fn append_seed_result_record(
    file: &mut fs::File,
    record: SeedResultRecord,
) -> anyhow::Result<()> {
    let mut buf = BufWriter::new(&mut *file);
    serde_json::to_writer(&mut buf, &record)?;
    writeln!(buf)?;
    buf.flush()?;
    Ok(())
}

#[derive(Debug, Clone, Copy)]
pub(super) struct SeedRunStats {
    pub(super) peak_frontier_size: usize,
    pub(super) peak_memo_len: usize,
    pub(super) total_seen_positions: u64,
    pub(super) terminal_step: u16,
    pub(super) termination_reason: TerminationReason,
}

pub(super) fn build_seed_result_record(
    seed_index: usize,
    seed: &PositionAux,
    max_step: Option<u16>,
    constraints: SearchConstraints,
    best: &Option<(u32, Vec<PositionAux>)>,
    stats: SeedRunStats,
    canonicalize_attacker_goldish: bool,
) -> SeedResultRecord {
    let (best_piece_count, positions, representative_sfen) =
        if let Some((piece_count, positions)) = best.as_ref() {
            let mut sfens = positions.iter().map(PositionAux::sfen).collect::<Vec<_>>();
            sfens.sort();
            (*piece_count, sfens.len(), sfens.into_iter().next())
        } else {
            (0, 0, None)
        };
    SeedResultRecord {
        version: IDEAL_BACKWARD_SEED_LOG_VERSION,
        max_step,
        max_frontier: None,
        constraints,
        seed_index,
        seed_sfen: seed.sfen(),
        best_step: 0, // deprecated, kept for compat
        best_piece_count,
        positions,
        representative_sfen,
        skipped: false,
        peak_frontier_size: stats.peak_frontier_size,
        peak_memo_len: stats.peak_memo_len,
        total_seen_positions: stats.total_seen_positions,
        terminal_step: stats.terminal_step,
        termination_reason: stats.termination_reason,
        canonicalize_attacker_goldish,
    }
}

pub(super) fn merge_seed_result_record(
    best: &mut (u32, rustc_hash::FxHashSet<String>, usize),
    record: &SeedResultRecord,
) {
    let Some(sfen) = record.representative_sfen.as_ref() else {
        return;
    };
    // Fallback for old records that only have best_step
    let piece_count = if record.best_piece_count > 0 {
        record.best_piece_count
    } else {
        record.best_step as u32 / 2 + 3
    };
    best.2 += 1;
    if piece_count > best.0 {
        best.0 = piece_count;
        best.1.clear();
    }
    if piece_count == best.0 {
        best.1.insert(sfen.clone());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fmrs_core::search::backward::BackwardSearchResumeState;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEST_DIR_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn unique_test_dir(tag: &str) -> PathBuf {
        let id = TEST_DIR_COUNTER.fetch_add(1, Ordering::Relaxed);
        let pid = std::process::id();
        let mut p = std::env::temp_dir();
        p.push(format!("fmrs-checkpoint-test-{tag}-{pid}-{id}"));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(&p).unwrap();
        p
    }

    fn dummy_checkpoint(
        seed_index: usize,
        seed_sfen: &str,
        max_step: Option<u16>,
        constraints: SearchConstraints,
        marker: u32,
    ) -> SeedCheckpoint {
        dummy_checkpoint_with_canonical(seed_index, seed_sfen, max_step, constraints, marker, false)
    }

    fn dummy_checkpoint_with_canonical(
        seed_index: usize,
        seed_sfen: &str,
        max_step: Option<u16>,
        constraints: SearchConstraints,
        marker: u32,
        canonicalize_attacker_goldish: bool,
    ) -> SeedCheckpoint {
        SeedCheckpoint {
            seed_index,
            seed_sfen: seed_sfen.to_string(),
            max_step,
            max_frontier: None,
            constraints,
            resume_state: BackwardSearchResumeState {
                initial_position_sfen: "test-init".to_string(),
                remaining_solution_moves: vec![],
                frontier_sfens: vec![],
                step: 0,
                one_way: true,
                no_black_goldish: false,
            },
            best_piece_count: marker,
            best_sfens: vec![],
            canonicalize_attacker_goldish,
            frontier_bytes: vec![],
            best_position_bytes: vec![],
        }
    }

    #[test]
    fn condition_key_is_deterministic_and_distinct() {
        let a = SearchConstraints::default();
        let b = SearchConstraints {
            no_pawn: true,
            ..SearchConstraints::default()
        };
        assert_eq!(condition_key(None, a), condition_key(None, a));
        assert_ne!(condition_key(None, a), condition_key(None, b));
        assert_ne!(condition_key(None, a), condition_key(Some(7), a));
    }

    #[test]
    fn checkpoint_roundtrips_with_matching_conditions() {
        let dir = unique_test_dir("roundtrip");
        let log = dir.join("log.jsonl");
        let constraints = SearchConstraints::default();
        let cp = dummy_checkpoint(7, "sfen-x", Some(5), constraints, 42);
        write_seed_checkpoint(&log, &cp).unwrap();

        let loaded = load_seed_checkpoint(&log, 7, "sfen-x", Some(5), constraints, false);
        assert!(loaded.is_some());
        assert_eq!(loaded.unwrap().best_piece_count, 42);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn checkpoint_with_different_conditions_is_isolated() {
        let dir = unique_test_dir("isolated");
        let log = dir.join("log.jsonl");
        let a = SearchConstraints::default();
        let b = SearchConstraints {
            no_pawn: true,
            ..SearchConstraints::default()
        };

        write_seed_checkpoint(&log, &dummy_checkpoint(7, "sfen-x", None, a, 1)).unwrap();
        write_seed_checkpoint(&log, &dummy_checkpoint(7, "sfen-x", None, b, 2)).unwrap();

        let la = load_seed_checkpoint(&log, 7, "sfen-x", None, a, false).unwrap();
        let lb = load_seed_checkpoint(&log, 7, "sfen-x", None, b, false).unwrap();
        assert_eq!(la.best_piece_count, 1);
        assert_eq!(lb.best_piece_count, 2);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn legacy_checkpoint_migrates_when_conditions_match() {
        let dir = unique_test_dir("migrate");
        let log = dir.join("log.jsonl");
        let cp_dir = checkpoint_dir(&log);
        fs::create_dir_all(&cp_dir).unwrap();

        let constraints = SearchConstraints::default();
        let cp = dummy_checkpoint(7, "sfen-x", None, constraints, 99);
        let legacy_path = cp_dir.join("seed_7.json");
        serde_json::to_writer(fs::File::create(&legacy_path).unwrap(), &cp).unwrap();

        let loaded = load_seed_checkpoint(&log, 7, "sfen-x", None, constraints, false);
        assert!(loaded.is_some());
        assert_eq!(loaded.unwrap().best_piece_count, 99);

        let key = condition_key(None, constraints);
        let ckpt_path = cp_dir.join(format!("seed_7_{key}.ckpt"));
        assert!(
            ckpt_path.exists(),
            "binary .ckpt file should exist after migrate"
        );
        assert!(
            legacy_path.exists(),
            "legacy JSON file should be kept after migrate"
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn legacy_checkpoint_skipped_and_kept_when_conditions_mismatch() {
        let dir = unique_test_dir("mismatch");
        let log = dir.join("log.jsonl");
        let cp_dir = checkpoint_dir(&log);
        fs::create_dir_all(&cp_dir).unwrap();

        let a = SearchConstraints::default();
        let b = SearchConstraints {
            no_pawn: true,
            ..SearchConstraints::default()
        };
        let cp = dummy_checkpoint(7, "sfen-x", None, a, 99);
        let legacy_path = cp_dir.join("seed_7.json");
        serde_json::to_writer(fs::File::create(&legacy_path).unwrap(), &cp).unwrap();

        // Loading with mismatched conditions should not migrate or use it.
        let loaded = load_seed_checkpoint(&log, 7, "sfen-x", None, b, false);
        assert!(loaded.is_none());
        assert!(
            legacy_path.exists(),
            "legacy file must remain for future runs"
        );

        // Subsequent load with matching conditions still picks it up.
        let loaded = load_seed_checkpoint(&log, 7, "sfen-x", None, a, false);
        assert!(loaded.is_some());
        assert!(
            legacy_path.exists(),
            "legacy JSON file should remain after migration"
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn canonical_and_regular_checkpoints_are_isolated() {
        let dir = unique_test_dir("canon-isolated");
        let log = dir.join("log.jsonl");
        let constraints = SearchConstraints::default();

        write_seed_checkpoint(
            &log,
            &dummy_checkpoint_with_canonical(7, "sfen-x", None, constraints, 11, false),
        )
        .unwrap();
        write_seed_checkpoint(
            &log,
            &dummy_checkpoint_with_canonical(7, "sfen-x", None, constraints, 22, true),
        )
        .unwrap();

        let off = load_seed_checkpoint(&log, 7, "sfen-x", None, constraints, false).unwrap();
        let on = load_seed_checkpoint(&log, 7, "sfen-x", None, constraints, true).unwrap();
        assert_eq!(off.best_piece_count, 11);
        assert_eq!(on.best_piece_count, 22);
        assert!(!off.canonicalize_attacker_goldish);
        assert!(on.canonicalize_attacker_goldish);

        // Files live at distinct paths so removal is mode-specific.
        remove_seed_checkpoint(&log, 7, None, constraints, true);
        assert!(load_seed_checkpoint(&log, 7, "sfen-x", None, constraints, true).is_none());
        assert!(load_seed_checkpoint(&log, 7, "sfen-x", None, constraints, false).is_some());

        let _ = fs::remove_dir_all(&dir);
    }
}
