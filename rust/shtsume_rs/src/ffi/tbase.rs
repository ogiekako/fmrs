use super::{create_tbase, destroy_tbase, tbase_t};

#[derive(Debug)]
pub struct Tbase(pub(super) *mut tbase_t);

impl Tbase {
    pub fn create(size: u64) -> Self {
        Self(unsafe { create_tbase(size) })
    }
}

impl Drop for Tbase {
    fn drop(&mut self) {
        unsafe {
            destroy_tbase(self.0);
        }
    }
}
