use criterion::{black_box, criterion_group, criterion_main, Criterion};
use fmrs::one_way_mate_steps;
use fmrs_core::piece::{Color, Kind};
use fmrs_core::position::bitboard::reachable;
use fmrs_core::position::{advance_old, Position, Square};
use fmrs_core::sfen::decode_position;
use pprof::criterion::{Output, PProfProfiler};
use rand::Rng;
use rand::{rngs::SmallRng, SeedableRng};

fn bench_black_advance(c: &mut Criterion) {
    let black_position = decode_position(include_str!("../problems/ofm-139_5.sfen")).unwrap();
    advance_old(&black_position).unwrap();
    c.bench_function("black_advance", |b| {
        b.iter(|| advance_old(black_box(&black_position)).unwrap())
    });
}

fn bench_white_advance(c: &mut Criterion) {
    let white_positions = [
        "B+l+pn1+pR+p1/+lR7/3+p+p+p1+p1/2+p1+p4/3+p1+p1+p+l/2n+B+p2+p1/3+p+p1k1g/7s1/3gs1+p2 w GSNgsnlp 1",
        "B+l+pn1+pR+p1/+l8/3+p+p+pB+p1/2+p1+p4/3+p1+p1+p+l/2n1+p2+p1/1+R1+p+p1k1g/7s1/3gs1+p2 w GSNgsnlp 1",
        "B+l+pn1+pR+p1/+lR7/3+p+p+pB+p1/2+p1+p4/3+p1+p1+p+l/2n1+p2+p1/3+p+p1k1g/7s1/3gs1+pN1 w GSgsnlp 1",
    ].map(|x|decode_position(x).unwrap());
    advance_old(&white_positions[0]).unwrap();
    c.bench_function("white_advance", |b| {
        b.iter(|| {
            for white_position in white_positions.iter() {
                advance_old(black_box(white_position)).unwrap();
            }
        })
    });
}

fn bench_black_pinned(c: &mut Criterion) {
    let black_position =
        decode_position("8l/l1SS1PGP1/1PPNP1Pp1/R1g5k/9/2s1gnbsP/3nN4/1lg3l2/K2+B2r2 b 8P2p 1")
            .unwrap();
    c.bench_function("black_pinned", |b| {
        b.iter(|| advance_old(black_box(&black_position)).unwrap())
    });
}

fn bench_solve3(c: &mut Criterion) {
    let position = decode_position("B+l+pn1+pR+p1/+lR7/3+p+p+pB+p1/2+p1+p4/3+p1+p1+p+l/2n1+p2+p1/3+p+p1k1g/7s1/3gs2+p1 b GSgs2nlp 1").unwrap();
    c.bench_function("solve3", |b| {
        b.iter(|| advance_old(black_box(&position)).unwrap())
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
                assert_eq!(one_way_mate_steps(black_box(position)), *steps)
            })
        })
    });
}

fn bench_reachable(c: &mut Criterion) {
    let mut test_cases = vec![];
    let mut rng = SmallRng::seed_from_u64(20241211182711);
    for _ in 0..100 {
        let mut position = Position::new();

        let hand_prob = rng.gen_range(0.0..0.7);
        let mut pieces = vec![18, 4, 4, 4, 4, 2, 2, 2];
        let mut remaining = 40;
        let mut pos_list = vec![];
        while remaining > 0 {
            let i = rng.gen_range(0..pieces.len());
            if pieces[i] == 0 {
                continue;
            }
            let c: Color = rng.gen();
            let k = Kind::from_index(i);

            if rng.gen_bool(hand_prob) {
                position.hands_mut().add(c, k);
                pieces[i] -= 1;
                remaining -= 1;
                continue;
            }
            let pos: Square = rng.gen();
            if position.get(pos).is_some() {
                continue;
            }
            position.set(pos, c, k);
            pieces[i] -= 1;
            remaining -= 1;
            pos_list.push(pos);
        }
        let pos = pos_list[rng.gen_range(0..pos_list.len())];
        let (color, kind) = position.get(pos).unwrap();
        let capture_same_color: bool = rng.gen();
        test_cases.push((position, color, pos, kind, capture_same_color));
    }
    c.bench_function("reachable", |b| {
        b.iter(|| {
            test_cases
                .iter()
                .for_each(|(position, color, pos, kind, capture_same_color)| {
                    reachable(
                        position.color_bb(),
                        *color,
                        *pos,
                        *kind,
                        *capture_same_color,
                    );
                })
        })
    });
}

criterion_group!(
    name = benches;
    // To generate profiling data, run `cargo bench <target> -- --profile-time 5`.
    // https://bheisler.github.io/criterion.rs/book/user_guide/profiling.html#implementing-in-process-profiling-hooks
    // And it generates target/criterion/<target>/profile/profile.pb.
    config = Criterion::default().noise_threshold(0.06).with_profiler(PProfProfiler::new(100_000, Output::Protobuf));
    targets = bench_black_advance, bench_white_advance, bench_black_pinned, bench_solve3, bench_oneway, bench_reachable
);
criterion_main!(benches);