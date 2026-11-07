//! Fresh disk copies of the concrete two-root app/api acquisition fixture.
use crate::workspace::origins::file_uri;
use lsp_types::Uri;
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

    fn cleanup(&self) -> io::Result<()> {
        let resolved = self.directory.canonicalize()?;
        if !resolved.starts_with(&self.base) || resolved == self.base {
            return Err(Error::other("fixture cleanup escaped its target directory"));
        }
        fs::remove_dir_all(resolved)
    }
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
