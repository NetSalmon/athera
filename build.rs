use std::path::PathBuf;

const MANIFEST_DIR: &str = env!("CARGO_MANIFEST_DIR");
const TIME_FORMAT: &str = "%a %b %d %H:%M:%S %Z %Y";
const CODE: &str = "0";

fn main() {
    use_config();
    build_info();
    version();

    let link = PathBuf::from(MANIFEST_DIR).join("linker.ld");
    println!("cargo:rustc-link-arg=-T{}", link.display());
}

fn use_config() {
    let config_path = PathBuf::from(MANIFEST_DIR).join("config.toml");

    let Ok(content) = std::fs::read_to_string(config_path) else {
        return;
    };

    let Ok(values) = content.parse::<toml::Value>() else {
        return;
    };

    if let Some(table) = values.as_table() {
        for (key, value) in table {
            println!("cargo:rustc-env={}={}", key, value);
        }
    }
}

fn version() {
    println!("cargo:rustc-env=VERSION={}", env!("CARGO_PKG_VERSION"));
}

fn build_info() {
    let now = chrono::Utc::now();

    let s = now.format(TIME_FORMAT).to_string();

    let info = format!("#{CODE} {s}");

    println!("cargo:rustc-env=BUILD_INFO={}", info);
}