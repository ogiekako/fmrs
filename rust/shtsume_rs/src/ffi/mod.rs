pub mod komainf;
pub mod mkey;
pub mod mlist;
pub mod move_;
pub mod mtt;
pub mod mvlist;
pub mod sdata;
pub mod search;
pub mod ssdata;
pub mod tbase;
pub mod tdata;

use std::{
    ffi::CString,
    sync::{Mutex, MutexGuard, Once},
};

mod inner {
    #![allow(
        non_upper_case_globals,
        non_camel_case_types,
        non_snake_case,
        unused,
        clippy::all
    )]
    include!(concat!(env!("OUT_DIR"), "/bindings.rs"));
}
use inner::*;

pub use inner::CHECKED;
pub use inner::ILL_POS;
pub use inner::MCARDS_PER_MBYTE;
pub use inner::NIFU;
pub use inner::TBASE_SIZE_DEFAULT;
pub use inner::TP_ALLMOVE;
pub use inner::TP_NONE;
pub use inner::TP_ZKEY;

use sdata::Sdata;
use tbase::Tbase;

#[link(name = "shtsume")]
extern "C" {
    fn shtsume_main(argc: i32, argv: *const *const u8);
    fn srand(seed: u32);
}

pub fn do_main(argv: &[&str]) {
    let _g = GLOBAL_MUX.lock().unwrap();

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

pub fn tsume_print(sdata: &Sdata, tbase: &Tbase, flag: u32) {
    unsafe { inner::tsume_print(&sdata.0, tbase.0, flag) };
}

static GLOBAL_MUX: Mutex<()> = Mutex::new(());

pub struct Global {
    _g: MutexGuard<'static, ()>,
}

impl Global {
    pub fn init(seed: u32, time_limit: Option<i32>) -> Global {
        let _g = GLOBAL_MUX.lock().unwrap();

        static ONCE: Once = Once::new();

        unsafe {
            ONCE.call_once(|| {
                create_seed();
                init_distance();
                init_bpos();
                init_effect();
                srand(seed);
            });
            g_time_limit = time_limit.unwrap_or(TM_INFINATE);
        }

        Global { _g }
    }
}

impl Drop for Global {
    fn drop(&mut self) {
        unsafe {
            mlist_free_stack();
            mvlist_free_stack();
        }
    }
}
