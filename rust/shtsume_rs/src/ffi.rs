use std::ffi::CString;

mod inner {
    #![allow(non_upper_case_globals, non_camel_case_types, non_snake_case, unused)]

    include!(concat!(env!("OUT_DIR"), "/bindings.rs"));
}
pub use inner::*;

#[link(name = "shtsume")]
extern "C" {
    fn shtsume_main(argc: i32, argv: *const *const u8);
    fn srand(seed: u32);
    pub fn time(seed: *mut i32) -> i32;
}

pub fn do_main(argv: &[&str]) {
    let mut argv = argv
        .iter()
        .map(|s| CString::new(*s).unwrap())
        .collect::<Vec<_>>();
    let ptrs = argv
        .iter_mut()
        .map(|s| s.as_ptr() as *const u8)
        .collect::<Vec<_>>();
    unsafe {
        shtsume_main(ptrs.len() as i32, ptrs.as_ptr());
    }
}

pub fn init(seed: u32) {
    unsafe {
        create_seed();
        init_distance();
        init_bpos();
        init_effect();
        srand(seed);
    }
}

#[derive(Debug, Clone, Default)]
pub struct Ssdata(ssdata_t);

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

#[derive(Debug)]
pub struct Tbase(pub *mut tbase_t);

impl Tbase {
    pub fn create(size: u64) -> Self {
        Self(unsafe { create_tbase(size) })
    }
}

// impl Drop for Tbase {
//     fn drop(&mut self) {
//         unsafe {
//             destroy_tbase(self.0);
//         }
//     }
// }

#[derive(Debug)]
pub struct Mtt(*mut mtt_t);

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

#[derive(Debug, Clone, Default)]
pub struct Tdata(tdata_t);

impl Tdata {
    pub fn pn(&self) -> u16 {
        self.0.pn
    }
    pub fn dn(&self) -> u16 {
        self.0.dn
    }
    pub fn sh(&self) -> u16 {
        self.0.sh
    }
}

pub fn search(sdata: &Sdata, tdata: &mut Tdata, tbase: &mut Tbase) {
    unsafe { bn_search(&sdata.0, &mut tdata.0, tbase.0) }
}
