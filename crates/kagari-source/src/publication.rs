//! Immutable source packages with readable module paths and content-bound snapshots.
//! Planning does not require a writable cache. Publish on an IO owner after the
//! caller validates declaration contracts; old snapshots are never overwritten.
use crate::source_database::normalize_source_name;
use blake3::Hasher;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File, OpenOptions},
    io::{self, Error, ErrorKind, Read, Write},
    iter,
    path::{Path, PathBuf, absolute},
    process,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

static TEMPORARY_ID: AtomicU64 = AtomicU64::new(0);
const MAX_PACKAGE_FILES: usize = 4096;
const MAX_RELATIVE_PATH_BYTES: usize = 8 * 1024;

#[derive(Debug, thiserror::Error)]
#[error("{path}: {error}")]
pub struct PublicationError {
    pub path: PathBuf,
    #[source]
    pub error: io::Error,
}

/// Captured destination root. Opening it does not create directories or files.
#[derive(Debug, Clone)]
pub struct SourceStore {
    directory: PathBuf,
    source_directory: String,
    installed: BTreeMap<String, InstalledPackage>,
}

#[derive(Debug, Clone)]
struct InstalledPackage {
    snapshot: String,
    directory: PathBuf,
    source_directory: String,
}

/// One authored or exported file in a source package.
#[derive(Clone)]
pub struct PublicationSource {
    pub path: String,
    pub text: Arc<str>,
}

impl SourceStore {
    /// Resolve an existing root when possible; otherwise retain its absolute path.
    /// Read-only or absent caches do not prevent planning semantic source inputs.
    pub fn open(root: &Path) -> Result<Self, PublicationError> {
        let absolute = absolute(root).map_err(|error| failure(root, error))?;
        let directory = fs::canonicalize(&absolute).unwrap_or(absolute);
        let source_directory = source_name(&directory)?;
        Ok(Self {
            directory,
            source_directory,
            installed: BTreeMap::new(),
        })
    }

    /// Bind a complete source set to an immutable directory. The snapshot hashes
    /// sorted paths and exact text; returned documents preserve the input order.
    /// Paths are portable relative components, with no traversal or case aliases.
    pub fn plan(
        &self,
        package: &str,
        sources: Vec<PublicationSource>,
    ) -> Result<Vec<SourceDocument>, PublicationError> {
        let snapshot = self.snapshot(package, &sources)?;
        let installed = self
            .installed
            .get(package)
            .filter(|installed| installed.snapshot == snapshot);
        let relative = format!("{package}/{snapshot}");
        Ok(sources
            .into_iter()
            .map(|source| SourceDocument {
                root: installed.map_or_else(
                    || self.directory.clone(),
                    |installed| installed.directory.clone(),
                ),
                path: installed
                    .map_or_else(
                        || self.directory.join(&relative),
                        |installed| installed.directory.clone(),
                    )
                    .join(&source.path),
                source_name: installed.map_or_else(
                    || format!("{}/{relative}/{}", self.source_directory, source.path),
                    |installed| format!("{}/{}", installed.source_directory, source.path),
                ),
                installed: installed.is_some(),
                text: source.text,
            })
            .collect())
    }

    /// Prefer an installed, immutable package after checking every expected file.
    /// A different source set still plans against the cache; installed files are
    /// read-only inputs and publication never repairs or overwrites them.
    pub fn prefer_installed(
        &mut self,
        package: &str,
        root: &Path,
        sources: &[PublicationSource],
    ) -> Result<(), PublicationError> {
        let snapshot = self.snapshot(package, sources)?;
        let directory = fs::canonicalize(root).map_err(|error| failure(root, error))?;
        for source in sources {
            let path = directory.join(&source.path);
            let physical = fs::canonicalize(&path).map_err(|error| failure(&path, error))?;
            if !physical.starts_with(&directory) {
                return Err(failure(
                    &path,
                    Error::new(
                        ErrorKind::InvalidInput,
                        "installed source escapes package root",
                    ),
                ));
            }
            verify(&path, source.text.as_bytes()).map_err(|error| failure(&path, error))?;
        }
        let source_directory = source_name(&directory)?;
        self.installed.insert(
            package.into(),
            InstalledPackage {
                snapshot,
                directory,
                source_directory,
            },
        );
        Ok(())
    }

    fn snapshot(
        &self,
        package: &str,
        sources: &[PublicationSource],
    ) -> Result<String, PublicationError> {
        let invalid = |message| {
            failure(
                &self.directory,
                Error::new(ErrorKind::InvalidInput, message),
            )
        };
        if !valid_path(package) || sources.len() > MAX_PACKAGE_FILES {
            return Err(invalid("invalid source package name or file count"));
        }
        let mut paths = BTreeSet::new();
        for source in sources {
            if !valid_path(&source.path)
                || !source.path.ends_with(".kgr")
                || !paths.insert(source.path.to_lowercase())
            {
                return Err(invalid(
                    "invalid, duplicate or case-aliased source package path",
                ));
            }
        }
        let mut sorted = sources.iter().collect::<Vec<_>>();
        sorted.sort_by(|left, right| left.path.cmp(&right.path));
        let mut hash = Hasher::new_derive_key("kagari immutable declaration source set");
        for source in sorted {
            for bytes in [source.path.as_bytes(), source.text.as_bytes()] {
                hash.update(&(bytes.len() as u64).to_le_bytes());
                hash.update(bytes);
            }
        }
        Ok(hash.finalize().to_hex().to_string())
    }
}

/// Filesystem-safe spelling for an export's package and module segments. Ordinary
/// lowercase names stay readable; escaped bytes prevent platform aliases/reserved names.
pub fn module_path(package: &str, modules: &[String]) -> String {
    let mut parts = iter::once(package)
        .chain(modules.iter().map(String::as_str))
        .map(component)
        .collect::<Vec<_>>();
    if modules.is_empty() {
        parts.push("lib".into());
    }
    format!("{}.kgr", parts.join("/"))
}

fn component(value: &str) -> String {
    let reserved = reserved(value);
    let mut output = String::new();
    for (index, byte) in value.bytes().enumerate() {
        if (byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'_' | b'-'))
            && !(reserved && index == 0)
        {
            output.push(char::from(byte));
        } else {
            output.push_str(&format!("%{byte:02X}"));
        }
    }
    if output.is_empty() {
        output.push_str("%00");
    }
    output
}

fn reserved(value: &str) -> bool {
    let stem = value
        .split('.')
        .next()
        .unwrap_or_default()
        .to_ascii_uppercase();
    matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || ((stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.len() == 4
            && matches!(stem.as_bytes()[3], b'1'..=b'9'))
}

fn valid_path(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_RELATIVE_PATH_BYTES
        && value.split('/').all(|part| {
            !part.is_empty()
                && !matches!(part, "." | "..")
                && !part.ends_with('.')
                && !reserved(part)
                && part.bytes().all(|byte| {
                    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b'%')
                })
        })
}

/// Exact immutable source bytes and their final presentation path.
#[derive(Debug, Clone)]
pub struct SourceDocument {
    root: PathBuf,
    path: PathBuf,
    source_name: String,
    text: Arc<str>,
    installed: bool,
}

impl SourceDocument {
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn source_name(&self) -> &str {
        &self.source_name
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    /// Publish after semantic validation. Existing targets must match exactly;
    /// corruption, redirected parents and IO failures preserve their destination.
    pub fn publish(&self) -> Result<(), PublicationError> {
        if self.installed {
            return verify(&self.path, self.text.as_bytes())
                .map_err(|error| failure(&self.path, error));
        }
        let directory = self.path.parent().expect("source package file parent");
        fs::create_dir_all(&self.root).map_err(|error| failure(&self.root, error))?;
        let root = fs::canonicalize(&self.root).map_err(|error| failure(&self.root, error))?;
        let mut parent = self.root.clone();
        for part in directory
            .strip_prefix(&self.root)
            .expect("planned package is inside root")
            .components()
        {
            parent.push(part);
            match fs::create_dir(&parent) {
                Ok(()) => {}
                Err(error) if error.kind() == ErrorKind::AlreadyExists => {}
                Err(error) => return Err(failure(&parent, error)),
            }
            let actual = fs::canonicalize(&parent).map_err(|error| failure(&parent, error))?;
            if !actual.starts_with(&root) {
                return Err(failure(
                    &parent,
                    Error::new(
                        ErrorKind::InvalidInput,
                        "source package path escapes publication root",
                    ),
                ));
            }
        }
        if fs::symlink_metadata(&self.path).is_ok_and(|metadata| metadata.file_type().is_symlink())
        {
            return Err(failure(
                &self.path,
                Error::new(
                    ErrorKind::InvalidInput,
                    "source publication target is a symlink",
                ),
            ));
        }
        match verify(&self.path, self.text.as_bytes()) {
            Ok(()) => return Ok(()),
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => return Err(failure(&self.path, error)),
        }
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

fn failure(path: &Path, error: Error) -> PublicationError {
    PublicationError {
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
            "source document content differs from its identity",
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

fn source_name(path: &Path) -> Result<String, PublicationError> {
    let name = path.to_str().ok_or_else(|| {
        failure(
            path,
            Error::new(ErrorKind::InvalidInput, "source document path is not UTF-8"),
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
