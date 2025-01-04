use fmrs_core::{
    memo::{Memo, MemoTrait},
    piece::{Color, Kind},
    position::{BitBoard, Position, PositionExt, Square},
    solve::{reconstruct::PositionTrait, reconstruct_solutions, Solution},
};
use shtsume_rs::ffi::{
    komainf::Komainf,
    mvlist::{generate_check, generate_evasion},
    sdata::Sdata,
    ssdata::Ssdata,
    tbase::Tbase,
    Global,
};

const PAWN_DROP_DIGEST: u64 = 0x5555_5555_5555_5555;

pub fn shtsume_solve(ssdata: &Ssdata, solutions_upto: usize) -> anyhow::Result<Vec<Solution>> {
    let _g = Global::init(0, None);

    let sdata = Sdata::from_ssdata(ssdata);

    let mut tbase = Tbase::create(1024 /* mb */);

    let mut black_visited: Memo = Default::default();
    black_visited.contains_or_insert(digest(&sdata, false), 0);
    let mut white_visited: Memo = Default::default();

    let mut black_sdata = vec![sdata];
    let mut white_sdata = vec![];

    let mut mate_sdata = vec![];

    for step in (1..).step_by(2) {
        if black_sdata.is_empty() {
            return Ok(vec![]);
        }

        for sdata in black_sdata.iter() {
            let moves = generate_check(sdata, &mut tbase);

            for mv in moves.iter().flat_map(|i| i.mlist()) {
                let mut np = sdata.clone();

                np.move_forward(mv);

                if white_visited
                    .contains_or_insert(digest(&np, mv.is_drop() && mv.hand() == Komainf::FU), step)
                {
                    continue;
                }

                white_sdata.push(np);
            }
        }
        black_sdata.clear();

        for sdata in white_sdata.iter() {
            let moves = generate_evasion(sdata, &mut tbase, true);

            if moves.is_empty() {
                mate_sdata.push(sdata.clone());
                continue;
            }

            for mv in moves.iter().flat_map(|i| i.mlist()) {
                let mut np = sdata.clone();
                np.move_forward(mv);

                if black_visited.contains_or_insert(
                    digest(&np, mv.is_drop() && mv.hand() == Komainf::FU),
                    step + 1,
                ) {
                    continue;
                }

                black_sdata.push(np);
            }
        }
        white_sdata.clear();

        if !mate_sdata.is_empty() {
            eprintln!("solution found: {}", step);

            let mut res = vec![];

            for mate in mate_sdata {
                res.append(&mut reconstruct_solutions(
                    &SdataPosition::new(mate, false),
                    &black_visited,
                    &white_visited,
                    solutions_upto - res.len(),
                ));
            }

            return Ok(res);
        }
    }
    unreachable!()
}

fn digest(sdata: &Sdata, pawn_drop: bool) -> u64 {
    let mut digest = sdata.zkey();
    digest ^= sdata.core().mkey()[0].as_u32() as u64;
    if pawn_drop {
        digest ^= PAWN_DROP_DIGEST
    }
    digest
}

fn ssdata_to_position(ssdata: &Ssdata) -> Position {
    let mut position = Position::default();

    position.set_turn(Color::from_is_black(ssdata.turn() == 0));
    let mut add = |c: Color, k: Kind, n| {
        for _ in 0..n {
            position.hands_mut().add(c, k);
        }
    };

    for i in 0..2 {
        let c = Color::from_is_black(i == 0);
        let m = &ssdata.mkey()[i];
        add(c, Kind::Pawn, m.fu());
        add(c, Kind::Lance, m.ky());
        add(c, Kind::Knight, m.ke());
        add(c, Kind::Silver, m.gi());
        add(c, Kind::Gold, m.ki());
        add(c, Kind::Bishop, m.ka());
        add(c, Kind::Rook, m.hi());
    }

    for (i, &k) in ssdata.board().iter().enumerate() {
        if k == Komainf::SPC {
            continue;
        }
        let rev_pos = Square::from_index(i);
        let pos = Square::new(rev_pos.row(), rev_pos.col());

        let kind = match k {
            Komainf::SFU | Komainf::GFU => Kind::Pawn,
            Komainf::SKY | Komainf::GKY => Kind::Lance,
            Komainf::SKE | Komainf::GKE => Kind::Knight,
            Komainf::SGI | Komainf::GGI => Kind::Silver,
            Komainf::SKI | Komainf::GKI => Kind::Gold,
            Komainf::SKA | Komainf::GKA => Kind::Bishop,
            Komainf::SHI | Komainf::GHI => Kind::Rook,
            Komainf::SOU | Komainf::GOU => Kind::King,
            Komainf::STO | Komainf::GTO => Kind::ProPawn,
            Komainf::SNY | Komainf::GNY => Kind::ProLance,
            Komainf::SNK | Komainf::GNK => Kind::ProKnight,
            Komainf::SNG | Komainf::GNG => Kind::ProSilver,
            Komainf::SUM | Komainf::GUM => Kind::ProBishop,
            Komainf::SRY | Komainf::GRY => Kind::ProRook,
            _ => unreachable!(),
        };

        position.set(
            pos,
            if k.sente() {
                Color::BLACK
            } else {
                Color::WHITE
            },
            kind,
        );
    }
    position
}

#[derive(Clone)]
struct SdataPosition {
    inner: Sdata,
    pawn_drop: bool,
}

impl SdataPosition {
    fn new(inner: Sdata, pawn_drop: bool) -> Self {
        Self { inner, pawn_drop }
    }
}

impl PositionTrait for SdataPosition {
    fn digest(&self) -> u64 {
        digest(&self.inner, self.pawn_drop)
    }

    fn to_position(&self) -> Position {
        let mut position = ssdata_to_position(self.inner.core());
        if self.pawn_drop {
            position.set_pawn_drop(true);
        }
        position
    }

    fn undo_digest(&self, undo_move: &fmrs_core::position::UndoMove) -> u64 {
        let mut position = self.to_position();
        position.undo_move(undo_move);
        let ssdata = Ssdata::from_sfen(&position.sfen());
        digest(&Sdata::from_ssdata(&ssdata), undo_move.was_pawn_drop())
    }

    fn undone(&self, undo_move: &fmrs_core::position::UndoMove) -> Self {
        let mut position = self.to_position();
        position.undo_move(undo_move);
        let ssdata = Ssdata::from_sfen(&position.sfen());
        SdataPosition::new(Sdata::from_ssdata(&ssdata), undo_move.was_pawn_drop())
    }

    fn stone(&self) -> Option<BitBoard> {
        None
    }
}
