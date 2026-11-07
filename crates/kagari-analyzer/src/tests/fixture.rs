//! Fresh disk copies of the concrete two-root app/api acquisition fixture.
use crate::workspace::origins::file_uri;
use lsp_types::Uri;
#[cfg(unix)]
use std::os::unix::fs::symlink;
#[cfg(windows)]
use std::process::Command;
use std::{
    fs,
    io::{self, Error},
    path::{Path, PathBuf},
    process,
    sync::atomic::{AtomicU64, Ordering},
    thread,
};

static NEXT: AtomicU64 = AtomicU64::new(1);

pub(crate) struct Fixture {
    base: PathBuf,
    directory: PathBuf,
}

impl Fixture {
    pub(crate) fn new() -> Self {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let base = root.join("../../target/lsp-tests");
        fs::create_dir_all(&base).unwrap();
        let base = base.canonicalize().unwrap();
        let directory = base.join(format!(
            "actor-{}-{}-空 格",
            process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&directory).unwrap();
        let fixture = Self { base, directory };
        for (source, destination) in [
            ("workspace.toml", "first/Kagari.toml"),
            ("app.toml", "first/app/Kagari.toml"),
            ("api.toml", "first/api/Kagari.toml"),
            ("other.toml", "second/Kagari.toml"),
            ("app.kgr", "first/app/src/lib.kgr"),
            ("api.kgr", "first/api/src/lib.kgr"),
            ("other.kgr", "second/src/lib.kgr"),
        ] {
            let destination = fixture.directory.join(destination);
            fs::create_dir_all(destination.parent().unwrap()).unwrap();
            fs::copy(
                root.join("tests/fixtures/workspace").join(source),
                destination,
            )
            .unwrap();
        }
        fixture
    }

    pub(crate) fn directory(&self) -> &Path {
        &self.directory
    }

    pub(crate) fn uri(&self, relative: &str) -> Uri {
        let path = self.directory.join(relative);
        file_uri(path.to_str().unwrap()).unwrap()
    }

    pub(crate) fn link_directory(&self, target: &Path, alias: &Path) {
        assert!(target.canonicalize().unwrap().starts_with(&self.directory));
        assert!(
            alias
                .parent()
                .unwrap()
                .canonicalize()
                .unwrap()
                .starts_with(&self.directory)
        );
        link_directory(target, alias);
    }

    fn cleanup(&self) -> io::Result<()> {
        let resolved = self.directory.canonicalize()?;
        if !resolved.starts_with(&self.base) || resolved == self.base {
            return Err(Error::other("fixture cleanup escaped its target directory"));
        }
        fs::remove_dir_all(resolved)
    }
}

#[cfg(windows)]
fn link_directory(target: &Path, alias: &Path) {
    let literal = |path: &Path| {
        path.to_str()
            .unwrap()
            .strip_prefix(r"\\?\")
            .unwrap_or(path.to_str().unwrap())
            .replace('\'', "''")
    };
    let command = format!(
        "$ErrorActionPreference = 'Stop'; New-Item -ItemType Junction -Path '{}' -Target '{}' | Out-Null",
        literal(alias),
        literal(target)
    );
    let result = Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", &command])
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
}

#[cfg(unix)]
fn link_directory(target: &Path, alias: &Path) {
    symlink(target, alias).unwrap();
}

impl Drop for Fixture {
    fn drop(&mut self) {
        if let Err(error) = self.cleanup() {
            if thread::panicking() {
                eprintln!("fixture cleanup failed during test unwind: {error}");
            } else {
                panic!("fixture cleanup failed: {error}");
            }
        }
    }
}
