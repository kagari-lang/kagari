//! Compile the bounded registration/lifetime proof before production adapters.
use std::{fs, path::PathBuf, process::Command};

#[test]
fn registration_and_scope_lifetime_proof() {
    let fixtures = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/interop_api");
    let output = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/interop-api-proof");
    fs::create_dir_all(&output).unwrap();
    let compiler = std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into());
    let cases = [
        ("pass.rs", None),
        ("scope_escape.rs", Some("lifetime may not live long enough")),
        ("root_reaccess.rs", Some("E0506")),
        ("root_move.rs", Some("E0505")),
        ("callback_escape.rs", Some("E0277")),
        ("callback_retain.rs", Some("E0521")),
        ("shared_upgrade.rs", Some("E0308")),
    ];
    for (case, diagnostic) in cases {
        let binary = output.join(case.trim_end_matches(".rs"));
        let result = Command::new(&compiler)
            .args(["--edition=2024", "-A", "dead_code"])
            .arg(fixtures.join(case))
            .arg("-o")
            .arg(&binary)
            .output()
            .unwrap();
        let stderr = String::from_utf8_lossy(&result.stderr);
        if let Some(diagnostic) = diagnostic {
            assert!(!result.status.success(), "{case} unexpectedly compiled");
            assert!(stderr.contains(diagnostic), "{case}: {stderr}");
        } else {
            assert!(result.status.success(), "{case}: {stderr}");
            assert!(Command::new(&binary).status().unwrap().success());
        }
    }
}
