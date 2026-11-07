use crate::{
    error::PackageError,
    input::InputTree,
    modules::{SourceInput, SourceText},
    path,
};
use kagari_common::cancellation::CancellationToken;
use kagari_source::source::SourceFile;
use std::{
    fs::{self, File},
    io::{self, ErrorKind, Read},
    path::{Path, PathBuf},
    sync::Arc,
};

/// Resolves the physical identity of an explicit absolute input, including an
/// absent suffix beneath an existing directory. Existing symlinks are resolved;
/// dangling links and non-directory ancestors remain errors.
///
/// This captures an origin and grants no package traversal authority. Rooted
/// readers must still enforce containment, as [`FilesystemSources::approved_path`]
/// does. The host-controlled concurrent-filesystem-mutation assumption applies.
pub fn canonical_input_path(requested: &Path) -> Result<PathBuf, PackageError> {
    let origin = requested.display().to_string();
    if !requested.is_absolute() {
        return Err(PackageError::input(
            &origin,
            "explicit input path must be absolute",
        ));
    }
    let mut ancestor = requested;
    let mut absent = Vec::new();
    loop {
        match fs::symlink_metadata(ancestor) {
            Ok(_) => {
                let mut canonical = ancestor
                    .canonicalize()
                    .map_err(|error| PackageError::input(&origin, error.to_string()))?;
                if !absent.is_empty() && !canonical.is_dir() {
                    return Err(PackageError::input(
                        &origin,
                        "input ancestor is not a directory",
                    ));
                }
                for component in absent.iter().rev() {
                    canonical.push(component);
                }
                return Ok(canonical);
            }
            Err(error) if error.kind() == ErrorKind::NotFound => {
                let component = ancestor.file_name().ok_or_else(|| {
                    PackageError::input(&origin, "approved input root disappeared")
                })?;
                absent.push(component);
                ancestor = ancestor.parent().ok_or_else(|| {
                    PackageError::input(&origin, "approved input root disappeared")
                })?;
            }
            Err(error) => return Err(PackageError::input(&origin, error.to_string())),
        }
    }
}

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

    /// Resolves an input path even when its final components do not yet exist.
    ///
    /// Existing ancestors must be directories inside the approved root. Existing
    /// symlinks, including dangling links, are never treated as absent components.
    /// This validates an unsaved source's origin without creating files; the same
    /// host-controlled concurrent-mutation assumption as `canonical_path` applies.
    pub fn approved_path(&self, relative: &str) -> Result<PathBuf, PackageError> {
        let relative = path::join("", relative, relative)?;
        let requested = self.root.join(&relative);
        let canonical = canonical_input_path(&requested)?;
        if !canonical.starts_with(&self.root) {
            return Err(PackageError::input(
                &requested.display().to_string(),
                "symlink escapes the approved input root",
            ));
        }
        Ok(canonical)
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
            source: Arc::new(SourceFile::new(&origin, text)),
            key: origin.clone(),
            origin,
        }))
    }
}
