use std::hint::black_box;

use fmrs_core::piece::{Color, Kind};
use fmrs_core::position::advance::advance::advance_aux;
use fmrs_core::position::advance::attack_prevent::attacker;
use fmrs_core::position::bitboard::reachable;
use fmrs_core::position::position::PositionAux;
use fmrs_core::position::{AdvanceOptions, Movement, Position, Square};
use fmrs_core::search::canonicalize::canonicalize_attacker_goldish;
use fmrs_core::sfen::decode_position;
use fmrs_core::solve::low_mem_standard::low_mem_standard_solve;
use iai_callgrind::{library_benchmark, library_benchmark_group, main};
use rand::Rng;
use rand::{rngs::SmallRng, SeedableRng};

// ---- helpers ----------------------------------------------------------------

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
        let mut pieces = [18, 4, 4, 4, 4, 2, 2, 2];
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
            if rng.gen() && k.can_promote() {
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

// ---- black_advance ----------------------------------------------------------

fn setup_black_advance() -> (PositionAux, Vec<Movement>) {
    let pos = decode_position(include_str!("../problems/ofm-139_5.sfen")).unwrap();
    (pos, vec![])
}

#[library_benchmark]
#[bench::default(setup_black_advance())]
fn bench_black_advance((mut pos, mut result): (PositionAux, Vec<Movement>)) {
    advance_aux(&mut pos, &AdvanceOptions::default(), &mut result).unwrap();
    black_box(&result);
}

// ---- white_advance ----------------------------------------------------------

fn setup_white_advance() -> Vec<(PositionAux, usize)> {
    [
        ("B+l+pn1+pR+p1/+lR7/3+p+p+p1+p1/2+p1+p4/3+p1+p1+p+l/2n+B+p2+p1/3+p+p1k1g/7s1/3gs1+p2 w GSNgsnlp 1", 42),
        ("B+l+pn1+pR+p1/+l8/3+p+p+pB+p1/2+p1+p4/3+p1+p1+p+l/2n1+p2+p1/1+R1+p+p1k1g/7s1/3gs1+p2 w GSNgsnlp 1", 49),
        ("B+l+pn1+pR+p1/+lR7/3+p+p+pB+p1/2+p1+p4/3+p1+p1+p+l/2n1+p2+p1/3+p+p1k1g/7s1/3gs1+pN1 w GSgsnlp 1", 9),
    ]
    .map(|(s, n)| (decode_position(s).unwrap(), n))
    .to_vec()
}

#[library_benchmark]
#[bench::default(setup_white_advance())]
fn bench_white_advance(mut cases: Vec<(PositionAux, usize)>) {
    let mut result = vec![];
    for (pos, want) in cases.iter_mut() {
        result.clear();
        advance_aux(black_box(pos), &AdvanceOptions::default(), &mut result).unwrap();
        assert_eq!(result.len(), *want);
    }
    black_box(&result);
}

// ---- reachable --------------------------------------------------------------

type ReachableCase = (PositionAux, Color, Square, Kind, bool);

fn setup_reachable() -> Vec<ReachableCase> {
    let mut rng = SmallRng::seed_from_u64(20241211182711);
    let positions = random_positions(&mut rng, 300);
    let mut cases = vec![];
    for position in positions {
        let (color, pos, kind) = loop {
            let pos: Square = rng.gen();
            if let Some((color, kind)) = position.get(pos) {
                break (color, pos, kind);
            }
        };
        let capture_same_color: bool = rng.gen();
        cases.push((
            PositionAux::new(position, None),
            color,
            pos,
            kind,
            capture_same_color,
        ));
    }
    cases
}

#[library_benchmark]
#[bench::default(setup_reachable())]
fn bench_reachable(mut cases: Vec<ReachableCase>) {
    for (pos, color, sq, kind, cap) in cases.iter_mut() {
        black_box(reachable(pos, *color, *sq, *kind, *cap));
    }
}

// ---- attacker ---------------------------------------------------------------

fn setup_attacker() -> Vec<PositionAux> {
    let mut rng = SmallRng::seed_from_u64(202412251401);
    let positions = random_positions_with_filter(&mut rng, 300, |position| {
        let mut aux = PositionAux::new(position.clone(), None);
        aux.checked_slow(Color::WHITE)
    });
    positions
        .into_iter()
        .map(|p| PositionAux::new(p, None))
        .collect()
}

#[library_benchmark]
#[bench::default(setup_attacker())]
fn bench_attacker(mut cases: Vec<PositionAux>) {
    for pos in cases.iter_mut() {
        black_box(attacker(pos, Color::WHITE, false));
        black_box(attacker(pos, Color::WHITE, true));
    }
}

// ---- canonicalize -----------------------------------------------------------

fn setup_canonicalize() -> Vec<PositionAux> {
    [
        "8k/9/9/9/9/9/9/9/B8 b 4r3b4g4s4n4l18p 1",
        "8k/9/9/9/9/9/9/9/+P+P+P6 b 4r4b4g4s4n4l15p 1",
        "8k/9/9/9/9/9/9/9/G+S+N6 b 4r4b3g3s3n4l16p 1",
        "8k/9/9/9/9/9/G+S+N+L+SG3/+N+L+SG5/9 b 4r4b1gs2n2l13p 1",
        "8k/9/9/9/9/9/9/9/G8 b 4r4b3g4s4n4l 1",
    ]
    .iter()
    .map(|s| decode_position(s).unwrap())
    .collect()
}

#[library_benchmark]
#[bench::default(setup_canonicalize())]
fn bench_canonicalize(mut positions: Vec<PositionAux>) {
    for p in positions.iter_mut() {
        canonicalize_attacker_goldish(black_box(p));
    }
    black_box(&positions);
}

// ---- near_mate --------------------------------------------------------------

fn setup_near_mate() -> Vec<PositionAux> {
    [
        "4l1+P2/3+P1n3/S3p1+L2/1S1G1p2G/3L3kS/1N1p1l1B1/B4N2R/1P1g1K1p1/PNP1P3P b rgs7p 1",
        "ggssn2p1/lgssn3l/2b6/5N3/+R2+l5/8k/6+n2/2g4L1/KBr3PP1 b 2P13p 1",
        "s2B5/1L1S5/1PPPPL2R/1+l5N1/P6N1/2k4N1/7N1/1pgg2g2/K5+Br1 b g2sl12p 1",
        "9/G2s4G/LLpNGNpPP/4L4/1sN3N2/1g1bpb1ss/1pk3P2/P2PPP3/1PP5K b 2rl5p 1",
        "9/6GGr/S1ssggN1b/pL7/4P1P1K/2PP1P3/kPpll1pp+B/+n8/L3+n1+nsr b 2P6p 1",
        "4GS2l/4S1Bp1/3N1N1N1/ssg1gp3/3gl3+R/3+npk2K/PPP5b/3P2l1r/4P4 b L3P7p 1",
        "9/9/9/9/5S1gN/8N/7G1/6NNk/6G2 b 2r2bg3s4l18p 1",
        "r2GSG3/3S1S3/+BPN1N+LLg1/2pp3ps/K2N5/4P1P2/BgN3p1P/r1l3l2/5k1P1 b P8p 1",
        "gpn1+P2nl/p2g4+R/1lp1sp1G1/1b1l5/P1gl5/1P1s1N2K/2Pk5/4PPNP+B/3P2s1r b Ps5p 1",
        "lgn1s2gl/p2+p5/1gp1sp1G1/1s1l5/k1bl4+R/3n1N2K/1P6+B/2P1PPNP1/3P2s1r b P7p 1",
    ]
    .iter()
    .map(|s| decode_position(s).unwrap())
    .collect()
}

#[library_benchmark]
#[bench::default(setup_near_mate())]
fn bench_near_mate(positions: Vec<PositionAux>) {
    for position in &positions {
        black_box(low_mem_standard_solve(black_box(position.clone()), 1, true).unwrap());
    }
}

// ---- groups -----------------------------------------------------------------

library_benchmark_group!(
    name = advance_group;
    benchmarks = bench_black_advance, bench_white_advance
);

library_benchmark_group!(
    name = position_group;
    benchmarks = bench_reachable, bench_attacker, bench_canonicalize
);

library_benchmark_group!(
    name = solve_group;
    benchmarks = bench_near_mate
);

main!(
    library_benchmark_groups = advance_group,
    position_group,
    solve_group
);
