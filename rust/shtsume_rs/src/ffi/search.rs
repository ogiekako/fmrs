use super::{bn_search, sdata::Sdata, tbase::Tbase, tdata::Tdata};

pub fn search(sdata: &Sdata, tdata: &mut Tdata, tbase: &mut Tbase) {
    unsafe { bn_search(&sdata.0, &mut tdata.0, tbase.0) }
}
