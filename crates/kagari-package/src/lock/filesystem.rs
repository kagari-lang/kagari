use crate::{
    error::PackageError,
    filesystem::FilesystemSources,
    lock::{Lockfile, MAX_LOCK_BYTES, ResolutionOptions},
    manifest::LOCK_NAME,
    modules::SourceInput,
    path,
};
use kagari_common::cancellation::CancellationToken;
use std::{
    fs::{self, OpenOptions},
    io::{ErrorKind, Write},
    path::PathBuf,
    process,
    sync::atomic::{AtomicU64, Ordering},
};

impl Lockfile {
    /// Reads a lock through the approved input provider; missing locks are distinct from invalid ones.
    pub fn load(
        input: &impl SourceInput,
        directory: &str,
        cancel: &CancellationToken,
    ) -> Result<Option<Self>, PackageError> {
        let name = path::join(directory, LOCK_NAME, directory)?;
        input
            .read(&name, MAX_LOCK_BYTES, cancel)?
            .map(|source| Self::parse(&source.origin, &source.text, cancel))
            .transpose()
    }

    /// Atomically replaces a changed lock in its approved workspace directory.
    ///
    /// Locked/frozen modes require an unchanged existing lock and never write.
    /// Cancellation before rename leaves the previous lock intact. Once renamed,
    /// the complete new lock is committed, even if cancellation arrives afterwards.
    /// Returns whether a new lock was published.
    pub fn publish(
        &self,
        input: &FilesystemSources,
        directory: &str,
        options: ResolutionOptions,
        cancel: &CancellationToken,
    ) -> Result<bool, PackageError> {
        cancel.check()?;
        let previous = Self::load(input, directory, cancel)?;
        self.check_policy(previous.as_ref(), options)?;
        if previous.as_ref() == Some(self) {
            return Ok(false);
        }
        let root = input
            .canonical_path(if directory.is_empty() { "." } else { directory })?
            .ok_or_else(|| PackageError::input(directory, "workspace directory is unavailable"))?;
        let target = root.join(LOCK_NAME);
        let encoded = self.encode()?;
        if encoded.len() > MAX_LOCK_BYTES {
            return Err(PackageError::input(LOCK_NAME, "lock exceeds byte limit"));
        }
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let mut temporary = None;
        for _ in 0..128 {
            cancel.check()?;
            let path = root.join(format!(
                ".Kagari.lock.{}.{}.tmp",
                process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            match OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(file) => {
                    temporary = Some((Temporary(path), file));
                    break;
                }
                Err(error) if error.kind() == ErrorKind::AlreadyExists => {}
                Err(error) => return Err(PackageError::input(LOCK_NAME, error.to_string())),
            }
        }
        let (temporary, mut file) = temporary
            .ok_or_else(|| PackageError::input(LOCK_NAME, "cannot allocate a temporary lock"))?;
        file.write_all(encoded.as_bytes())
            .and_then(|()| file.sync_all())
            .map_err(|error| PackageError::input(LOCK_NAME, error.to_string()))?;
        drop(file);
        cancel.check()?;
        fs::rename(&temporary.0, &target)
            .map_err(|error| PackageError::input(LOCK_NAME, error.to_string()))?;
        Ok(true)
    }
}

struct Temporary(PathBuf);

impl Drop for Temporary {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}
