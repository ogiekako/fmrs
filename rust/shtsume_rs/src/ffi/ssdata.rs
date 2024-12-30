use std::ffi::CString;

use super::{sfen_to_ssdata, ssdata_t};

#[derive(Debug, Clone)]
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
}
