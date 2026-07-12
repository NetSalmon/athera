use std::{env, fs};
use std::path::{Path, PathBuf};

fn main() {
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let config_path = Path::new(&manifest_dir).join("config.toml");
    let content = fs::read_to_string(config_path).expect("未能找到 config.toml");

    let values: toml::Value = toml::from_str(content.as_str()).unwrap();

    if let Some(table) = values.as_table() {
        for (key, value) in table {
            println!("cargo:rustc-env={}={}", key, value);
            println!("set {}={}", key, value);
        }
    }

    let link = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("linker.ld");
    println!("cargo:rustc-link-arg=-T{}", link.display());
}
