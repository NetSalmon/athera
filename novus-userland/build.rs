use std::{env, fs, path::PathBuf};

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());

    // Generate a linker fragment that sets the high base address
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let base_ld = out_dir.join("base.ld");
    fs::write(&base_ld, "BASE_ADDRESS = 0xffffffc00000000;\n").unwrap();
    println!("cargo:rustc-link-arg=-T{}", base_ld.display());

    let ld_path = manifest_dir.join("src/linker.ld");
    println!("cargo:rustc-link-arg=-T{}", ld_path.display());
    println!("cargo:rerun-if-changed={}", ld_path.display());
}
