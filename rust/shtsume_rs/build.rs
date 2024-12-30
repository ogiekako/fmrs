use std::path::{self, PathBuf};

fn main() {
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());

    let shtusme_source = path::absolute("../../shtsume/source")
        .unwrap()
        .canonicalize()
        .unwrap();

    let (headers, sources): (Vec<_>, Vec<_>) = std::fs::read_dir(&shtusme_source)
        .unwrap()
        .map(|e| e.unwrap().path())
        .partition(|f| f.extension() == Some("h".as_ref()));

    bindgen::Builder::default()
        .headers(headers.iter().map(|p| p.to_str().unwrap()))
        .allowlist_file(format!("{}.*", shtusme_source.to_str().unwrap()))
        .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()))
        .derive_default(true)
        .generate()
        .unwrap()
        .write_to_file(out_dir.join("bindings.rs"))
        .unwrap();

    let mut files = vec![];

    for source in sources {
        if source.extension() != Some("c".as_ref()) {
            continue;
        }

        println!("cargo:rerun-if-changed={}", source.to_str().unwrap());

        if source.file_name().unwrap() == "nmain.c" {
            let contents = std::fs::read_to_string(source)
                .unwrap()
                .replace("int main(", "int shtsume_main(");

            let nf = out_dir.join("nmain.c");
            std::fs::write(&nf, contents).unwrap();
            files.push(nf);
            continue;
        }
        files.push(source);
    }

    cc::Build::new()
        .files(files)
        .flag("-std=c11")
        .flag("-Wall")
        .flag("-O3")
        .flag(format!("-I{}", shtusme_source.to_str().unwrap()))
        .warnings(false)
        .cargo_warnings(false)
        .compile("shtsume");
}
