//! Bake `registry/*.toml` into the binary so an embedded registry item is
//! available without reading the source tree at runtime.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

fn main() {
    println!("cargo:rerun-if-changed=registry");
    let registry = Path::new("registry");
    let mut files = Vec::new();
    if registry.is_dir() {
        for entry in fs::read_dir(registry).expect("read registry directory") {
            let path = entry.expect("read registry entry").path();
            if path.extension().and_then(|ext| ext.to_str()) == Some("toml") {
                println!("cargo:rerun-if-changed={}", path.display());
                files.push(path);
            }
        }
    }
    files.sort();

    let mut code = String::from("pub static EMBEDDED: &[(&str, &str)] = &[\n");
    for path in files {
        let stem = path
            .file_stem()
            .expect("registry file stem")
            .to_string_lossy()
            .to_string();
        let absolute = absolute(&path);
        code.push_str(&format!(
            "    ({stem:?}, include_str!({path:?})),\n",
            path = absolute.to_string_lossy()
        ));
    }
    code.push_str("];\n");

    let out = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR"));
    fs::write(out.join("embedded.rs"), code).expect("write embedded registry");
}

fn absolute(path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        Path::new(&env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR")).join(path)
    }
}
