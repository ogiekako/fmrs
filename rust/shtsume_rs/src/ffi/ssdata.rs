use std::ffi::CString;

use super::{komainf::Komainf, mkey::Mkey, sfen_to_ssdata, ssdata_t};

#[derive(Debug, Clone)]
#[repr(C)]
pub struct Ssdata(pub(super) ssdata_t);

impl Ssdata {
    pub fn from_sfen(sfen: &str) -> Self {
        let n = sfen.split(" ").count();
        let sfen = if n < 3 {
            panic!("Invalid SFEN: {}", sfen)
        } else if n == 3 {
            CString::new(format!("{} 1", sfen)).unwrap()
        } else {
            CString::new(sfen).unwrap()
        };

        let mut ssdata = Ssdata(ssdata_t::default());
        unsafe {
            sfen_to_ssdata(sfen.as_ptr() as *mut i8, &mut ssdata.0);
        }
        ssdata
    }

    pub fn board(&self) -> &[Komainf; 81] {
        unsafe { std::mem::transmute(&self.0.board) }
    }

    pub fn mkey(&self) -> &[Mkey; 3] {
        unsafe { std::mem::transmute(&self.0.mkey) }
    }

    pub fn turn(&self) -> u8 {
        self.0.turn
    }
}
