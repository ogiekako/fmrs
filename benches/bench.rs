use criterion::{black_box, criterion_group, criterion_main, Criterion};
use fmrs::position::advance_old;
use fmrs::sfen::decode_position;

fn bench_advance_black(c: &mut Criterion) {
    let black_position = decode_position(include_str!("../problems/ofm-139_5.sfen")).unwrap();
    advance_old(&black_position).unwrap();
    c.bench_function("black advance", |b| {
        b.iter(|| advance_old(black_box(&black_position)).unwrap())
    });
}

fn bench_advance_white(c: &mut Criterion) {
    let white_positions = [
        "B+l+pn1+pR+p1/+lR7/3+p+p+p1+p1/2+p1+p4/3+p1+p1+p+l/2n+B+p2+p1/3+p+p1k1g/7s1/3gs1+p2 w GSNgsnlp 1",
        "B+l+pn1+pR+p1/+l8/3+p+p+pB+p1/2+p1+p4/3+p1+p1+p+l/2n1+p2+p1/1+R1+p+p1k1g/7s1/3gs1+p2 w GSNgsnlp 1",
        "B+l+pn1+pR+p1/+lR7/3+p+p+pB+p1/2+p1+p4/3+p1+p1+p+l/2n1+p2+p1/3+p+p1k1g/7s1/3gs1+pN1 w GSgsnlp 1",
    ].map(|x|decode_position(x).unwrap());
    advance_old(&white_positions[0]).unwrap();
    c.bench_function("white advance", |b| {
        b.iter(|| {
            for white_position in white_positions.iter() {
                advance_old(black_box(white_position)).unwrap();
            }
        })
    });
}

criterion_group!(
    name = benches;
    config = Criterion::default().noise_threshold(0.06);
    targets = bench_advance_black, bench_advance_white
);
criterion_main!(benches);
