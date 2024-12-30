use super::{create_mtt, destroy_mtt, g_mtt, mtt_t};

#[derive(Debug)]
pub struct Mtt(pub(super) *mut mtt_t);

impl Mtt {
    pub fn create(size: u32) -> Self {
        Self(unsafe { create_mtt(size) })
    }

    pub fn set_global(&self) {
        unsafe {
            g_mtt = self.0;
        }
    }
}

impl Drop for Mtt {
    fn drop(&mut self) {
        unsafe {
            destroy_mtt(self.0);
        }
    }
}
