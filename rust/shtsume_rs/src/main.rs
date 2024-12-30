extern crate shtsume_rs;
use shtsume_rs::ffi;

fn main() {
    let args = std::env::args().collect::<Vec<_>>();
    ffi::do_main(&args.iter().map(|s| s.as_str()).collect::<Vec<_>>());
}
