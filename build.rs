use std::path::PathBuf;

fn main() {
    let link = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("linker.ld");
    println!("cargo:rustc-link-arg=-T{}", link.display());
}