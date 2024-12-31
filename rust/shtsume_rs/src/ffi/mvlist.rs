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

    pub fn is_empty(&self) -> bool {
        self.0.is_null()
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

pub fn generate_evasion(sdata: &Sdata, tbase: &mut Tbase, allow_mudaai: bool) -> Mvlist {
    Mvlist::new(unsafe { super::generate_evasion2(&sdata.0, tbase.0, allow_mudaai) })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ffi::{ssdata::Ssdata, Global};

    #[test]
    fn test_generate_check() {
        let _g = Global::init(0, None);

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

    #[test]
    fn test_generate_evasion() {
        let _g = Global::init(0, None);

        for (sfen, want) in [
            ("7pk/7l1/8L/7N1/9/9/9/9/9 w 2r2b4g4s3n2l17p", 7),
            ("9/9/9/9/4k4/3PL4/9/9/9 w 2r2b4g4s4n3l17p", 6),
            ("RBG6/SSG6/8b/9/9/9/1Gn4l1/SG3s3/k7R w 3n3l18p", 11),
        ] {
            let ssdata = Ssdata::from_sfen(sfen);
            let sdata = Sdata::from_ssdata(&ssdata);
            let mut tbase = Tbase::default();
            let mvlist = generate_evasion(&sdata, &mut tbase, true);

            let mut got = 0;
            for item in mvlist.iter() {
                got += item.mlist().count();
            }
            assert_eq!(got, want);
        }
    }
}
