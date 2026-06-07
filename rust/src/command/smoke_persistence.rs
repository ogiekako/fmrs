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
    /// Output step at which the current best positions were found. Persisted so
    /// that a resumed search keeps the `(best_piece_count, best_step)`
    /// lexicographic ordering instead of resetting `best_step` to 0.
    #[serde(default)]
    pub(super) best_step: u16,
    pub(super) best_sfens: Vec<String>,
    #[serde(default)]
    pub(super) canonicalize_attacker_goldish: bool,
    /// Beam-config fingerprint when this checkpoint was written by a beam run
    /// (`None` for exact runs). Namespaces the checkpoint file so a beam run
    /// only resumes from an identically-configured beam checkpoint and never
    /// shares the exact-run checkpoint path. See `BeamConfig::checkpoint_key`.
    #[serde(default)]
    pub(super) beam_key: Option<String>,
    /// Persisted Bottom-K pool adaptation state. On resume these let the
    /// search start at the same `pool_factor` it had reached previously,
    /// avoiding a default-value cold start that would shrink |next| for
    /// several steps until the EMA caught up. Both Option for backward
    /// compatibility with checkpoints written before adaptation existed.
    #[serde(default)]
    pub(super) adaptive_pool_factor: Option<usize>,
    #[serde(default)]
    pub(super) ema_inv_survival: Option<f64>,
    /// Binary-encoded frontier: 96 bytes per `Position`. Not serialized to JSON;
    /// populated when loading a `.ckpt` file and consumed when writing one.
    #[serde(skip)]
    pub(super) frontier_bytes: Vec<u8>,
    /// Binary-encoded best positions: 113 bytes per `PositionAux`. Not serialized
    /// to JSON; populated when loading a `.ckpt` file.
    #[serde(skip)]
    pub(super) best_position_bytes: Vec<u8>,
}

/// Chunk-granularity progress marker for split mode (`--split-start-step`).
///
/// Records how many of the deterministically-ordered frontier chunks have been
/// fully processed plus the running best across the prefix and completed chunks.
/// On resume the prefix BFS is re-run to regenerate the split frontier (cheap
/// relative to the chunks), re-shuffled with the same `split_seed`, and chunks
/// `< next_chunk` are skipped. Small (a handful of fields + the deduped best),
/// unlike the per-chunk frontier which is the thing we are bounding.
#[derive(Clone, Serialize, Deserialize)]
pub(super) struct SplitProgress {
    pub(super) seed_index: usize,
    pub(super) seed_sfen: String,
    pub(super) max_step: Option<u16>,
    #[serde(default)]
    pub(super) constraints: SearchConstraints,
    #[serde(default)]
    pub(super) canonicalize_attacker_goldish: bool,
    /// Split parameters this marker was produced under. A mismatch against the
    /// current CLI flags invalidates the marker (the chunk boundaries would
    /// differ), so it is ignored and the split restarts.
    pub(super) split_start_step: u16,
    pub(super) split_chunk_size: usize,
    pub(super) split_seed: u64,
    pub(super) num_chunks: usize,
    /// Index of the next chunk to process. `== num_chunks` means all chunks are
    /// done (the marker is removed on full completion, so this is transient).
    pub(super) next_chunk: usize,
    pub(super) best_piece_count: u32,
    pub(super) best_step: u16,
    /// Binary-encoded accumulated best positions: 113 bytes per `PositionAux`.
    /// Not serialized to JSON; carried in the binary tail of the `.split` file.
    #[serde(skip)]
    pub(super) best_position_bytes: Vec<u8>,
}

const SPLIT_MAGIC: &[u8; 8] = b"FMRSSPLT";
const SPLIT_VERSION: u32 = 1;

fn split_progress_path(
    seed_result_log: &Path,
    seed_index: usize,
    key: &str,
    canonicalize_attacker_goldish: bool,
) -> PathBuf {
    let suffix = canonical_path_suffix(canonicalize_attacker_goldish);
    checkpoint_dir(seed_result_log).join(format!("seed_{seed_index}_{key}{suffix}.split"))
}

/// Persist a `SplitProgress` marker (binary: magic | version | meta_json |
/// best section), atomically via tmp-rename. Layout mirrors `.ckpt`.
pub(super) fn write_split_progress(
    seed_result_log: &Path,
    progress: &SplitProgress,
) -> anyhow::Result<()> {
    let dir = checkpoint_dir(seed_result_log);
    fs::create_dir_all(&dir)?;
    let key = condition_key(progress.max_step, progress.constraints);
    let path = split_progress_path(
        seed_result_log,
        progress.seed_index,
        &key,
        progress.canonicalize_attacker_goldish,
    );
    let tmp_path = dir.join(format!(
        ".{}.tmp",
        path.file_name().unwrap_or_default().to_string_lossy()
    ));
    let meta_json = serde_json::to_vec(progress)?;
    let best_count = progress.best_position_bytes.len() / 113;
    let result = (|| -> anyhow::Result<()> {
        let mut f = BufWriter::new(fs::File::create(&tmp_path)?);
        f.write_all(SPLIT_MAGIC)?;
        f.write_all(&SPLIT_VERSION.to_le_bytes())?;
        f.write_all(&(meta_json.len() as u32).to_le_bytes())?;
        f.write_all(&meta_json)?;
        f.write_all(&(best_count as u32).to_le_bytes())?;
        f.write_all(&progress.best_position_bytes)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp_path);
        return result;
    }
    fs::rename(&tmp_path, &path)?;
    Ok(())
}

/// Load a `SplitProgress` marker, validating it matches the seed identity and
/// canonicalize flag. Split-parameter validation (start step / chunk size /
/// seed) is left to the caller, which knows the current CLI flags. Returns
/// `None` if absent, unreadable, or mismatched.
pub(super) fn load_split_progress(
    seed_result_log: &Path,
    seed_index: usize,
    seed_sfen: &str,
    max_step: Option<u16>,
    constraints: SearchConstraints,
    canonicalize_attacker_goldish: bool,
) -> Option<SplitProgress> {
    let key = condition_key(max_step, constraints);
    let path = split_progress_path(seed_result_log, seed_index, &key, canonicalize_attacker_goldish);
    let mut f = BufReader::new(fs::File::open(&path).ok()?);
    let mut magic = [0u8; 8];
    f.read_exact(&mut magic).ok()?;
    if &magic != SPLIT_MAGIC {
        return None;
    }
    let mut u32_buf = [0u8; 4];
    f.read_exact(&mut u32_buf).ok()?;
    if u32::from_le_bytes(u32_buf) != SPLIT_VERSION {
        return None;
    }
    f.read_exact(&mut u32_buf).ok()?;
    let meta_len = u32::from_le_bytes(u32_buf) as usize;
    let mut meta_json = vec![0u8; meta_len];
    f.read_exact(&mut meta_json).ok()?;
    let mut progress: SplitProgress = serde_json::from_slice(&meta_json).ok()?;
    f.read_exact(&mut u32_buf).ok()?;
    let best_count = u32::from_le_bytes(u32_buf) as usize;
    let mut best_bytes = vec![0u8; best_count * 113];
    f.read_exact(&mut best_bytes).ok()?;
    progress.best_position_bytes = best_bytes;
    if progress.seed_index == seed_index
        && progress.seed_sfen == seed_sfen
        && progress.canonicalize_attacker_goldish == canonicalize_attacker_goldish
    {
        Some(progress)
    } else {
        None
    }
}

pub(super) fn remove_split_progress(
    seed_result_log: &Path,
    seed_index: usize,
    max_step: Option<u16>,
    constraints: SearchConstraints,
    canonicalize_attacker_goldish: bool,
) {
    let key = condition_key(max_step, constraints);
    let _ = fs::remove_file(split_progress_path(
        seed_result_log,
        seed_index,
        &key,
        canonicalize_attacker_goldish,
    ));
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
    beam_key: Option<&str>,
) -> PathBuf {
    let suffix = canonical_path_suffix(canonicalize_attacker_goldish);
    let beam = beam_segment(beam_key);
    checkpoint_dir(seed_result_log).join(format!("seed_{seed_index}_{key}{suffix}{beam}.json"))
}

fn canonical_path_suffix(canonicalize_attacker_goldish: bool) -> &'static str {
    if canonicalize_attacker_goldish {
        "_canon"
    } else {
        ""
    }
}

/// Filename segment namespacing beam checkpoints. Empty for exact runs, so the
/// exact checkpoint path is byte-for-byte unchanged.
fn beam_segment(beam_key: Option<&str>) -> String {
    match beam_key {
        Some(k) => format!("_beam{k}"),
        None => String::new(),
    }
}

fn checkpoint_path_bin(
    seed_result_log: &Path,
    seed_index: usize,
    key: &str,
    canonicalize_attacker_goldish: bool,
    beam_key: Option<&str>,
) -> PathBuf {
    let suffix = canonical_path_suffix(canonicalize_attacker_goldish);
    let beam = beam_segment(beam_key);
    checkpoint_dir(seed_result_log).join(format!("seed_{seed_index}_{key}{suffix}{beam}.ckpt"))
}

const CKPT_MAGIC: &[u8; 8] = b"FMRSCKPT";
const CKPT_VERSION: u32 = 1;

/// Write checkpoint as a binary `.ckpt` file.
///
/// Layout: magic(8) | version u32 LE | meta_json_len u32 LE | meta_json |
///         frontier_count u64 LE | frontier_bytes (96 B each) |
///         best_count u32 LE | best_bytes (113 B each)
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
    let frontier_count = checkpoint.frontier_bytes.len() / 96;
    let best_count = checkpoint.best_position_bytes.len() / 113;

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
    let mut frontier_bytes = vec![0u8; frontier_count * 96];
    f.read_exact(&mut frontier_bytes)?;
    cp.frontier_bytes = frontier_bytes;

    f.read_exact(&mut u32_buf)?;
    let best_count = u32::from_le_bytes(u32_buf) as usize;
    let mut best_bytes = vec![0u8; best_count * 113];
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
        checkpoint.beam_key.as_deref(),
    );
    write_seed_checkpoint_bin(&path, checkpoint)
}

fn validate_checkpoint(
    cp: &SeedCheckpoint,
    seed_index: usize,
    seed_sfen: &str,
    canonicalize_attacker_goldish: bool,
    beam_key: Option<&str>,
) -> bool {
    cp.seed_index == seed_index
        && cp.seed_sfen == seed_sfen
        && cp.max_frontier.is_none()
        && cp.canonicalize_attacker_goldish == canonicalize_attacker_goldish
        && cp.beam_key.as_deref() == beam_key
}

pub(super) fn load_seed_checkpoint(
    seed_result_log: &Path,
    seed_index: usize,
    seed_sfen: &str,
    max_step: Option<u16>,
    constraints: SearchConstraints,
    canonicalize_attacker_goldish: bool,
    beam_key: Option<&str>,
) -> Option<SeedCheckpoint> {
    let key = condition_key(max_step, constraints);

    // 1. Try binary .ckpt (current format).
    let bin_path = checkpoint_path_bin(
        seed_result_log,
        seed_index,
        &key,
        canonicalize_attacker_goldish,
        beam_key,
    );
    if let Ok(cp) = load_seed_checkpoint_bin(&bin_path) {
        if validate_checkpoint(&cp, seed_index, seed_sfen, canonicalize_attacker_goldish, beam_key)
        {
            return Some(cp);
        }
    }

    // Beam checkpoints are a current-format-only feature; no legacy JSON exists
    // for them, so skip the backward-compat fallbacks below.
    if beam_key.is_some() {
        return None;
    }

    // 2. Try legacy JSON (keyed format: seed_N_KEY.json).
    let json_path = checkpoint_path(
        seed_result_log,
        seed_index,
        &key,
        canonicalize_attacker_goldish,
        beam_key,
    );
    if let Ok(file) = fs::File::open(&json_path) {
        if let Ok(cp) = serde_json::from_reader::<_, SeedCheckpoint>(BufReader::new(file)) {
            if validate_checkpoint(&cp, seed_index, seed_sfen, canonicalize_attacker_goldish, beam_key)
            {
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
    beam_key: Option<&str>,
) {
    let key = condition_key(max_step, constraints);
    let _ = fs::remove_file(checkpoint_path_bin(
        seed_result_log,
        seed_index,
        &key,
        canonicalize_attacker_goldish,
        beam_key,
    ));
    // Also remove any legacy JSON checkpoint that might still exist.
    let _ = fs::remove_file(checkpoint_path(
        seed_result_log,
        seed_index,
        &key,
        canonicalize_attacker_goldish,
        beam_key,
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
    best: &Option<(u32, u16, Vec<PositionAux>)>,
    stats: SeedRunStats,
    canonicalize_attacker_goldish: bool,
) -> SeedResultRecord {
    let (best_piece_count, best_step, positions, representative_sfen) =
        if let Some((piece_count, step, positions)) = best.as_ref() {
            let mut sfens = positions.iter().map(PositionAux::sfen).collect::<Vec<_>>();
            sfens.sort();
            (*piece_count, *step, sfens.len(), sfens.into_iter().next())
        } else {
            (0, 0, 0, None)
        };
    SeedResultRecord {
        version: IDEAL_BACKWARD_SEED_LOG_VERSION,
        max_step,
        max_frontier: None,
        constraints,
        seed_index,
        seed_sfen: seed.sfen(),
        best_step,
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

/// Cross-seed best: `(best_piece_count, best_step, sfens, succeeded_seeds)`.
/// Ranked by the `(piece_count, step)` lexicographic order so the reported
/// best is the lexicographic maximum across every seed.
pub(super) type CrossSeedBest = (u32, u16, rustc_hash::FxHashSet<String>, usize);

/// Merge a single seed's `(piece_count, step)` candidate plus its SFENs into
/// the running cross-seed best. A strictly larger `(piece_count, step)` tuple
/// resets the accumulated SFEN set; a tie unions into it.
pub(super) fn merge_best_candidate(
    best: &mut CrossSeedBest,
    piece_count: u32,
    step: u16,
    sfens: impl IntoIterator<Item = String>,
) {
    best.3 += 1;
    if (piece_count, step) > (best.0, best.1) {
        best.0 = piece_count;
        best.1 = step;
        best.2.clear();
    }
    if (piece_count, step) == (best.0, best.1) {
        best.2.extend(sfens);
    }
}

pub(super) fn merge_seed_result_record(best: &mut CrossSeedBest, record: &SeedResultRecord) {
    let Some(sfen) = record.representative_sfen.as_ref() else {
        return;
    };
    // Fallback for old records that only have best_step. Legacy records stored
    // the piece count implicitly via best_step; their step ordering is not
    // meaningful, so treat the step component as 0.
    let (piece_count, step) = if record.best_piece_count > 0 {
        (record.best_piece_count, record.best_step)
    } else {
        (record.best_step as u32 / 2 + 3, 0)
    };
    merge_best_candidate(best, piece_count, step, std::iter::once(sfen.clone()));
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
        dummy_checkpoint_full(
            seed_index,
            seed_sfen,
            max_step,
            constraints,
            marker,
            canonicalize_attacker_goldish,
            None,
        )
    }

    fn dummy_checkpoint_full(
        seed_index: usize,
        seed_sfen: &str,
        max_step: Option<u16>,
        constraints: SearchConstraints,
        marker: u32,
        canonicalize_attacker_goldish: bool,
        beam_key: Option<String>,
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
            best_step: 0,
            best_sfens: vec![],
            canonicalize_attacker_goldish,
            beam_key,
            adaptive_pool_factor: None,
            ema_inv_survival: None,
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

        let loaded = load_seed_checkpoint(&log, 7, "sfen-x", Some(5), constraints, false, None);
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

        let la = load_seed_checkpoint(&log, 7, "sfen-x", None, a, false, None).unwrap();
        let lb = load_seed_checkpoint(&log, 7, "sfen-x", None, b, false, None).unwrap();
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

        let loaded = load_seed_checkpoint(&log, 7, "sfen-x", None, constraints, false, None);
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
        let loaded = load_seed_checkpoint(&log, 7, "sfen-x", None, b, false, None);
        assert!(loaded.is_none());
        assert!(
            legacy_path.exists(),
            "legacy file must remain for future runs"
        );

        // Subsequent load with matching conditions still picks it up.
        let loaded = load_seed_checkpoint(&log, 7, "sfen-x", None, a, false, None);
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

        let off = load_seed_checkpoint(&log, 7, "sfen-x", None, constraints, false, None).unwrap();
        let on = load_seed_checkpoint(&log, 7, "sfen-x", None, constraints, true, None).unwrap();
        assert_eq!(off.best_piece_count, 11);
        assert_eq!(on.best_piece_count, 22);
        assert!(!off.canonicalize_attacker_goldish);
        assert!(on.canonicalize_attacker_goldish);

        // Files live at distinct paths so removal is mode-specific.
        remove_seed_checkpoint(&log, 7, None, constraints, true, None);
        assert!(load_seed_checkpoint(&log, 7, "sfen-x", None, constraints, true, None).is_none());
        assert!(load_seed_checkpoint(&log, 7, "sfen-x", None, constraints, false, None).is_some());

        let _ = fs::remove_dir_all(&dir);
    }

    /// Regression: the exact (non-beam) checkpoint path and round-trip must be
    /// byte-for-byte unchanged by the beam-namespace feature. An exact run
    /// writes with `beam_key=None`; the produced filename has no `_beam` segment
    /// and is loadable with `beam_key=None`.
    #[test]
    fn exact_checkpoint_path_unchanged_by_beam_feature() {
        let dir = unique_test_dir("exact-path");
        let log = dir.join("log.jsonl");
        let constraints = SearchConstraints::default();
        let cp_dir = checkpoint_dir(&log);

        let cp = dummy_checkpoint(7, "sfen-x", Some(10), constraints, 42);
        write_seed_checkpoint(&log, &cp).unwrap();

        // The exact path is exactly seed_{i}_{key}.ckpt (no _beam segment).
        let key = condition_key(Some(10), constraints);
        let exact_path = cp_dir.join(format!("seed_7_{key}.ckpt"));
        assert!(exact_path.exists(), "exact .ckpt path must be unchanged");
        // And nothing with a _beam segment was written.
        let beam_like: Vec<_> = fs::read_dir(&cp_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().contains("_beam"))
            .collect();
        assert!(beam_like.is_empty(), "no beam file for an exact run");

        let loaded = load_seed_checkpoint(&log, 7, "sfen-x", Some(10), constraints, false, None);
        assert_eq!(loaded.unwrap().best_piece_count, 42);

        let _ = fs::remove_dir_all(&dir);
    }

    /// A beam checkpoint round-trips only via its own beam key, and lands at a
    /// distinct path from the exact checkpoint.
    #[test]
    fn beam_checkpoint_roundtrips_under_its_key() {
        let dir = unique_test_dir("beam-roundtrip");
        let log = dir.join("log.jsonl");
        let constraints = SearchConstraints::default();
        let cp_dir = checkpoint_dir(&log);

        let key = "deadbeefcafef00d".to_string();
        let cp = dummy_checkpoint_full(
            7,
            "sfen-x",
            Some(10),
            constraints,
            55,
            false,
            Some(key.clone()),
        );
        write_seed_checkpoint(&log, &cp).unwrap();

        // Lands at the _beam{key}-suffixed path.
        let cond = condition_key(Some(10), constraints);
        let beam_path = cp_dir.join(format!("seed_7_{cond}_beam{key}.ckpt"));
        assert!(beam_path.exists(), "beam .ckpt must use the _beam path");

        // Loadable with the matching beam key.
        let loaded =
            load_seed_checkpoint(&log, 7, "sfen-x", Some(10), constraints, false, Some(&key));
        assert_eq!(loaded.unwrap().best_piece_count, 55);

        let _ = fs::remove_dir_all(&dir);
    }

    /// A beam run never reads the exact checkpoint and vice-versa: the two live
    /// at distinct paths and `load` only returns a match when the beam key
    /// agrees. Different beam configs (keys) are likewise isolated.
    #[test]
    fn beam_and_exact_checkpoints_are_isolated() {
        let dir = unique_test_dir("beam-isolated");
        let log = dir.join("log.jsonl");
        let constraints = SearchConstraints::default();

        // Exact checkpoint + two distinct beam-config checkpoints, same seed.
        write_seed_checkpoint(&log, &dummy_checkpoint(7, "sfen-x", None, constraints, 1)).unwrap();
        write_seed_checkpoint(
            &log,
            &dummy_checkpoint_full(7, "sfen-x", None, constraints, 2, false, Some("aaaa".into())),
        )
        .unwrap();
        write_seed_checkpoint(
            &log,
            &dummy_checkpoint_full(7, "sfen-x", None, constraints, 3, false, Some("bbbb".into())),
        )
        .unwrap();

        // Each key resolves to its own checkpoint; none bleeds into another.
        let exact = load_seed_checkpoint(&log, 7, "sfen-x", None, constraints, false, None).unwrap();
        let beam_a =
            load_seed_checkpoint(&log, 7, "sfen-x", None, constraints, false, Some("aaaa")).unwrap();
        let beam_b =
            load_seed_checkpoint(&log, 7, "sfen-x", None, constraints, false, Some("bbbb")).unwrap();
        assert_eq!(exact.best_piece_count, 1);
        assert_eq!(beam_a.best_piece_count, 2);
        assert_eq!(beam_b.best_piece_count, 3);

        // An unknown beam key matches nothing (no fallback to exact).
        assert!(load_seed_checkpoint(&log, 7, "sfen-x", None, constraints, false, Some("cccc"))
            .is_none());

        // Removing one beam checkpoint leaves the exact and the other beam one.
        remove_seed_checkpoint(&log, 7, None, constraints, false, Some("aaaa"));
        assert!(load_seed_checkpoint(&log, 7, "sfen-x", None, constraints, false, Some("aaaa"))
            .is_none());
        assert!(load_seed_checkpoint(&log, 7, "sfen-x", None, constraints, false, None).is_some());
        assert!(load_seed_checkpoint(&log, 7, "sfen-x", None, constraints, false, Some("bbbb"))
            .is_some());

        let _ = fs::remove_dir_all(&dir);
    }
}
