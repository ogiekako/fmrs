use std::fs::File;

use crate::{sfen, solver};

pub fn bench() -> anyhow::Result<()> {
    let problem = include_str!("../../problems/forest-06-10_97.sfen");

    let position = sfen::decode_position(&problem).map_err(|_e| anyhow::anyhow!("parse failed"))?;

    let guard = pprof::ProfilerGuardBuilder::default().build()?;
    let answer = solver::solve(position.clone(), None).map_err(|e| anyhow::anyhow!("{}", e))?;
    assert_eq!(answer[0].len(), 97);

    if let Ok(report) = guard.report().build() {
        let file = File::create("flamegraph.svg").unwrap();
        let mut options = pprof::flamegraph::Options::default();
        options.image_width = Some(2500);
        report.flamegraph_with_options(file, &mut options).unwrap();
    };

    Ok(())
}
