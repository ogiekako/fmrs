use super::{
    initialize_sdata, is_sdata_illegal, move_::Move_, sdata_move_forward, sdata_t, ssdata::Ssdata,
    ssdata_t,
};

#[derive(Clone, Default)]
#[repr(C)]
pub struct Sdata(pub(super) sdata_t);

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

    pub fn move_forward(&mut self, move_: Move_) -> i32 {
        unsafe { sdata_move_forward(&mut self.0, move_.0) }
    }

    pub fn zkey(&self) -> u64 {
        self.0.zkey
    }

    pub fn core(&self) -> &Ssdata {
        unsafe { &*(&self.0.core as *const ssdata_t as *const Ssdata) }
    }
}
