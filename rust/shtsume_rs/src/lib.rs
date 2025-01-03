use anyhow::bail;
use ffi::{sdata::Sdata, search::search, ssdata::Ssdata, tbase::Tbase, tdata::Tdata};

pub mod ffi;

pub fn solve(sfen: &str) -> anyhow::Result<Option<u16>> {
    let _g = ffi::Global::init(0, None);

    let ssdata = Ssdata::from_sfen(sfen);
    let mut sdata = Sdata::from_ssdata(&ssdata);

    match sdata.is_illegal() as u32 {
        0 => (),
        ffi::CHECKED => bail!("checked"),
        ffi::NIFU => bail!("double pawn"),
        ffi::ILL_POS => bail!("unmovable piece"),
        _ => bail!("unknown error"),
    }

    let mut tbase = Tbase::default();
    let mut tdata = Tdata::default();

    search(&sdata, &mut tdata, &mut tbase);

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
    use std::os::fd::AsRawFd;

    use crate::{ffi, solve};

    #[test]
    fn test_main() {
        for sfen in [
            "3sks3/9/4+P4/9/9/8B/9/9/9 b S2rb4gs4n4l17p 1",
            "4k4/9/4P4/9/9/9/9/9/9 b G2r2b3g4s4n4l17p 1",
        ] {
            let dev_null = std::fs::File::open("/dev/null").unwrap();
            unsafe { libc::dup2(dev_null.as_raw_fd(), 1) };
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
            assert_eq!(solve(sfen).unwrap(), step);
        }
    }
}
