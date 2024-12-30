use std::cell::OnceCell;

use anyhow::bail;

pub mod ffi;

pub fn solve(sfen: &str) -> anyhow::Result<Option<u16>> {
    unsafe {
        ffi::g_info_interval = 5;
        ffi::g_pv_length = 5;

        ffi::g_commandline = true;
    }

    const ONCE: OnceCell<()> = OnceCell::new();

    // ONCE.get_or_init(|| {
    ffi::init(0);
    // });

    let res = solve_inner(sfen);

    unsafe {
        ffi::initialize_tbase(ffi::g_tbase);
        ffi::init_mtt(ffi::g_mtt);
        ffi::mlist_free_stack();
        ffi::mvlist_free_stack();
    }

    res
}

fn solve_inner(sfen: &str) -> anyhow::Result<Option<u16>> {
    let ssdata = ffi::Ssdata::from_sfen(sfen);
    let mut sdata = ffi::Sdata::from_ssdata(&ssdata);

    unsafe { ffi::g_sdata = sdata.0 };

    match sdata.is_illegal() as u32 {
        0 => (),
        ffi::CHECKED => bail!("checked"),
        ffi::NIFU => bail!("double pawn"),
        ffi::ILL_POS => bail!("unmovable piece"),
        _ => bail!("unknown error"),
    }

    let size = ffi::TBASE_SIZE_DEFAULT as u64 * ffi::MCARDS_PER_MBYTE as u64 - 1;

    let mut g_tbase = ffi::Tbase::create(size);

    unsafe {
        ffi::g_tbase = g_tbase.0;
        ffi::g_mtt = ffi::create_mtt(ffi::MTT_SIZE);
    }
    unsafe { ffi::g_tbase = g_tbase.0 };
    unsafe { ffi::g_mtt = ffi::create_mtt(ffi::MTT_SIZE) };

    unsafe { ffi::g_time_limit = ffi::TM_INFINATE };

    let mut tdata = ffi::Tdata::default();

    ffi::search(&sdata, &mut tdata, &mut g_tbase);

    Ok(if tdata.pn() == 0 {
        Some(tdata.sh())
    } else if tdata.dn() == 0 {
        None
    } else {
        bail!("no result");
    })
}

#[cfg(test)]
mod tests {
    use crate::{ffi, solve};

    #[test]
    fn test_main() {
        for sfen in [
            "3sks3/9/4+P4/9/9/8B/9/9/9 b S2rb4gs4n4l17p 1",
            "4k4/9/4P4/9/9/9/9/9/9 b G2r2b3g4s4n4l17p 1",
        ] {
            let argv = ["shtsume", sfen];
            ffi::do_main(&argv);
        }
    }

    #[test]
    fn test_shtsume_solve() {
        for (sfen, step) in [
            ("4k4/9/4P4/9/9/9/9/9/9 b G2r2b3g4s4n4l17p 1", Some(1)),
            ("3sks3/9/4+P4/9/9/8B/9/9/9 b S2rb4gs4n4l17p 1", Some(3)),
            ("4k4/9/PPPPPPPPP/9/9/9/9/9/9 b B4L2rb4g4s4n9p 1", Some(11)),
        ] {
            ffi::do_main(&["shtsume", sfen]);
            assert_eq!(solve(sfen).unwrap(), step);
        }
    }
}
