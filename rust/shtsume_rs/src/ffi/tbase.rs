use super::{create_tbase, destroy_tbase, tbase_t};

#[derive(Debug)]
pub struct Tbase(pub(super) *mut tbase_t);

impl Default for Tbase {
    fn default() -> Self {
        let size = super::TBASE_SIZE_DEFAULT as u64 * super::MCARDS_PER_MBYTE as u64 - 1;
        Self::create(size)
    }
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create() {
        let _t = Tbase::default();
    }
}
