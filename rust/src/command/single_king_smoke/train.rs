use anyhow::{bail, Context as _};
use fmrs_core::position::position::PositionAux;
use rustc_hash::FxHashMap;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::Path;

use super::super::smoke_features::{extract_features, feature_names};
use super::super::smoke_persistence::SeedResultRecord;

pub(super) fn export_features(
    feature_log: &Path,
    seed_result_log: &Path,
    out: &Path,
    min_label: u32,
) -> anyhow::Result<()> {
    // Build seed_index → best_piece_count map from seed result log.
    let mut labels: FxHashMap<usize, u32> = FxHashMap::default();
    let file = fs::File::open(seed_result_log)
        .with_context(|| format!("open seed_result_log {}", seed_result_log.display()))?;
    for (i, line) in BufReader::new(file).lines().enumerate() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let record: SeedResultRecord = match serde_json::from_str(&line) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("skip malformed seed_result line {}: {e}", i + 1);
                continue;
            }
        };
        let piece_count = if record.best_piece_count > 0 {
            record.best_piece_count
        } else {
            record.best_step as u32 / 2 + 3
        };
        // Keep the max in case of duplicates.
        let entry = labels.entry(record.seed_index).or_insert(0);
        if piece_count > *entry {
            *entry = piece_count;
        }
    }

    let names = feature_names();
    let mut writer = std::io::BufWriter::new(
        fs::File::create(out).with_context(|| format!("create {}", out.display()))?,
    );
    write!(writer, "seed_index,step,label")?;
    for n in names.iter() {
        write!(writer, ",{}", n)?;
    }
    writeln!(writer)?;

    let file = fs::File::open(feature_log)
        .with_context(|| format!("open feature_log {}", feature_log.display()))?;
    let mut total = 0usize;
    let mut kept = 0usize;
    for (i, line) in BufReader::new(file).lines().enumerate() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        total += 1;
        let v: serde_json::Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("skip malformed feature line {}: {e}", i + 1);
                continue;
            }
        };
        let seed_index = v["seed_index"].as_u64().unwrap_or(0) as usize;
        let step = v["step"].as_u64().unwrap_or(0) as u16;
        let Some(&label) = labels.get(&seed_index) else {
            continue;
        };
        if label < min_label {
            continue;
        }
        let features = match v["features"].as_array() {
            Some(arr) => arr,
            None => continue,
        };
        if features.len() != names.len() {
            eprintln!(
                "skip line {}: expected {} features, got {}",
                i + 1,
                names.len(),
                features.len()
            );
            continue;
        }
        write!(writer, "{seed_index},{step},{label}")?;
        for f in features {
            write!(writer, ",{}", f.as_f64().unwrap_or(0.0))?;
        }
        writeln!(writer)?;
        kept += 1;
    }
    writer.flush()?;
    eprintln!(
        "export-features: wrote {} rows (out of {} samples) to {}",
        kept,
        total,
        out.display()
    );
    Ok(())
}

pub(super) fn train_model(
    seed_result_log: &Path,
    model_out: &Path,
    min_label: u32,
) -> anyhow::Result<()> {
    use fmrs_core::solve::standard_solve::standard_solve;

    let file = fs::File::open(seed_result_log)
        .with_context(|| format!("open {}", seed_result_log.display()))?;
    let mut records: Vec<SeedResultRecord> = Vec::new();
    for line in BufReader::new(file).lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let Ok(record) = serde_json::from_str::<SeedResultRecord>(&line) else {
            continue;
        };
        if record.best_piece_count >= min_label && record.representative_sfen.is_some() {
            records.push(record);
        }
    }
    records.sort_by(|a, b| b.best_piece_count.cmp(&a.best_piece_count));
    records.dedup_by_key(|r| r.seed_index);
    eprintln!(
        "train-model: {} seeds with best_piece_count >= {}",
        records.len(),
        min_label
    );

    let names = feature_names();
    let csv_path = model_out.with_extension("csv");
    let mut writer = std::io::BufWriter::new(
        fs::File::create(&csv_path).with_context(|| format!("create {}", csv_path.display()))?,
    );
    write!(writer, "seed_index,step,label")?;
    for n in names.iter() {
        write!(writer, ",{n}")?;
    }
    writeln!(writer)?;

    let mut total_rows = 0usize;
    let mut solved = 0usize;
    let mut failed = 0usize;
    for record in &records {
        let sfen = record.representative_sfen.as_deref().unwrap();
        let Ok(pos) = PositionAux::from_sfen(sfen) else {
            failed += 1;
            continue;
        };
        let Ok(reconstructor) = standard_solve(pos.clone(), 1, true) else {
            failed += 1;
            continue;
        };
        let solutions = reconstructor.solutions();
        let Some(movements) = solutions.first() else {
            failed += 1;
            continue;
        };
        solved += 1;

        let mut pos = pos;
        let label = record.best_piece_count;
        for (i, mv) in movements.iter().enumerate() {
            if pos.turn().is_black() {
                let features = extract_features(&pos);
                write!(writer, "{},{i},{label}", record.seed_index)?;
                for f in &features {
                    write!(writer, ",{f}")?;
                }
                writeln!(writer)?;
                total_rows += 1;
            }
            pos.do_move(mv);
        }
        if pos.turn().is_black() {
            let features = extract_features(&pos);
            write!(writer, "{},{},{label}", record.seed_index, movements.len())?;
            for f in &features {
                write!(writer, ",{f}")?;
            }
            writeln!(writer)?;
            total_rows += 1;
        }
    }
    writer.flush()?;
    eprintln!(
        "train-model: {} rows from {} solved seeds ({} failed) → {}",
        total_rows,
        solved,
        failed,
        csv_path.display()
    );

    let script = Path::new("scripts/train_beam_model.py");
    if !script.exists() {
        bail!("training script not found: {}", script.display());
    }
    if let Some(parent) = model_out.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent)?;
    }
    let status = std::process::Command::new("python3")
        .arg(script)
        .arg("--csv")
        .arg(&csv_path)
        .arg("--out")
        .arg(model_out)
        .arg("--standardize")
        .status()
        .context("failed to run python3")?;
    if !status.success() {
        bail!("training script failed with {status}");
    }
    eprintln!("train-model: model saved to {}", model_out.display());
    Ok(())
}
