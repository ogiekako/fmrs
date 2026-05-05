use anyhow::Context as _;
use fmrs_core::{
    position::position::PositionAux,
    search::backward::BackwardSearchResumeState,
};
use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use super::smoke_constraints::SearchConstraints;

pub(super) const IDEAL_BACKWARD_SEED_LOG_VERSION: u32 = 1;

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
}

pub(super) fn condition_key(
    max_step: Option<u16>,
    max_frontier: Option<usize>,
    constraints: SearchConstraints,
) -> String {
    use std::hash::{DefaultHasher, Hasher};
    let s = serde_json::to_string(&(max_step, max_frontier, &constraints))
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
) -> PathBuf {
    checkpoint_dir(seed_result_log).join(format!("seed_{seed_index}_{key}.json"))
}

pub(super) fn write_seed_checkpoint(
    seed_result_log: &Path,
    checkpoint: &SeedCheckpoint,
) -> anyhow::Result<()> {
    let dir = checkpoint_dir(seed_result_log);
    fs::create_dir_all(&dir)?;
    let key =
        condition_key(checkpoint.max_step, checkpoint.max_frontier, checkpoint.constraints);
    let path = checkpoint_path(seed_result_log, checkpoint.seed_index, &key);
    let tmp_path = dir.join(format!(
        ".seed_{}_{}.json.tmp",
        checkpoint.seed_index, key
    ));
    serde_json::to_writer(fs::File::create(&tmp_path)?, checkpoint)?;
    fs::rename(&tmp_path, &path)?;
    Ok(())
}

pub(super) fn load_seed_checkpoint(
    seed_result_log: &Path,
    seed_index: usize,
    seed_sfen: &str,
    max_step: Option<u16>,
    max_frontier: Option<usize>,
    constraints: SearchConstraints,
) -> Option<SeedCheckpoint> {
    let key = condition_key(max_step, max_frontier, constraints);
    let new_path = checkpoint_path(seed_result_log, seed_index, &key);
    if let Ok(file) = fs::File::open(&new_path) {
        if let Ok(cp) = serde_json::from_reader::<_, SeedCheckpoint>(file) {
            if cp.seed_index == seed_index && cp.seed_sfen == seed_sfen {
                return Some(cp);
            }
        }
    }

    // Legacy fallback: older runs used seed_{i}.json without the condition
    // key. Open it, verify conditions match, and migrate by renaming to the
    // new path so future loads stay fast.
    let legacy_path = checkpoint_dir(seed_result_log).join(format!("seed_{seed_index}.json"));
    let file = fs::File::open(&legacy_path).ok()?;
    let checkpoint: SeedCheckpoint = serde_json::from_reader(file).ok()?;
    if checkpoint.seed_index == seed_index
        && checkpoint.seed_sfen == seed_sfen
        && checkpoint.max_step == max_step
        && checkpoint.max_frontier == max_frontier
        && checkpoint.constraints == constraints
    {
        let _ = fs::rename(&legacy_path, &new_path);
        Some(checkpoint)
    } else {
        None
    }
}

pub(super) fn remove_seed_checkpoint(
    seed_result_log: &Path,
    seed_index: usize,
    max_step: Option<u16>,
    max_frontier: Option<usize>,
    constraints: SearchConstraints,
) {
    let key = condition_key(max_step, max_frontier, constraints);
    let _ = fs::remove_file(checkpoint_path(seed_result_log, seed_index, &key));
}

pub(super) fn load_seed_result_log(
    path: &Path,
    max_step: Option<u16>,
    max_frontier: Option<usize>,
    constraints: SearchConstraints,
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
            && record.max_frontier == max_frontier
            && record.constraints == constraints
        {
            records.insert(record.seed_index, record);
        }
    }
    Ok(records)
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
    serde_json::to_writer(&mut *file, &record)?;
    writeln!(file)?;
    file.flush()?;
    Ok(())
}

pub(super) fn build_seed_result_record(
    seed_index: usize,
    seed: &PositionAux,
    max_step: Option<u16>,
    max_frontier: Option<usize>,
    constraints: SearchConstraints,
    best: &Option<(u32, Vec<PositionAux>)>,
    is_killer: bool,
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
        max_frontier,
        constraints,
        seed_index,
        seed_sfen: seed.sfen(),
        best_step: 0, // deprecated, kept for compat
        best_piece_count,
        positions,
        representative_sfen,
        skipped: is_killer,
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
        max_frontier: Option<usize>,
        constraints: SearchConstraints,
        marker: u32,
    ) -> SeedCheckpoint {
        SeedCheckpoint {
            seed_index,
            seed_sfen: seed_sfen.to_string(),
            max_step,
            max_frontier,
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
        }
    }

    #[test]
    fn condition_key_is_deterministic_and_distinct() {
        let a = SearchConstraints::default();
        let b = SearchConstraints {
            no_pawn: true,
            ..SearchConstraints::default()
        };
        assert_eq!(condition_key(None, None, a), condition_key(None, None, a));
        assert_ne!(condition_key(None, None, a), condition_key(None, None, b));
        assert_ne!(
            condition_key(None, None, a),
            condition_key(Some(7), None, a)
        );
        assert_ne!(
            condition_key(None, None, a),
            condition_key(None, Some(1), a)
        );
    }

    #[test]
    fn checkpoint_roundtrips_with_matching_conditions() {
        let dir = unique_test_dir("roundtrip");
        let log = dir.join("log.jsonl");
        let constraints = SearchConstraints::default();
        let cp = dummy_checkpoint(7, "sfen-x", Some(5), None, constraints, 42);
        write_seed_checkpoint(&log, &cp).unwrap();

        let loaded = load_seed_checkpoint(&log, 7, "sfen-x", Some(5), None, constraints);
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

        write_seed_checkpoint(&log, &dummy_checkpoint(7, "sfen-x", None, None, a, 1)).unwrap();
        write_seed_checkpoint(&log, &dummy_checkpoint(7, "sfen-x", None, None, b, 2)).unwrap();

        let la = load_seed_checkpoint(&log, 7, "sfen-x", None, None, a).unwrap();
        let lb = load_seed_checkpoint(&log, 7, "sfen-x", None, None, b).unwrap();
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
        let cp = dummy_checkpoint(7, "sfen-x", None, None, constraints, 99);
        let legacy_path = cp_dir.join("seed_7.json");
        serde_json::to_writer(fs::File::create(&legacy_path).unwrap(), &cp).unwrap();

        let loaded = load_seed_checkpoint(&log, 7, "sfen-x", None, None, constraints);
        assert!(loaded.is_some());
        assert_eq!(loaded.unwrap().best_piece_count, 99);

        let key = condition_key(None, None, constraints);
        let new_path = cp_dir.join(format!("seed_7_{key}.json"));
        assert!(new_path.exists(), "new-format file should exist after migrate");
        assert!(!legacy_path.exists(), "legacy file should be renamed");

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
        let cp = dummy_checkpoint(7, "sfen-x", None, None, a, 99);
        let legacy_path = cp_dir.join("seed_7.json");
        serde_json::to_writer(fs::File::create(&legacy_path).unwrap(), &cp).unwrap();

        // Loading with mismatched conditions should not migrate or use it.
        let loaded = load_seed_checkpoint(&log, 7, "sfen-x", None, None, b);
        assert!(loaded.is_none());
        assert!(legacy_path.exists(), "legacy file must remain for future runs");

        // Subsequent load with matching conditions still picks it up.
        let loaded = load_seed_checkpoint(&log, 7, "sfen-x", None, None, a);
        assert!(loaded.is_some());
        assert!(!legacy_path.exists(), "legacy file should now be migrated");

        let _ = fs::remove_dir_all(&dir);
    }
}
