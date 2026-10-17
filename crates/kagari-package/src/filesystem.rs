use crate::{
    error::PackageError,
    input::InputTree,
    modules::{SourceInput, SourceText},
    path,
};
use kagari_common::cancellation::CancellationToken;
use std::{
    fs::{self, File},
    io::{self, ErrorKind, Read},
    path::{Path, PathBuf},
    sync::Arc,
};

/// An explicitly approved filesystem input root, captured independently of cwd.
///
/// Canonicalized paths and symlinks must stay below this root. Internal symlinks
/// retain their canonical key for duplicate-source detection. This loader assumes
/// the host controls concurrent filesystem mutations; it is not an OS sandbox.
#[derive(Debug, Clone)]
pub struct FilesystemSources {
    root: PathBuf,
}

impl InputTree for FilesystemSources {
    fn directories(
        &self,
        directory: &str,
        max_entries: usize,
        cancel: &CancellationToken,
    ) -> Result<Vec<String>, PackageError> {
        cancel.check()?;
        let Some(canonical) =
            self.canonical_path(if directory.is_empty() { "." } else { directory })?
        else {
            return Ok(Vec::new());
        };
        let origin = canonical.display().to_string();
        let entries = fs::read_dir(canonical)
            .map_err(|error| PackageError::input(&origin, error.to_string()))?;
        let mut directories = Vec::new();
        for (visited, entry) in entries.enumerate() {
            cancel.check()?;
            if visited >= max_entries {
                return Err(PackageError::input(
                    &origin,
                    "directory discovery exceeds limit",
                ));
            }
            let entry = entry.map_err(|error| PackageError::input(&origin, error.to_string()))?;
            let name = entry
                .file_name()
                .into_string()
                .map_err(|_| PackageError::input(&origin, "non-UTF-8 input name"))?;
            let relative = path::join(directory, &name, &origin)?;
            if let Some(canonical) = self.canonical_path(&relative)?
                && canonical.is_dir()
            {
                directories.push(name);
            }
        }
        directories.sort();
        cancel.check()?;
        Ok(directories)
    }
}

impl FilesystemSources {
    pub fn new(root: &Path) -> Result<Self, PackageError> {
        let canonical = root
            .canonicalize()
            .map_err(|error| PackageError::input(&root.display().to_string(), error.to_string()))?;
        if !canonical.is_dir() {
            return Err(PackageError::input(
                &root.display().to_string(),
                "input root is not a directory",
            ));
        }
        Ok(Self { root: canonical })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Resolves one relative path under the approved root, preserving absence.
    pub fn canonical_path(&self, relative: &str) -> Result<Option<PathBuf>, PackageError> {
        let relative = path::join("", relative, relative)?;
        let requested = self.root.join(&relative);
        let canonical = match requested.canonicalize() {
            Ok(path) => path,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
            Err(error) => {
                return Err(PackageError::input(
                    &requested.display().to_string(),
                    error.to_string(),
                ));
            }
        };
        if !canonical.starts_with(&self.root) {
            return Err(PackageError::input(
                &requested.display().to_string(),
                "symlink escapes the approved input root",
            ));
        }
        Ok(Some(canonical))
    }
}

impl SourceInput for FilesystemSources {
    fn read(
        &self,
        relative: &str,
        max_bytes: usize,
        cancel: &CancellationToken,
    ) -> Result<Option<SourceText>, PackageError> {
        cancel.check()?;
        let Some(canonical) = self.canonical_path(relative)? else {
            return Ok(None);
        };
        let origin = canonical.display().to_string();
        let io_error = |error: io::Error| PackageError::input(&origin, error.to_string());
        let mut file = File::open(&canonical).map_err(io_error)?;
        let metadata = file.metadata().map_err(io_error)?;
        if !metadata.is_file() {
            return Err(PackageError::input(&origin, "source is not a regular file"));
        }
        if metadata.len() > max_bytes as u64 {
            return Err(PackageError::input(&origin, "source exceeds byte limit"));
        }
        let mut bytes = Vec::new();
        let mut chunk = [0; 16 * 1024];
        loop {
            cancel.check()?;
            let length = file.read(&mut chunk).map_err(io_error)?;
            if length == 0 {
                break;
            }
            if length > max_bytes.saturating_sub(bytes.len()) {
                return Err(PackageError::input(&origin, "source exceeds byte limit"));
            }
            bytes.extend_from_slice(&chunk[..length]);
        }
        let text = String::from_utf8(bytes)
            .map_err(|error| PackageError::input(&origin, error.to_string()))?;
        cancel.check()?;
        Ok(Some(SourceText {
            key: origin.clone(),
            origin,
            text: Arc::from(text),
        }))
    }
}
