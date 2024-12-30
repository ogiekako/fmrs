use super::{initialize_sdata, is_sdata_illegal, sdata_t, ssdata::Ssdata};

#[derive(Clone, Default)]
pub struct Sdata(pub sdata_t);

impl Sdata {
    pub fn from_ssdata(ssdata: &Ssdata) -> Self {
        let mut sdata = Sdata::default();
        unsafe {
            initialize_sdata(&mut sdata.0, &ssdata.0);
        }
        sdata
    }

    pub fn is_illegal(&mut self) -> i32 {
        unsafe { is_sdata_illegal(&mut self.0) }
    }
}
