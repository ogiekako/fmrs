use std::{fs::File, io::Write};

use fmrs_core::sfen;
use pprof::protos::Message;

use crate::solver::{self, Algorithm};

pub fn bench() -> anyhow::Result<()> {
    // let problem = include_str!("../../problems/forest-06-10_97.sfen");
    // let problem = include_str!("../../problems/ofm-139_5.sfen");
    let problem = include_str!("../../problems/chain_207.sfen");

    let position = sfen::decode_position(problem).map_err(|_e| anyhow::anyhow!("parse failed"))?;

    let guard = pprof::ProfilerGuardBuilder::default()
        .frequency(60)
        .build()?;

    let start = std::time::Instant::now();

    let answer =
        solver::solve(position, None, Algorithm::Parallel).map_err(|e| anyhow::anyhow!("{}", e))?;
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
