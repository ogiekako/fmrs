use std::ffi::CString;

use super::{sfen_to_ssdata, ssdata_t};

#[derive(Debug, Clone, Default)]
pub struct Ssdata(pub(super) ssdata_t);

impl Ssdata {
    pub fn from_sfen(sfen: &str) -> Self {
        let sfen = CString::new(sfen).unwrap();
        let mut ssdata = Ssdata::default();
        unsafe {
            sfen_to_ssdata(sfen.as_ptr() as *mut i8, &mut ssdata.0);
        }
        ssdata
    }
}
