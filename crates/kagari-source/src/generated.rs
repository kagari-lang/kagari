//! Content-addressed generated documents, independent of rendering and semantics.
//!
//! Open the store and publish documents on an IO owner. Planning is pure after
//! opening; callers validate the final source identity before publishing its bytes.
use crate::source_database::normalize_source_name;
use blake3::hash;
use std::{
    error::Error as StdError,
    fmt,
    fs::{self, File, OpenOptions},
    io::{self, Error, ErrorKind, Read, Write},
    path::{Path, PathBuf},
    process,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

static TEMPORARY_ID: AtomicU64 = AtomicU64::new(0);

/// Exact filesystem destination and underlying failure from store IO or validation.
#[derive(Debug)]
pub struct GeneratedDocumentError {
    pub path: PathBuf,
    pub error: io::Error,
}

impl fmt::Display for GeneratedDocumentError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.path.display(), self.error)
    }
}

impl StdError for GeneratedDocumentError {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        Some(&self.error)
    }
}

/// Canonical renderer directory captured once by the filesystem owner.
/// Clones contain only immutable paths, never file handles or semantic state.
#[derive(Debug, Clone)]
pub struct GeneratedDocuments {
    directory: PathBuf,
    source_directory: String,
}

impl GeneratedDocuments {
    /// Creates and canonicalizes one renderer namespace under the supplied root.
    /// The namespace is a single ASCII component containing letters, digits, `.`,
    /// `_` or `-`, excluding `.` and `..`. The normalized path must be local UTF-8.
    /// No generated file is published by this operation.
    pub fn open(root: &Path, namespace: &str) -> Result<Self, GeneratedDocumentError> {
        if namespace.is_empty()
            || matches!(namespace, "." | "..")
            || !namespace
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
        {
            return Err(failure(
                root,
                Error::new(
                    ErrorKind::InvalidInput,
                    "invalid generated-document namespace",
                ),
            ));
        }
        let directory = root.join(namespace);
        fs::create_dir_all(&directory).map_err(|error| failure(&directory, error))?;
        let directory = fs::canonicalize(&directory).map_err(|error| failure(&directory, error))?;
        let source_directory = source_name(&directory)?;
        Ok(Self {
            directory,
            source_directory,
        })
    }

    /// Plans the immutable path and exact bytes without filesystem access.
    /// The caller retains rendering/validation authority; a plan proves neither.
    pub fn plan(&self, text: Arc<str>) -> GeneratedDocument {
        let filename = format!("{}.kgr", hash(text.as_bytes()).to_hex());
        GeneratedDocument {
            path: self.directory.join(&filename),
            source_name: format!("{}/{filename}", self.source_directory),
            text,
        }
    }
}

/// Immutable publication bytes bound to their final content-addressed source name.
/// Old files are never replaced or removed by the service; cache cleanup belongs
/// outside sessions that may retain source locations.
#[derive(Debug, Clone)]
pub struct GeneratedDocument {
    path: PathBuf,
    source_name: String,
    text: Arc<str>,
}

impl GeneratedDocument {
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Normalized absolute source path, not a protocol file URI.
    pub fn source_name(&self) -> &str {
        &self.source_name
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    /// Atomically publishes the planned bytes without replacing an existing target.
    /// Call only after semantic validation at this exact source identity. Existing
    /// content must match byte-for-byte; corruption and IO failures retain their
    /// destination and error kind. Concurrent identical publishers may both succeed.
    pub fn publish(&self) -> Result<(), GeneratedDocumentError> {
        match verify(&self.path, self.text.as_bytes()) {
            Ok(()) => return Ok(()),
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => return Err(failure(&self.path, error)),
        }
        let directory = self.path.parent().expect("generated document directory");
        let temporary = directory.join(format!(
            ".{}-{}.tmp",
            process::id(),
            TEMPORARY_ID.fetch_add(1, Ordering::Relaxed)
        ));
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(|error| failure(&temporary, error))?;
        // Cleanup owns only a file this operation successfully created. A stale
        // temporary file that made create_new fail must not be deleted.
        let temporary_file = TemporaryFile(temporary);
        file.write_all(self.text.as_bytes())
            .map_err(|error| failure(&temporary_file.0, error))?;
        drop(file);
        match fs::hard_link(&temporary_file.0, &self.path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == ErrorKind::AlreadyExists => {
                verify(&self.path, self.text.as_bytes())
            }
            Err(error) => Err(error),
        }
        .map_err(|error| failure(&self.path, error))
    }
}

struct TemporaryFile(PathBuf);

impl Drop for TemporaryFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

fn failure(path: &Path, error: Error) -> GeneratedDocumentError {
    GeneratedDocumentError {
        path: path.to_owned(),
        error,
    }
}

/// Compare with bounded scratch storage even if an external editor replaces the
/// cached file with a much larger document after it was originally published.
fn verify(path: &Path, expected: &[u8]) -> io::Result<()> {
    let mut file = File::open(path)?;
    let metadata = file.metadata()?;
    let mismatch = || {
        Error::new(
            ErrorKind::InvalidData,
            "generated document content differs from its identity",
        )
    };
    if !metadata.is_file() || metadata.len() != expected.len() as u64 {
        return Err(mismatch());
    }
    let mut position = 0;
    let mut chunk = [0; 16 * 1024];
    loop {
        let length = file.read(&mut chunk)?;
        if length == 0 {
            return if position == expected.len() {
                Ok(())
            } else {
                Err(mismatch())
            };
        }
        if expected.get(position..position + length) != Some(&chunk[..length]) {
            return Err(mismatch());
        }
        position += length;
    }
}

fn source_name(path: &Path) -> Result<String, GeneratedDocumentError> {
    let name = path.to_str().ok_or_else(|| {
        failure(
            path,
            Error::new(
                ErrorKind::InvalidInput,
                "generated document path is not UTF-8",
            ),
        )
    })?;
    let name = name
        .strip_prefix(r"\\?\")
        .filter(|name| name.as_bytes().get(1) == Some(&b':'))
        .unwrap_or(name);
    normalize_source_name(name)
        .map_err(|message| failure(path, Error::new(ErrorKind::InvalidInput, message)))
}

#[cfg(test)]
mod tests;
