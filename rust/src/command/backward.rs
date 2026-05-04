use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{bail, Context};
use fmrs_core::{
    nohash::NoHashMap64,
    piece::Color,
    position::position::PositionAux,
    search::backward::{backward_initial_variants, BackwardSearch, BackwardSearchResumeState},
};
use serde::{Deserialize, Serialize};

use super::parse_to_sfen;

#[derive(Serialize, Deserialize)]
struct BackwardCheckpointFile {
    version: u32,
    black_turn: bool,
    forward: usize,
    parallel: usize,
    one_way: bool,
    no_black_goldish: bool,
    bare_white_king: bool,
    current_forward_index: usize,
    current_search: BackwardSearchResumeState,
    best_step: u16,
    best_positions: Vec<String>,
    remaining_variants: Vec<String>,
}

pub fn backward(
    sfen_like: Option<&str>,
    forward: usize,
    parallel: Option<usize>,
    black_turn: bool,
    one_way: bool,
    no_black_goldish: bool,
    bare_white_king: bool,
    dump_frontier_dir: Option<&str>,
    resume_frontier: Option<&str>,
) -> anyhow::Result<()> {
    if sfen_like.is_some() == resume_frontier.is_some() {
        bail!("Specify exactly one of sfen_like or --resume-frontier");
    }

    let sfen_like = sfen_like.map(str::to_owned);
    let resume_frontier = resume_frontier.map(PathBuf::from);
    let dump_frontier_dir = dump_frontier_dir.map(PathBuf::from).or_else(|| {
        resume_frontier
            .as_ref()
            .and_then(|path| path.parent().map(PathBuf::from))
    });

    let builder = std::thread::Builder::new().stack_size(32 * 1024 * 1024); // 32 MB
    let handler = builder.spawn(move || {
        let (step, positions) = if let Some(resume_frontier) = resume_frontier {
            run_from_checkpoint(&resume_frontier, parallel, dump_frontier_dir.as_deref())?
        } else {
            let sfen = parse_to_sfen(sfen_like.as_deref().expect("checked"))?;
            let mut position = PositionAux::from_sfen(&sfen)?;
            if position.checked_slow(Color::WHITE) {
                position.set_turn(Color::WHITE);
            }
            run_from_initial(
                &position,
                forward,
                parallel.unwrap_or(1),
                black_turn,
                one_way,
                no_black_goldish,
                bare_white_king,
                dump_frontier_dir.as_deref(),
            )?
        };

        eprintln!("mate in {}:", step);
        for position in positions {
            eprintln!("{}", position.sfen_url());
            println!("{}", position.sfen());
        }

        Ok::<_, anyhow::Error>(())
    })?;
    handler.join().unwrap()?;

    Ok(())
}

fn run_from_initial(
    position: &PositionAux,
    forward: usize,
    parallel: usize,
    black_turn: bool,
    one_way: bool,
    no_black_goldish: bool,
    bare_white_king: bool,
    dump_frontier_dir: Option<&Path>,
) -> anyhow::Result<(u16, Vec<PositionAux>)> {
    let mut best = (0, NoHashMap64::default());
    let variants = backward_initial_variants(position);

    run_skipping_variant_errors(variants.iter().enumerate(), |(variant_index, variant)| {
        let mut search =
            BackwardSearch::new_with_parallel(variant, one_way, parallel, no_black_goldish)?;
        let remaining_variants = variants[variant_index + 1..]
            .iter()
            .map(PositionAux::sfen)
            .collect::<Vec<_>>();
        run_variant(
            &mut search,
            0,
            forward,
            black_turn,
            bare_white_king,
            parallel,
            dump_frontier_dir,
            &mut best,
            &remaining_variants,
        )
    })?;

    finalize_best(best)
}

fn run_from_checkpoint(
    checkpoint_path: &Path,
    parallel_override: Option<usize>,
    dump_frontier_dir: Option<&Path>,
) -> anyhow::Result<(u16, Vec<PositionAux>)> {
    let file = fs::File::open(checkpoint_path)
        .with_context(|| format!("Failed to open checkpoint {}", checkpoint_path.display()))?;
    let checkpoint: BackwardCheckpointFile =
        serde_json::from_reader(file).context("Failed to parse checkpoint")?;

    if checkpoint.version != 1 {
        bail!("Unsupported checkpoint version {}", checkpoint.version);
    }

    let mut best = (
        checkpoint.best_step,
        checkpoint
            .best_positions
            .iter()
            .map(|sfen| PositionAux::from_sfen(sfen).map(|p| (p.digest(), p)))
            .collect::<anyhow::Result<NoHashMap64<_>>>()?,
    );

    let parallel = parallel_override.unwrap_or(checkpoint.parallel);
    let mut search = BackwardSearch::from_resume_state(&checkpoint.current_search, parallel)?;
    run_variant(
        &mut search,
        checkpoint.current_forward_index,
        checkpoint.forward,
        checkpoint.black_turn,
        checkpoint.bare_white_king,
        parallel,
        dump_frontier_dir,
        &mut best,
        &checkpoint.remaining_variants,
    )?;

    run_ignoring_variant_errors(checkpoint.remaining_variants.iter(), |variant_sfen| {
        let variant = PositionAux::from_sfen(variant_sfen)?;
        let mut search = BackwardSearch::new_with_parallel(
            &variant,
            checkpoint.one_way,
            parallel,
            checkpoint.no_black_goldish,
        )?;
        run_variant(
            &mut search,
            0,
            checkpoint.forward,
            checkpoint.black_turn,
            checkpoint.bare_white_king,
            parallel,
            dump_frontier_dir,
            &mut best,
            &[],
        )
    })?;

    finalize_best(best)
}

fn run_skipping_variant_errors<I, F>(variants: I, mut f: F) -> anyhow::Result<()>
where
    I: IntoIterator,
    F: FnMut(I::Item) -> anyhow::Result<()>,
{
    let mut last_error = None;
    let mut succeeded = false;

    for variant in variants {
        match f(variant) {
            Ok(()) => succeeded = true,
            Err(err) => {
                if last_error.is_none() {
                    last_error = Some(err);
                }
            }
        }
    }

    if succeeded {
        Ok(())
    } else {
        Err(last_error.unwrap_or_else(|| anyhow::anyhow!("No backward search result")))
    }
}

fn run_ignoring_variant_errors<I, F>(variants: I, mut f: F) -> anyhow::Result<()>
where
    I: IntoIterator,
    F: FnMut(I::Item) -> anyhow::Result<()>,
{
    for variant in variants {
        let _ = f(variant);
    }
    Ok(())
}

fn run_variant(
    search: &mut BackwardSearch,
    start_forward_index: usize,
    forward: usize,
    black_turn: bool,
    bare_white_king: bool,
    parallel: usize,
    dump_frontier_dir: Option<&Path>,
    best: &mut (u16, NoHashMap64<PositionAux>),
    remaining_variants: &[String],
) -> anyhow::Result<()> {
    for i in start_forward_index..=forward {
        if i > start_forward_index {
            search.forward();
            log::info!("forward to {} ({}/{})", search.step(), i, forward);
        }

        let (step, positions) = search.output_positions(black_turn, bare_white_king)?;
        merge_best(best, step, positions);

        let mut last_logged_step = search.step();
        loop {
            if !search.advance()? {
                break;
            }
            if search.step() != last_logged_step {
                last_logged_step = search.step();
                let checkpoint_path = if let Some(dir) = dump_frontier_dir {
                    let checkpoint = BackwardCheckpointFile {
                        version: 1,
                        black_turn,
                        forward,
                        parallel,
                        one_way: search.resume_state().one_way,
                        no_black_goldish: search.resume_state().no_black_goldish,
                        bare_white_king,
                        current_forward_index: i,
                        current_search: search.resume_state(),
                        best_step: best.0,
                        best_positions: best.1.values().map(PositionAux::sfen).collect(),
                        remaining_variants: remaining_variants.to_vec(),
                    };
                    Some(write_checkpoint(dir, &checkpoint)?)
                } else {
                    None
                };

                let (stone, positions) = search.positions();
                let url = PositionAux::new(positions[0].clone(), stone).sfen_url();
                eprintln!(
                    "backward step={} count={} {}",
                    search.step(),
                    positions.len(),
                    url
                );
                if let Some(path) = checkpoint_path {
                    eprintln!(
                        "resume: cargo run --release -- backward --resume-frontier '{}'",
                        path.display()
                    );
                }

                let (step, positions) = search.output_positions(black_turn, bare_white_king)?;
                merge_best(best, step, positions);
            }
        }
    }
    Ok(())
}

fn merge_best(best: &mut (u16, NoHashMap64<PositionAux>), step: u16, positions: Vec<PositionAux>) {
    if positions.is_empty() || step < best.0 {
        return;
    }
    if step > best.0 {
        best.0 = step;
        best.1.clear();
    }
    for position in positions {
        best.1.insert(position.digest(), position);
    }
}

fn finalize_best(best: (u16, NoHashMap64<PositionAux>)) -> anyhow::Result<(u16, Vec<PositionAux>)> {
    if best.1.is_empty() {
        bail!("No backward search result");
    }
    let mut positions = best.1.into_values().collect::<Vec<_>>();
    positions.sort_by_key(PositionAux::sfen);
    Ok((best.0, positions))
}

fn write_checkpoint(dir: &Path, checkpoint: &BackwardCheckpointFile) -> anyhow::Result<PathBuf> {
    fs::create_dir_all(dir)?;

    let file_name = format!(
        "backward-f{:03}-step-{:05}.json",
        checkpoint.current_forward_index, checkpoint.current_search.step
    );
    let path = dir.join(&file_name);
    let tmp_path = dir.join(format!(".{}.tmp", file_name));
    let latest_path = dir.join("latest.json");
    let latest_tmp_path = dir.join(".latest.json.tmp");

    serde_json::to_writer_pretty(fs::File::create(&tmp_path)?, checkpoint)?;
    fs::rename(&tmp_path, &path)?;

    serde_json::to_writer_pretty(fs::File::create(&latest_tmp_path)?, checkpoint)?;
    fs::rename(&latest_tmp_path, &latest_path)?;

    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::{run_ignoring_variant_errors, run_skipping_variant_errors};

    #[test]
    fn run_skipping_variant_errors_succeeds_if_any_variant_succeeds() {
        let variants = [0, 1, 2];
        let result = run_skipping_variant_errors(variants, |variant| match variant {
            1 => Ok(()),
            _ => anyhow::bail!("failed variant {variant}"),
        });
        assert!(result.is_ok());
    }

    #[test]
    fn run_skipping_variant_errors_returns_error_if_all_variants_fail() {
        let variants = [0, 1];
        let err = run_skipping_variant_errors(variants, |variant| {
            anyhow::bail!("failed variant {variant}")
        })
        .unwrap_err();
        assert!(err.to_string().contains("failed variant"));
    }

    #[test]
    fn run_ignoring_variant_errors_succeeds_even_if_all_variants_fail() {
        let variants = [0, 1];
        let result = run_ignoring_variant_errors(variants, |variant| {
            anyhow::bail!("failed variant {variant}")
        });
        assert!(result.is_ok());
    }
}
