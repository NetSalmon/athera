use std::path::PathBuf;

const MANIFEST_DIR: &str = env!("CARGO_MANIFEST_DIR");
const TIME_FORMAT: &str = "%a %b %d %H:%M:%S %Z %Y";

fn main() {
    let config = load_config();

    if let Some(kernel) = config.get("kernel").and_then(toml::Value::as_table) {
        for (key, value) in kernel {
            println!("cargo:rustc-env={}={}", key, value);
        }
    }

    let smp = config
        .get("build")
        .and_then(|b| b.get("smp"))
        .and_then(|v| v.as_str())
        .unwrap_or("UP")
        .to_string();

    let build_number = config
        .get("build")
        .and_then(|b| b.get("number"))
        .and_then(|v| v.as_integer());

    release(&smp, build_number);
    version();

    let link = PathBuf::from(MANIFEST_DIR).join("linker.ld");
    println!("cargo:rustc-link-arg=-T{}", link.display());
    println!("cargo:rerun-if-changed={}", link.display());
}

fn load_config() -> toml::Table {
    let config_path = PathBuf::from(MANIFEST_DIR).join("config.toml");
    println!("cargo:rerun-if-changed={}", config_path.display());

    let Ok(content) = std::fs::read_to_string(&config_path) else {
        return toml::Table::new();
    };

    content.parse::<toml::Table>().unwrap_or_else(|e| {
        panic!("failed to parse config.toml: {e}");
    })
}

fn code(build_number: Option<i64>) -> u64 {
    if let Ok(v) = std::env::var("BUILD_NUMBER") {
        return v.parse().expect("BUILD_NUMBER must be a valid integer");
    }

    if let Some(n) = build_number {
        return n as u64;
    }

    let version_path = PathBuf::from(MANIFEST_DIR).join(".version");
    let content = std::fs::read_to_string(&version_path)
        .unwrap_or(String::from("0"))
        .parse::<u64>()
        .unwrap_or(0);

    std::fs::write(version_path, format!("{}", content + 1)).unwrap();

    content
}

fn version() {
    println!("cargo:rustc-env=VERSION={}", env!("CARGO_PKG_VERSION"));

    let sys = std::env::var("SYS").unwrap_or_else(|_| env!("CARGO_PKG_NAME").to_string());
    println!("cargo:rustc-env=SYS={sys}");

    let arch = std::env::var("ARCH").unwrap_or_else(|_| {
        std::env::var("TARGET")
            .unwrap_or_else(|_| String::from("riscv64gc"))
            .split('-')
            .next()
            .unwrap_or("riscv64gc")
            .to_string()
    });
    println!("cargo:rustc-env=ARCH={arch}");
}

fn release(smp: &str, build_number: Option<i64>) {
    let now = chrono::Utc::now();
    let s = now.format(TIME_FORMAT).to_string();
    let info = format!("#{} {smp} {s}", code(build_number));
    println!("cargo:rustc-env=RELEASE={}", info);
}
