// web の自動逆算 (rust/wasm/src/backward_search.rs) と同じロジックをネイティブで
// 実行し、進捗 (step / 経過時間 / steps/min) を表示しながら唯一解のまま逆算する。
//
// usage:
//   cargo run --release -p fmrs_core --example backward_extend -- "<sfen>" [--one-way] [--parallel N]
//
// --parallel N: 唯一性検証 (深い問題では共有 BFS) を N スレッドで実行。
//
// 出力: step が進むたびに 1 行、停滞中は 30 秒ごとに heartbeat。
// 終了時に最終盤面 (黒番) の SFEN を表示。

use fmrs_core::piece::Color;
use fmrs_core::position::position::PositionAux;
use fmrs_core::search::backward::{backward_initial_variants, BackwardSearch};
use std::collections::VecDeque;
use std::time::{Duration, Instant};

fn best_index(inners: &[BackwardSearch]) -> usize {
    inners
        .iter()
        .enumerate()
        .max_by_key(|(_, inner)| inner.step())
        .map(|(index, _)| index)
        .unwrap()
}

fn sfen_of(inners: &[BackwardSearch]) -> String {
    let (stone, positions) = inners[best_index(inners)].positions();
    PositionAux::new(positions[0].clone(), stone).sfen()
}

fn main() {
    let mut sfen = None;
    let mut one_way = false;
    let mut parallel = 1usize;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--one-way" {
            one_way = true;
        } else if arg == "--parallel" {
            parallel = args
                .next()
                .and_then(|s| s.parse().ok())
                .expect("--parallel N");
        } else {
            sfen = Some(arg);
        }
    }
    let sfen = sfen.expect("usage: backward_extend <sfen> [--one-way] [--parallel N]");

    let mut position = PositionAux::from_sfen(&sfen).unwrap();
    if position.checked_slow(Color::WHITE) {
        position.set_turn(Color::WHITE);
    }

    let mut inners: Vec<BackwardSearch> = backward_initial_variants(&position)
        .into_iter()
        .filter_map(|variant| match BackwardSearch::new(&variant, one_way) {
            Ok(s) => Some(s),
            Err(e) => {
                eprintln!("variant skipped: {e}");
                None
            }
        })
        .collect();
    assert!(!inners.is_empty(), "failed to initialize backward search");
    if parallel > 1 {
        for inner in inners.iter_mut() {
            inner.set_pool(
                rayon::ThreadPoolBuilder::new()
                    .num_threads(parallel)
                    .build()
                    .unwrap(),
            );
        }
    }

    let start = Instant::now();
    let initial_step = inners[best_index(&inners)].step();
    let mut last_step = initial_step;
    let mut last_black_sfen = sfen_of(&inners);
    let mut last_report = Instant::now();
    // 直近レート算出用: (時刻, step) を step 更新ごとに記録
    let mut history: VecDeque<(Instant, u16)> = VecDeque::new();
    history.push_back((start, initial_step));

    eprintln!(
        "start: step={initial_step} (mate-in-{initial_step}, one_way={one_way}, parallel={parallel}) variants={}",
        inners.len()
    );

    loop {
        // 深い問題の検証はステップ単位の共有 BFS でまとめて行われるため、
        // 1 コールでステップ全体を進める。
        let has_next = inners
            .iter_mut()
            .any(|inner| inner.advance_upto(usize::MAX / 2).unwrap());

        let step = inners[best_index(&inners)].step();
        let now = Instant::now();

        if step != last_step {
            let current_sfen = sfen_of(&inners);
            if step % 2 == 1 {
                last_black_sfen = current_sfen.clone();
            }
            history.push_back((now, step));
            while history.len() > 21 {
                history.pop_front();
            }
            let overall = 60.0 * f64::from(step - initial_step) / start.elapsed().as_secs_f64();
            let (t0, s0) = *history.front().unwrap();
            let recent_dt = now.duration_since(t0).as_secs_f64();
            let recent = if recent_dt > 0.0 {
                60.0 * f64::from(step - s0) / recent_dt
            } else {
                f64::NAN
            };
            eprintln!(
                "step {step} (+{}) elapsed {:.0?} rate {overall:.1} steps/min (recent {recent:.1})  {current_sfen}",
                step - initial_step,
                start.elapsed(),
            );
            last_step = step;
            last_report = now;
        } else if now.duration_since(last_report) >= Duration::from_secs(30) {
            let frontier = inners[best_index(&inners)].positions().1.len();
            eprintln!(
                "... step {step} 検証中 (frontier {frontier} 局面, elapsed {:.0?})",
                start.elapsed()
            );
            last_report = now;
        }

        if !has_next {
            break;
        }
    }

    eprintln!(
        "finished: step {last_step} (+{}) in {:.0?}",
        last_step - initial_step,
        start.elapsed()
    );
    println!("{last_black_sfen}");
}
