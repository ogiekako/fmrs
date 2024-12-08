use fmrs_core::{
    piece::Color,
    position::{advance, checked_slow, AdvanceOptions, Position},
};
use rustc_hash::{FxHashMap, FxHashSet};

pub fn one_way_mate_steps(position: &Position) -> Option<usize> {
    let mut position = position.clone();
    if checked_slow(&position, Color::White) {
        return None;
    }

    let mut visited = FxHashSet::default();

    let options = {
        let mut options = AdvanceOptions::default();
        options.max_allowed_branches = Some(1);
        options
    };

    let mut hashmap = FxHashMap::default();

    // TODO: `advance` without cache.
    for step in (1..).step_by(2) {
        hashmap.clear();
        let (white_positions, _) = advance(&position, &mut hashmap, step, &options).ok()?;
        debug_assert!(white_positions.len() <= 1);
        if white_positions.len() != 1 {
            return None;
        }

        hashmap.clear();
        let (mut black_positions, is_mate) =
            advance(&white_positions[0], &mut hashmap, step + 1, &options).ok()?;

        if is_mate && !white_positions[0].pawn_drop() {
            if !white_positions[0].hands().is_empty(Color::Black) {
                return None;
            }
            return (step as usize).into();
        }

        debug_assert_eq!(black_positions.len(), 1);

        if !visited.insert(position.digest()) {
            return None;
        }

        position = black_positions.remove(0);
    }
    unreachable!();
}

#[cfg(test)]
mod tests {
    use rand::{rngs::SmallRng, Rng, SeedableRng};

    use super::*;
    use crate::command::one_way_mate::action::Action;

    #[test]
    fn test_one_way_mate_steps() {
        let mut rng = SmallRng::seed_from_u64(0);

        let mut got: Vec<usize> = vec![];
        let mut got_oneway_positions = vec![];
        for _ in 0..3 {
            let mut sum_steps = 0;
            let mut position =
                Position::from_sfen("4k4/9/9/9/9/9/9/9/4K4 b 2r2b4g4s4n4l18p 1").unwrap();

            for _ in 0..1_000_000 {
                let action = random_action(&mut rng);
                if action.try_apply(&mut position).is_err() {
                    continue;
                }
                if let Some(steps) = one_way_mate_steps(&position) {
                    sum_steps += steps;
                    eprintln!("{:?}", position);
                    got_oneway_positions.push(position.to_sfen());
                }
            }
            got.push(sum_steps);
        }

        let want_oneway_positions = [
            "9/5S+LG1/1s2G4/2G3+bB1/2KNg1S2/+L5+l2/1L7/6RR1/k+p6s b 3n17p",
            "4lL2k/+R3G4/+N2b2+P1K/2S6/3+l5/5ps2/+n6+B1/5s3/2nr5 b 3gsnl16p",
            "1+Rs+N5/9/5K3/1R2+s4/p2+b1g2p/k3+p1+B2/g3G+s2P/9/9 b gs3n4l14p",
            "2P6/1R2+r4/+b2G5/+s2sP2K1/4G4/2n2P2k/1N3+l3/2sp+N1+LL1/9 b b2gsnl14p",
            "1k7/9/1K5N+L/N8/2+s1+P1P2/1gp+nR4/4P1n2/1+s+s+R5/1l1+p+p2+pP b 2b3gs2l10p",
            "1k7/9/1K5N+L/N8/2+s1+P1P2/1gp+nR4/4P1n2/1+s+s+R5/1l1+p+p2+pP b 2b3gs2l10p",
            "1k3P3/9/1K5N+L/N8/2+s1+P1P2/1gp+nR4/6n2/1+s+s+R5/1l1+p+p2+pP b 2b3gs2l10p",
            "3+R5/5P1G1/l8/k1K6/N4+l3/+B8/5+n2+p/1+N7/1p1+l5 b rb3g4snl15p",
            "1p1+l5/3+R5/5P3/l8/k1K6/N4+l3/+B8/5+n1B+p/1+N7 b r4g4snl15p",
            "Np+b4+r1/1g3p2N/3l+P4/7b1/+p1L1P4/k1K1+L4/3P5/2+SG5/3n1N3 b r2g3sl12p",
            "Np+b2b1+r1/1g3p2N/3l+P3g/9/+p1L1P4/k1K1+L1s2/3P5/2+SG5/3n1N3 b rg2sl12p",
            "Np+b2b1+r1/1g3p2N/3l+P3g/9/+p1L1P4/k1K1+L1s2/3P5/2+SG5/5N3 b rg2snl12p",
            "5N3/Np+b2b1+r1/1g3p2N/3l+P3g/9/+p1L1P4/k1K1+L1s2/3P5/2+SG5 b rg2snl12p",
            "5N3/Np+b2b1+r1/1g3p2N/3l+P3g/9/+p1L1P4/k1KN+L1s2/3P5/2+SG5 b rg2sl12p",
            "Np+b2b1+r1/1g3p2N/3l+P3g/9/+p1L1P4/k1KN+L1s2/3P5/2+SG5/5N3 b rg2sl12p",
            "3N5/1LG1+S4/9/4+R3s/+l8/g2+N4+l/1G1+RL1K1k/1p7/6+P2 b 2bg2s2n16p",
            "+p+N6k/4K4/5N1P1/1+l1b3L+P/8g/p8/3+s2+b1r/P2G+PP3/4l4 b r2g3s2nl11p",
            "2G+p4k/9/4+PP1K1/3L+s3N/8+p/+R8/2+r5+n/+lgN2s3/+L3+p4 b 2b2g2snl13p",
            "3r+L3S/7G1/2l1Nn2+p/2+l5B/P2G4+N/9/2B2+L3/k1+N1K4/g2psR3 b g2s15p",
            "7G1/2l1Nn2+p/2+l5B/P2G4+N/9/2B2+L3/k1+N1K4/g2psR3/3r+L3S b g2s15p",
            "2G4G1/2l1Nn2+p/2+l5B/P7+N/9/2B2+L3/k1+N1K4/g2psR3/3r+L3S b g2s15p",
            "1Gp1k+b3/1+S3+p3/2+SL4P/2K6/+s1n2s3/4+p2+P1/9/3p1+P3/1n6+b b 2r3g2n3l11p",
            "2Gp1k+b2/2+S3+p2/P2+SL4/3K5/1+s1n2s2/5+p2+P/9/4p1+P2/+b1n6 b 2r3g2n3l11p",
            "2Gp1k+b2/2+S3+p2/3+SL4/3K5/1+s1n2s2/5+p2+P/9/4p1+P2/+b1n6 b 2r3g2n3l12p",
            "2Gp1k+b2/2+S3+p2/3+SL1r2/3K5/1+s1n2s2/5+p2+P/9/4p1+P2/+b1n6 b r3g2n3l12p",
            "2np1k+b2/2+S3+p2/3+SL1r2/3K5/1+s1n2s2/5+p2+P/9/4p1+P2/+b1G6 b r3g2n3l12p",
            "1np1k+b3/1+S3+p3/2+SL1r3/2K6/+s1n2s3/4+p2+P1/9/3p1+P3/1G6+b b r3g2n3l12p",
            "4l3+N/3K3gr/6p2/2P6/2+P1+L+S3/+P8/1gS3+lp1/3B+n4/1+P1k3n1 b rb2g2snl12p",
            "4l3+N/3K3gr/6p2/2P6/2+P1+L+S3/+P8/1gS3+lp1/3B+n4/1+P1k5 b rb2g2s2nl12p",
            "9/4+S1+B2/9/7+sP/3+P2+S2/1+S4p1+l/2K2+l3/+p1B6/2k1G+p+RR1 b 3g4n2l13p",
            "2K1s1G2/4B3N/lr4+P1p/1ns+p1P3/3r1l2k/1P6B/9/2G2+sl1G/1+n4+p1+P b gsnl11p",
            "2K1s1G2/4B3N/l5+P1p/1ns+p1P3/3r1l2k/1P1r4B/9/2G2+sl1G/1+n4+p1+P b gsnl11p",
            "2K1s1G2/4B3N/l5+P1p/1ns+p1P3/3r+Sl2k/1P1r4B/9/2G2+sl1G/1+n4+p1+P b gnl11p",
            "+L4+p3/K1P3g2/1Pb+r5/1P4L2/1p4+P2/1+p4+P1k/n+bp1+S+s3/5G2N/4S2Ns b r2gn2l9p",
            "9/3p3R1/2+P1+p4/3G+S2+n+p/k1Gl+S1sN1/4+Bl3/1K7/4B2P1/3l5 b r2gs2nl13p",
            "7+N1/+p2gB4/Np7/K5l2/S1R1Lp3/p1Ppl4/8R/2+SnS4/4k1+S2 b b3gnl12p",
            "2+p6/P1S1n4/P+p1lgG3/2BS3G1/+p1+N6/5+p2B/8k/n1+N+l1g+SP1/8K b 2rs2l11p",
            "P1S6/P+pslgG3/3S5/+p1+N1gn3/5+p3/G7k/n1+N+l2+SP1/B+p6K/2+p6 b 2rb2l10p",
            "6p+P1/n5+L2/GR3+Bl+n1/g8/1+P+l2N3/3+s2+r2/5L3/2K3+n2/kB7 b 2g3s15p",
            "6p+P1/n8/GR3+Bl+n1/g8/1+P+l2N3/3+s1+L+r2/5L3/2K3+n2/kB7 b 2g3s15p",
            "6p+P1/n8/GR3+Bl2/3S5/1+P+l2N3/3+s1+L+r2/5L3/2K2s+n2/kB1+n5 b 3gs15p",
            "k1K4p+r/5+S2n/2+lB4P/1+p1s1g3/9/+b+S1+p1R1N1/n1+p3g2/1+P6g/4+P4 b gsn3l11p",
            "B4n1Pk/9/4n1nL1/1G1B4+l/4S4/3+r3L1/6+N1l/7g1/KP3G2+p b rg3s15p",
            "B4n1Pk/9/4n1nL1/1G1B4+l/4S4/3+r3L1/5s+N1l/7g1/KP3G2+p b rg2s15p",
            "B4n1Pk/9/4n1nL1/1G1B4+l/9/3+r3L1/5s+NSl/7g1/KP3G2+p b rg2s15p",
            "B4n1Pk/9/4n1nL1/1G1B4+l/8g/3+r3L1/5s+NSl/9/KP3G2+p b rg2s15p",
            "B4n1Pk/9/3Gn1nL1/1G1B4+l/8g/3+r3L1/5s+NSl/9/KP3G2+p b r2s15p",
            "6L1g/pgs4n1/6P2/4+r1s2/+RL7/9/3+L4n/2K2+s3/+P1G1kN+L2 b 2bgsn15p",
            "6L1g/pgs4n1/4+P1P2/4+r1s2/+RL7/9/3+L4n/2K2+s3/2G1kN+L2 b 2bgsn15p",
            "9/1R3+l3/3+pp4/5G3/5+S1+S1/+pn+LR1K1S+B/+N8/1b6n/1+N4Lkg b 2gsl15p",
            "l4R3/6b2/+N4L1+S1/1Kl5S/G5G1k/5g3/R1PS2+pB1/1gn1s4/2+P2+p3 b 2nl14p",
            "2+P2+p3/l4R3/6b2/+N4L1+S1/1Kl5S/G5G1k/5g3/R1PS2+pB1/1gn1s4 b 2nl14p",
            "2+P2+p3/l4R3/6b2/+N4L3/1Kl5S/G5G1k/5g3/R1PS2+pB1/1gn1s4 b s2nl14p",
            "1G1+p+P+p3/l3RR3/6b2/+N4s3/1Kl5S/G5+P1k/5g3/2PS2+pB1/+Sgn1L4 b 2nl12p",
            "1G1+p+P+p3/l3RR3/6b2/+N2l1s3/1K6S/G5+P1k/5g3/2PS2+pB1/+Sgn1L4 b 2nl12p",
            "k5+S1g/N8/1P1N5/8S/+lL1P1K3/R5+P1+R/4+p4/5G3/1+L7 b 2b2g2s2nl14p",
            "4+r4/5l3/1p5S1/N8/P2+n1+S+p2/k1+S1p+r3/NP3s3/+Lp+P1+b4/B5K1n b 4g2l11p",
            "1+l7/2+B4+L1/6n2/7+S1/2r1+L4/2p2+p3/2PS+N1+s1K/4L1+p2/2+b1+r3k b 4gs2n14p",
            "1+l7/2+B4+L1/6n2/7+S1/2r1+L4/2p2+p3/3S+N1+s1K/4LP+p2/2+b1+r3k b 4gs2n14p",
            "R8/9/K2+SbN3/1+n2+l4/2n+l2p2/l4+r3/1n3+l+B2/S4+S+P+S1/k1G6 b 3g16p",
            "+s2+P3+n1/9/2n2L+S+nP/1PP6/3s2p1B/1+P1G+B3r/p1K6/4p2P1/1+l1k1+r1N1 b 3gs2l9p",
            "9/9/k2P+pL3/2K5g/8P/3+l2g2/3B+Rp2p/3b5/1L2+r1+p2 b 2g4s4nl12p",
            "9/9/k2P+pL3/1LK5g/8P/3+l2g2/3B+Rp2p/3b5/1L2+r1+p2 b 2g4s4n12p",
            "6+Lg+P/2+L5n/1r1P5/9/9/1+SB1G4/kB3+l3/2K1+s+N+s2/5s2+P b r2g2nl15p",
            "6+Lg+P/2+L5n/1r1P5/9/9/1+SB1G4/kB3+l3/2K1+s+N+s2/5s2+P b r2g2nl15p",
            "2+L5n/1r1P5/9/9/1+SB1G4/kB3+l3/2K1+s+N+s2/5s2+P/6+Lg+P b r2g2nl15p",
            "2+L5n/1r1P5/9/9/1+SB1G4/kB3+l3/2K1+s1+s2/5s2+P/6+Lg+P b r2g3nl15p",
            "6n2/9/4+l4/1r3S3/+l1n6/G3+L3+R/1p2K4/S8/k1+N+P1bn+b1 b 3g2sl16p",
            "6n2/9/4+l4/1r3S3/+l1n5+b/G3+L3+R/1p2K4/S8/k1+N+P1bn2 b 3g2sl16p",
            "6n2/9/4+l4/1r3S3/+l7+b/G3+L3+R/1p2K4/S8/k1+N+P1bn2 b 3g2snl16p",
            "3n2n2/9/4+l4/1r3S3/+l7+b/G3+L3+R/1p2K4/S8/k1+N+P1b3 b 3g2snl16p",
            "3n2n2/9/4+ls3/1r3S3/+l7+b/G3+L3+R/1p2K4/S8/k1+N+P1b3 b 3gsnl16p",
            "3n2n2/9/4+ls3/5S3/+l7+b/G3+L3+R/1p2K4/S8/k1+N+P1b3 b r3gsnl16p",
            "2+B6/kPl6/6+b2/1K4+P1+P/+p8/1pG4+R1/p2+n5/1l2+s1s1R/2S1+l1G2 b 2gs3nl12p",
            "kPl6/2+P3+b2/1K6+P/+p8/1pG4+R1/p2+n5/1l2+s1s1R/2S1+l1G2/2+B6 b 2gs3nl12p",
            "1s5+Lg/2N1+n4/5+n3/2p6/5P2s/s7k/n6L+p/K3+B4/2S1Gl+p+R1 b rb2gl14p",
            "1s5+Lg/2N1+n4/4p+n3/9/5P2s/s7k/n6L+p/K3+B4/2S1Gl+p+R1 b rb2gl14p",
            "r3kB1G+n/6+bP1/2P1Kl3/2l2n3/9/+P+L5+s1/6g2/1p7/s8 b r2g2s2nl14p",
            "1P1ns2g1/1+s1+s5/5r2p/2+l4sg/3N1+p1g1/nNp2Bkp1/1+l1+RG4/3p1b1K1/9 b 2l12p",
            "2P1ns2g/2+s1+s4/p5r2/g2+l4s/4N1+p1g/1nNp2Bkp/2+l1+RG3/4p1b1K/9 b 2l12p",
            "2P1ns2g/2+s1+s4/p5r2/g7s/4N1+p1g/1nNp2Bkp/2+l1+RG3/4p1b1K/9 b 3l12p",
            "2+s1+s4/p5r2/g7s/4N1+p1g/1nNp2Bkp/2+l1+RG3/4p1b1K/9/2P1ns2g b 3l12p",
            "2+s1+s2+l1/p5r2/g7s/4N1+p1g/1nNp2Bkp/4+RG3/4p1b1K/9/2P1ns2g b 3l12p",
            "2+s1+s2+l1/p5r2/g7s/4N1+p1g/1nNp2Bkp/4+RG3/4p1b1K/9/2P1n3g b s3l12p",
            "2P1n3g/2+s1+s2+l1/p5r2/g7s/4N1+p1g/1nNp2Bkp/4+RG3/4p1b1K/9 b s3l12p",
            "g2P1n3/3+s1+s2+l/1p5r1/sg7/g4N1+p1/p1nNp2Bk/5+RG2/K4p1b1/9 b s3l12p",
            "g2P1n3/3+s1+s3/1p5r1/sg7/g4N1+p1/p1nNp2Bk/5+RG2/K4p1b1/4+l4 b s3l12p",
            "4+l4/g2P1n3/3+s1+s3/1p5r1/sg7/g4N1+p1/p1nNp2Bk/5+RG2/K4p1b1 b s3l12p",
            "K4p1b1/4+l4/g2P1n3/3+s1+s3/1p5r1/sg7/g4N1+p1/p1nNp2Bk/5+RG2 b s3l12p",
            "1K1kp4/4N1+n2/1+Sb1+s1+N2/1g2+s4/2sp1+N3/1p1g4B/4+l+P3/6+lG1/2+p1g4 b 2r2l13p",
            "1K1kp4/4N1+n2/1+Sb1+s1+N2/1g2+s4/2sp1+N3/1p6B/4+l+P3/6+lG1/2+p1g4 b 2rg2l13p",
            "1K1kp4/4N1+n2/1+Sb1+s1+N2/1g2+s4/2sp1+N3/1p6B/5+P3/6+lG1/2+p1g4 b 2rg3l13p",
            "K1kp5/3N1+n3/+Sb1+s1+N3/g2+s5/1sp1+N4/p6B1/4+P4/5+lG2/1+p1g5 b 2rg3l13p",
            "K1kp5/3N1+n3/+Sb1+s1+N3/g8/1sp1+N4/p6B1/4+P4/5+lG2/1+p1g5 b 2rgs3l13p",
            "9/2+b2RK2/9/3+r1G3/k1G+n+pg3/lP7/8+s/2B1+s1L1G/2+S1n1N2 b sn2l16p",
            "9/1+S+R6/l1K1+p3+n/kN4b+N1/+pl7/8G/l8/7G1/1g7 b rbg3snl16p",
            "9/9/k1P6/1LK3L+l1/8+N/+rG6+n/2RB4+b/2p3s2/3P5 b 3g3s2nl15p",
            "9/9/k1P6/1LK3L+l1/8+N/+rG6+n/2RB4+b/2p3s2/3P5 b 3g3s2nl15p",
            "9/9/k1P6/1LK3L+l1/8+N/+rG6+n/2RB4+b/2p1n1s2/3P5 b 3g3snl15p",
            "K8/2P6/3G1+b3/6+P1k/1+B3l2g/4+sL1+pN/2+s2s1+N1/2+PL1+N3/5g2g b 2rsnl14p",
            "4+p1n2/+p1G2l2S/Pn4n2/5+p1+sL/+P5+R2/9/5n2L/2+PS5/3k1K2+b b rb3gsl12p",
            "8k/+s2G5/6G1K/1+Nn+s3g1/4p4/4b+s+l2/1L7/7sN/2p+R1G3 b rbn2l16p",
            "8k/+s2G5/6G1K/1+N1+s3g1/4p4/4b+s+l2/1L7/2n4sN/2p+R1G3 b rbn2l16p",
            "1p7/2K6/+l5+r2/1N2g4/p8/1+lS3+P1k/3P5/+p5+sBN/3+S3G+S b rb2g2n2l13p",
            "2K6/+l5+n2/S2N5/k4G+P1R/+s1Ss1RL2/9/6+B1L/4+Pg1np/3+N4L b b2g15p",
            "2K6/+l5+n2/S2N5/k4G+P1R/+s1Ss1RL2/9/6+B1L/4+Pg1np/3+N4L b b2g15p",
            "+Pg5+S1/b8/2r6/5Kn2/5P+R+N1/G+s1+l3s1/5+Ps+P1/B+P7/k8 b 2g2n3l13p",
            "+Pg5+S1/9/2r6/5Kn2/5P+R+N1/G+s1+l3s1/5+Ps+P1/B+P7/k8 b b2g2n3l13p",
            "8R/1Kl1P+lB1S/G+P3BN+l1/9/S8/6+s2/4s+R3/9/k2+l1+n2+p b 3g2n15p",
        ];

        pretty_assertions::assert_eq!(got_oneway_positions, want_oneway_positions);

        assert_eq!(got, vec![33, 22, 54]);
    }

    #[test]
    fn test_diamond() {
        let position = Position::from_sfen(include_str!("../../../problems/diamond.sfen")).unwrap();
        assert_eq!(one_way_mate_steps(&position), Some(55));
    }

    fn random_action(rng: &mut SmallRng) -> Action {
        loop {
            match rng.gen_range(0..100) {
                0..=9 => return Action::Move(rng.gen(), rng.gen()),
                10..=19 => return Action::Swap(rng.gen(), rng.gen()),
                20..=29 => return Action::FromHand(rng.gen(), rng.gen(), rng.gen(), rng.gen()),
                30..=39 => return Action::ToHand(rng.gen(), Color::White),
                40..=49 => return Action::Shift(rng.gen()),
                _ => (),
            }
        }
    }
}
