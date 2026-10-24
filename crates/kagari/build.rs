//! Track example discovery and unpublished-workspace contracts for persisted native code.
#[cfg(feature = "aot-load")]
use blake3::Hasher;
#[cfg(any(feature = "aot-load", feature = "source"))]
use std::{env, path::PathBuf};
#[cfg(feature = "aot-load")]
use std::{fs, path::Path};

#[cfg(feature = "aot-load")]
fn collect(path: &Path, paths: &mut Vec<PathBuf>) {
    println!("cargo:rerun-if-changed={}", path.display());
    for entry in fs::read_dir(path).expect("contract source directory") {
        let path = entry.expect("source entry").path();
        if path.is_dir() {
            collect(&path, paths);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            paths.push(path);
        }
    }
}

fn main() {
    #[cfg(feature = "source")]
    {
        let manifest = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("manifest"));
        for relative in ["../../examples/syntax", "tests/fixtures/language"] {
            let directory = manifest.join(relative);
            if directory.is_dir() {
                // Explicit rerun rules replace the package scan; rustc tracks Rust sources.
                println!("cargo:rerun-if-changed=build.rs");
                println!("cargo:rerun-if-changed={}", directory.display());
            }
        }
    }
    #[cfg(feature = "aot-load")]
    fingerprint();
}

#[cfg(feature = "aot-load")]
fn fingerprint() {
    let manifest = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("manifest"));
    let root = manifest
        .parent()
        .expect("crates")
        .parent()
        .expect("workspace");
    let mut paths = vec![
        root.join("Cargo.toml"),
        root.join("Cargo.lock"),
        manifest.join("build.rs"),
    ];
    for name in [
        "kagari-abi",
        "kagari-contract",
        "kagari-common",
        "kagari-types",
        "kagari-bytecode",
        "kagari-runtime",
        "kagari-vm",
        "kagari-mir",
        "kagari-compiler",
        "kagari-codegen",
        "kagari-codegen-cranelift",
        "kagari",
    ] {
        let dir = root.join("crates").join(name);
        paths.push(dir.join("Cargo.toml"));
        collect(&dir.join("src"), &mut paths);
    }
    paths.sort();
    let mut hash = Hasher::new_derive_key("kagari unpublished native build contract");
    for path in paths {
        println!("cargo:rerun-if-changed={}", path.display());
        let name = path
            .strip_prefix(root)
            .expect("workspace path")
            .to_string_lossy()
            .replace('\\', "/");
        let source = fs::read_to_string(&path)
            .expect("contract source")
            .replace("\r\n", "\n");
        for field in [name.as_bytes(), source.as_bytes()] {
            hash.update(&(field.len() as u64).to_le_bytes());
            hash.update(field);
        }
    }
    println!(
        "cargo:rustc-env=KAGARI_NATIVE_BUILD_CONTRACT={}",
        hash.finalize().to_hex()
    );
}
