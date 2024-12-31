use super::{mlist::Mlist, mvlist_t, sdata::Sdata, tbase::Tbase};

#[derive(Debug)]
pub struct Mvlist(*mut mvlist_t);

impl Mvlist {
    fn new(inner: *mut mvlist_t) -> Self {
        Self(inner)
    }

    pub fn iter(&self) -> MvlistItemIter {
        MvlistItemIter(self.0)
    }
}

impl Drop for Mvlist {
    fn drop(&mut self) {
        unsafe {
            super::mvlist_free(self.0);
        }
    }
}

pub struct MvlistItemIter(*const mvlist_t);

impl Iterator for MvlistItemIter {
    type Item = MvlistItem;

    fn next(&mut self) -> Option<Self::Item> {
        if self.0.is_null() {
            return None;
        }
        let item = MvlistItem(self.0);

        self.0 = unsafe { (*self.0).next };

        Some(item)
    }
}

pub struct MvlistItem(*const mvlist_t);

impl MvlistItem {
    pub fn mlist(&self) -> Mlist {
        Mlist::new(unsafe { (*self.0).mlist })
    }
}

pub fn generate_check(sdata: &Sdata, tbase: &mut Tbase) -> Mvlist {
    Mvlist::new(unsafe { super::generate_check(&sdata.0, tbase.0) })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ffi::{ssdata::Ssdata, Global};

    #[test]
    fn test_generate_check() {
        let _g = Global::init(0);

        let ssdata = Ssdata::from_sfen("4k4/9/4P4/9/9/9/9/9/9 b G2r2b3g4s4n4l17p");
        let sdata = Sdata::from_ssdata(&ssdata);
        let mut tbase = Tbase::default();
        let mvlist = generate_check(&sdata, &mut tbase);

        let mut count = 0;
        for item in mvlist.iter() {
            count += item.mlist().count();
        }
        assert_eq!(count, 2 + 5);
    }
}
