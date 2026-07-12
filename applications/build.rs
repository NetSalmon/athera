use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    // Generate a linker fragment that sets the high base address
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let base_ld = out_dir.join("base.ld");
    fs::write(&base_ld, "BASE_ADDRESS = 0xffffffc00000000;\n").unwrap();
    println!("cargo:rustc-link-arg=-T{}", base_ld.display());

    let ld_path = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap()).join("src/linker.ld");
    println!("cargo:rustc-link-arg=-T{}", ld_path.display());
}
