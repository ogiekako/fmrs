use std::time::Duration;

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use fmrs::one_way_mate_steps;
use fmrs::solver::standard_solve::standard_solve;
use fmrs::solver::{solve, Algorithm};
use fmrs_core::memo::Memo;
use fmrs_core::piece::{Color, Kind};
use fmrs_core::position::advance::attack_prevent::attacker;
use fmrs_core::position::advance::pinned::pinned;
use fmrs_core::position::bitboard::reachable;
use fmrs_core::position::position::PositionAux;
use fmrs_core::position::{advance::advance, checked, AdvanceOptions, Position, Square};
use fmrs_core::sfen::decode_position;
use pprof::criterion::{Output, PProfProfiler};
use rand::Rng;
use rand::{rngs::SmallRng, SeedableRng};
use shtsume_rs::ffi::mvlist::{generate_check, generate_evasion};
use shtsume_rs::ffi::sdata::Sdata;
use shtsume_rs::ffi::ssdata::Ssdata;
use shtsume_rs::ffi::tbase::Tbase;
use shtsume_rs::ffi::Global;

fn bench_black_advance(c: &mut Criterion) {
    let mut black_position = decode_position(include_str!("../problems/ofm-139_5.sfen")).unwrap();
    let mut result = vec![];
    let mut memo = Memo::default();
    advance(
        &mut black_position,
        &mut memo,
        1,
        &AdvanceOptions::default(),
        &mut result,
    )
    .unwrap();
    c.bench_function("black_advance", |b| {
        b.iter(|| {
            result.clear();
            memo.clear();
            advance(
                &mut black_position,
                &mut memo,
                1,
                &AdvanceOptions::default(),
                &mut result,
            )
            .unwrap();
            assert_eq!(result.len(), 66);
        })
    });
}

fn bench_black_advance_shtsume(c: &mut Criterion) {
    let _g = shtsume_rs::ffi::Global::init(0, None);

    let ssdata = Ssdata::from_sfen(include_str!("../problems/ofm-139_5.sfen"));
    let mut tbase = Tbase::default();

    let mut sdata = Sdata::from_ssdata(&ssdata);
    assert_eq!(sdata.is_illegal(), 0);

    c.bench_function("black_advance_shtsume", |b| {
        b.iter(|| {
            let sdata = Sdata::from_ssdata(&ssdata);
            let list = generate_check(&sdata, &mut tbase);
            let count = list.iter().map(|item| item.mlist().count()).sum::<usize>();
            assert_eq!(count, 66);
        })
    });
}

fn bench_white_advance(c: &mut Criterion) {
    let mut white_positions = [
        ("B+l+pn1+pR+p1/+lR7/3+p+p+p1+p1/2+p1+p4/3+p1+p1+p+l/2n+B+p2+p1/3+p+p1k1g/7s1/3gs1+p2 w GSNgsnlp 1", 42),
        ("B+l+pn1+pR+p1/+l8/3+p+p+pB+p1/2+p1+p4/3+p1+p1+p+l/2n1+p2+p1/1+R1+p+p1k1g/7s1/3gs1+p2 w GSNgsnlp 1", 49),
        ("B+l+pn1+pR+p1/+lR7/3+p+p+pB+p1/2+p1+p4/3+p1+p1+p+l/2n1+p2+p1/3+p+p1k1g/7s1/3gs1+pN1 w GSgsnlp 1", 9),
    ].map(|x|(decode_position(x.0).unwrap(), x.1));

    let mut result = vec![];
    let mut memo = Memo::default();

    advance(
        &mut white_positions[0].0,
        &mut memo,
        1,
        &AdvanceOptions::default(),
        &mut result,
    )
    .unwrap();
    c.bench_function("white_advance", |b| {
        b.iter(|| {
            for (white_position, want) in white_positions.iter_mut() {
                result.clear();
                memo.clear();
                advance(
                    black_box(white_position),
                    &mut memo,
                    1,
                    &AdvanceOptions::default(),
                    black_box(&mut result),
                )
                .unwrap();
                assert_eq!(result.len(), *want);
            }
        })
    });
}

fn bench_white_advance_shtsume(c: &mut Criterion) {
    let  white_positions = vec![
        ("B+l+pn1+pR+p1/+lR7/3+p+p+p1+p1/2+p1+p4/3+p1+p1+p+l/2n+B+p2+p1/3+p+p1k1g/7s1/3gs1+p2 w GSNgsnlp 1", 42),
        ("B+l+pn1+pR+p1/+l8/3+p+p+pB+p1/2+p1+p4/3+p1+p1+p+l/2n1+p2+p1/1+R1+p+p1k1g/7s1/3gs1+p2 w GSNgsnlp 1", 49),
        ("B+l+pn1+pR+p1/+lR7/3+p+p+pB+p1/2+p1+p4/3+p1+p1+p+l/2n1+p2+p1/3+p+p1k1g/7s1/3gs1+pN1 w GSgsnlp 1", 9),
    ];

    let _g = Global::init(0, None);

    let mut tbase = Tbase::default();

    let data = white_positions
        .into_iter()
        .map(|x| {
            let ssdata = Ssdata::from_sfen(x.0);
            (ssdata, x.1)
        })
        .collect::<Vec<_>>();

    c.bench_function("white_advance_shtsume", |b| {
        b.iter(|| {
            for (ssdata, want) in data.iter() {
                let sdata = Sdata::from_ssdata(&ssdata);
                let res = generate_evasion(&sdata, &mut tbase, true);
                assert_eq!(res.iter().map(|i| i.mlist().count()).sum::<usize>(), *want);
            }
        })
    });
}

fn bench_black_pinned(c: &mut Criterion) {
    let mut black_position =
        decode_position("8l/l1SS1PGP1/1PPNP1Pp1/R1g5k/9/2s1gnbsP/3nN4/1lg3l2/K2+B2r2 b 8P2p 1")
            .unwrap();
    let mut result = vec![];
    c.bench_function("black_pinned", |b| {
        b.iter(|| {
            advance(
                black_box(&mut black_position),
                &mut Memo::default(),
                1,
                &AdvanceOptions::default(),
                black_box(&mut result),
            )
            .unwrap()
        })
    });
}

fn bench_solve3(c: &mut Criterion) {
    let mut position = decode_position("B+l+pn1+pR+p1/+lR7/3+p+p+pB+p1/2+p1+p4/3+p1+p1+p+l/2n1+p2+p1/3+p+p1k1g/7s1/3gs2+p1 b GSgs2nlp 1").unwrap();
    let mut result = vec![];
    c.bench_function("solve3", |b| {
        b.iter(|| {
            advance(
                black_box(&mut position),
                &mut Memo::default(),
                1,
                &AdvanceOptions::default(),
                black_box(&mut result),
            )
            .unwrap()
        })
    });
}

fn bench_oneway(c: &mut Criterion) {
    let positions = [
        (decode_position("B+l+pn1+pR+p1/+lR7/3+p+p+pB+p1/2+p1+p4/3+p1+p1+p+l/2n1+p2+p1/3+p+p1k1g/7s1/3gs2+p1 b GSgs2nlp 1").unwrap(), None),
        (decode_position("B+l+pn1+pR+p1/+lR7/3+p+p+p+B+p1/2+p1+p4/3+p1+p1+p+l/2n1+p2+p1/2n+p+p1k1g/7s1/1K1gs2+p1 b GSgsnlp 1").unwrap(), None),
        (decode_position(include_str!("../problems/diamond.sfen")).unwrap(), Some(55)),
    ];
    c.bench_function("oneway", |b| {
        b.iter(|| {
            positions.iter().for_each(|(position, steps)| {
                let mut position = PositionAux::new(position.clone());
                assert_eq!(
                    one_way_mate_steps(black_box(&mut position), &mut vec![]),
                    *steps
                )
            })
        })
    });
}

fn random_positions(rng: &mut SmallRng, len: usize) -> Vec<Position> {
    random_positions_with_filter(rng, len, |_| true)
}

fn random_positions_with_filter<F: Fn(&mut Position) -> bool>(
    rng: &mut SmallRng,
    len: usize,
    ok: F,
) -> Vec<Position> {
    let mut positions = vec![];
    while positions.len() < len {
        let mut position = Position::default();

        let hand_prob = rng.gen_range(0.0..0.7);
        let mut pieces = vec![18, 4, 4, 4, 4, 2, 2, 2];
        let mut remaining = 40;
        let mut put_king_color = None;
        while remaining > 0 {
            let i = rng.gen_range(0..pieces.len());
            if pieces[i] == 0 {
                continue;
            }
            let c: Color = rng.gen();
            let k = Kind::from_index(i);

            if rng.gen_bool(hand_prob) && k.is_hand_piece() {
                position.hands_mut().add(c, k);
                pieces[i] -= 1;
                remaining -= 1;
                continue;
            }
            let pos: Square = rng.gen();
            if position.get(pos).is_some() {
                continue;
            }
            if k == Kind::King {
                if put_king_color == Some(c) {
                    continue;
                } else if put_king_color.is_none() {
                    put_king_color = Some(c);
                }
            }

            if rng.gen() && k.is_promotable() {
                position.set(pos, c, k.promote().unwrap());
            } else {
                position.set(pos, c, k);
            }
            pieces[i] -= 1;
            remaining -= 1;
        }
        if !ok(&mut position) {
            continue;
        }
        positions.push(position);
    }
    positions
}

fn bench_reachable(c: &mut Criterion) {
    let mut rng = SmallRng::seed_from_u64(20241211182711);
    let positions = random_positions(&mut rng, 300);
    let mut test_cases = vec![];
    for position in positions {
        let (color, pos, kind) = loop {
            let pos: Square = rng.gen();
            if let Some((color, kind)) = position.get(pos) {
                break (color, pos, kind);
            }
        };
        let capture_same_color: bool = rng.gen();
        test_cases.push((
            PositionAux::new(position),
            color,
            pos,
            kind,
            capture_same_color,
        ));
    }

    c.bench_function("reachable", |b| {
        b.iter_with_setup(
            || test_cases.clone(),
            |mut test_cases| {
                test_cases.iter_mut().for_each(
                    |(position, color, pos, kind, capture_same_color)| {
                        let bb = reachable(position, *color, *pos, *kind, *capture_same_color);
                        black_box(bb);
                    },
                )
            },
        )
    });
}

fn bench_attacker(c: &mut Criterion) {
    let mut rng = SmallRng::seed_from_u64(202412251401);
    let positions = random_positions_with_filter(&mut rng, 300, |position| {
        let mut position_aux = PositionAux::new(position.clone());
        position_aux.checked_slow(Color::WHITE)
    });
    let mut test_cases = vec![];
    for position in positions {
        test_cases.push(PositionAux::new(position));
    }

    c.bench_function("attacker", |b| {
        b.iter_with_setup(
            || test_cases.clone(),
            |mut test_cases| {
                test_cases.iter_mut().for_each(|position| {
                    attacker(position, Color::WHITE, false);
                    attacker(position, Color::WHITE, true);
                })
            },
        )
    });
}

fn bench_pinned300(c: &mut Criterion) {
    let mut rng = SmallRng::seed_from_u64(202412131444);
    let positions = random_positions(&mut rng, 700);

    let mut test_cases = vec![];
    for position in positions {
        let king_color: Color = rng.gen();

        let mut position_aux = PositionAux::new(position.clone());
        if checked(&mut position_aux, king_color) {
            continue;
        }

        let blocker_color: Color = rng.gen();
        test_cases.push((position_aux, king_color, blocker_color));
        if test_cases.len() >= 300 {
            break;
        }
    }
    assert_eq!(300, test_cases.len());

    c.bench_function("pinned300", |b| {
        b.iter_with_setup(
            || test_cases.clone(),
            |mut test_cases| {
                test_cases
                    .iter_mut()
                    .for_each(|(position, king_color, blocker_color)| {
                        let pinned = pinned(position, *king_color, *blocker_color);
                        black_box(pinned);
                    });
            },
        )
    });
}

fn bench_solve97(c: &mut Criterion) {
    let position = decode_position(include_str!("../problems/forest-06-10_97.sfen")).unwrap();
    let n_samples = 3;
    let mut times = vec![];
    for _ in 0..n_samples {
        let start = std::time::Instant::now();
        let solutions = standard_solve(position.clone(), 1);
        debug_assert_eq!(1, solutions.as_ref().unwrap().len());
        debug_assert_eq!(97, solutions.as_ref().unwrap()[0].len());

        times.push(start.elapsed());
    }
    let mut i = 0;
    c.bench_function("bench_solve97", |b| {
        b.iter(|| {
            let start = std::time::Instant::now();
            while start.elapsed() < times[i] / 1_000 {}
            i = (i + 1) % n_samples;
        })
    });
}

fn bench_heavy_problem(
    c: &mut Criterion,
    name: &str,
    sfen: &str,
    steps: usize,
    n_samples: usize,
    algo: Algorithm,
) {
    let position = decode_position(sfen).unwrap();
    let mut times = vec![];
    for _ in 0..n_samples {
        let start = std::time::Instant::now();
        let solutions = solve(position.clone(), 1.into(), algo, None).unwrap();
        assert_eq!(1, solutions.len());
        assert_eq!(steps, solutions[0].len());

        times.push(start.elapsed());
        assert!(times[0] > Duration::from_millis(100));
    }
    let mut i = 0;
    c.bench_function(name, |b| {
        b.iter(|| {
            let start = std::time::Instant::now();
            while start.elapsed() < times[i] / 1_000_000 {}
            i = (i + 1) % n_samples;
        })
    });
}

fn bench_jugemu(c: &mut Criterion) {
    bench_heavy_problem(
        c,
        "bench_jugemu",
        include_str!("../problems/jugemu_gentei_kai_36603.sfen"),
        36603,
        1,
        Algorithm::Standard,
    );
}

fn bench_1461(c: &mut Criterion) {
    bench_heavy_problem(
        c,
        "bench_1461",
        include_str!("../problems/morishige_1461.sfen"),
        1461,
        1,
        Algorithm::Standard,
    );
}

fn bench_1965(c: &mut Criterion) {
    bench_heavy_problem(
        c,
        "bench_1965",
        include_str!("../problems/morishige_1965.sfen"),
        1965,
        2,
        Algorithm::Standard,
    );
}

fn bench_bataco(c: &mut Criterion) {
    bench_heavy_problem(
        c,
        "bench_bataco",
        include_str!("../problems/bataco_4247.sfen"),
        4247,
        1,
        Algorithm::Parallel,
    );
}

criterion_group!(
    name = benches;
    // To generate profiling data, run `cargo bench <target> -- --profile-time 5`.
    // https://bheisler.github.io/criterion.rs/book/user_guide/profiling.html#implementing-in-process-profiling-hooks
    // And it generates target/criterion/<target>/profile/profile.pb.
    config = Criterion::default().noise_threshold(0.06).with_profiler(PProfProfiler::new(100_000, Output::Protobuf)).measurement_time(Duration::from_secs(4)).warm_up_time(Duration::from_secs(2));
    targets = bench_black_advance, bench_black_advance_shtsume, bench_white_advance, bench_white_advance_shtsume, bench_black_pinned, bench_solve3, bench_oneway, bench_reachable, bench_pinned300, bench_solve97, bench_attacker,
);

const EXTRA: bool = option_env!("FMRS_ENABLE_EXTRA_BENCH").is_some();

fn bench_extra() {
    if !EXTRA {
        return;
    }
    bench_extra_inner();
}

criterion_group!(
    name = bench_extra_inner;
    config = Criterion::default().measurement_time(Duration::from_secs(1)).warm_up_time(Duration::from_millis(500)).nresamples(10).sample_size(10);
    targets = bench_jugemu, bench_1965, bench_1461, bench_bataco,
);

criterion_main!(benches, bench_extra);
