use std::{fs::File, io::Write};

use fmrs_core::sfen;
use pprof::protos::Message;

use crate::solver::{self, Algorithm};

pub fn bench(file: &str) -> anyhow::Result<()> {
    let sfen = std::fs::read_to_string(file)?;
    let position = sfen::decode_position(&sfen).map_err(|_e| anyhow::anyhow!("parse failed"))?;

    let guard = pprof::ProfilerGuard::new(100).unwrap();

    let start = std::time::Instant::now();

    let answer =
        solver::solve(position, None, Algorithm::Standard).map_err(|e| anyhow::anyhow!("{}", e))?;
    assert_eq!(answer.len(), 1);

    println!(
        "duration: {:.2}s",
        (std::time::Instant::now() - start).as_secs_f64()
    );

    let report = guard.report().build().unwrap();
    let mut file = File::create("prof/profile.pb").unwrap();
    let profile = report.pprof().unwrap();

    let mut content = Vec::new();
    profile.write_to_vec(&mut content).unwrap();
    file.write_all(&content).unwrap();

    {
        let file = File::create("prof/flamegraph.svg").unwrap();
        let mut options = pprof::flamegraph::Options::default();
        options.image_width = Some(2500);
        report.flamegraph_with_options(file, &mut options).unwrap();
    }

    Ok(())
}
