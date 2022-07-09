use criterion::{black_box, criterion_group, criterion_main, Criterion};
use fmrs::position::advance_old;
use fmrs::sfen::decode_position;
use pprof::criterion::{Output, PProfProfiler};

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

criterion_group!(
    name = benches;
    config = Criterion::default().noise_threshold(0.06).with_profiler(PProfProfiler::new(100_000, Output::Protobuf));
    targets = bench_black_advance, bench_white_advance, bench_black_pinned
);
criterion_main!(benches);
