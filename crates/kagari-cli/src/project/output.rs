//! Immutable project outputs, addressed by captured inputs and emitted content.
//!
//! This is output storage, not a pre-compilation cache: every request still checks
//! its current source and native inputs. Existing bytes are compared before reuse.

use crate::CliError;
use blake3::Hasher;
use kagari_package::snapshot::ProgramInputs;
use std::{
    fs::{self, File, OpenOptions},
    io::{self, ErrorKind, Read, Write},
    path::{Path, PathBuf},
    process,
    sync::atomic::{AtomicU64, Ordering},
};

pub(super) fn publish(
    workspace: &Path,
    inputs: &ProgramInputs,
    bytes: &[u8],
) -> Result<PathBuf, CliError> {
    let mut hash = Hasher::new_derive_key("kagari project output v1");
    hash.update(inputs.fingerprint().as_bytes());
    hash.update(bytes);
    let directory = workspace.join("target/kagari/programs");
    fs::create_dir_all(&directory).map_err(io_error)?;
    let target = directory.join(format!("{}.kbc", hash.finalize().to_hex()));
    match matches_file(&target, bytes) {
        Ok(true) => return Ok(target),
        Ok(false) => {
            return Err(CliError::message(
                1,
                "project output content differs from its identity",
            ));
        }
        Err(error) if error.kind() == ErrorKind::NotFound => {}
        Err(error) => return Err(io_error(error)),
    }
    let (temporary, mut file) = temporary(&directory)?;
    file.write_all(bytes).map_err(io_error)?;
    file.sync_all().map_err(io_error)?;
    drop(file);
    match fs::hard_link(&temporary.0, &target) {
        Ok(()) => {}
        Err(error) if error.kind() == ErrorKind::AlreadyExists => {
            if !matches_file(&target, bytes).map_err(io_error)? {
                return Err(CliError::message(
                    1,
                    "concurrent project output differs from its identity",
                ));
            }
        }
        Err(error) => return Err(io_error(error)),
    }
    Ok(target)
}

fn matches_file(path: &Path, bytes: &[u8]) -> io::Result<bool> {
    let file = File::open(path)?;
    if file.metadata()?.len() != bytes.len() as u64 {
        return Ok(false);
    }
    // Bound reads even if another process replaces or extends a damaged product.
    let mut previous = Vec::new();
    file.take(bytes.len() as u64 + 1)
        .read_to_end(&mut previous)?;
    Ok(previous == bytes)
}

fn temporary(directory: &Path) -> Result<(Temporary, File), CliError> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    for _ in 0..128 {
        let path = directory.join(format!(
            ".{}-{}.tmp",
            process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => return Ok((Temporary(path), file)),
            Err(error) if error.kind() == ErrorKind::AlreadyExists => {}
            Err(error) => return Err(io_error(error)),
        }
    }
    Err(CliError::message(
        1,
        "cannot allocate temporary project output",
    ))
}

fn io_error(error: io::Error) -> CliError {
    CliError::message(1, format!("cannot publish project output: {error}"))
}

struct Temporary(PathBuf);

impl Drop for Temporary {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}
