use rand::{rngs::StdRng, Rng, SeedableRng};
use std::env;
use std::fs;
use std::path::Path;

fn main() {
    let out_dir = env::var_os("OUT_DIR").unwrap();
    let dest_path = Path::new(&out_dir).join("zobrist_data.rs");

    let mut rng = StdRng::seed_from_u64(202412141622);
    let mut out = String::new();
    out.push_str("pub const M: [u64; 4096] = [\n");
    for _ in 0..4096 {
        out.push_str(&format!("    {},\n", rng.gen::<u64>()));
    }
    out.push_str("];\n");

    fs::write(&dest_path, out).unwrap();
    println!("cargo:rerun-if-changed=build.rs");
}
