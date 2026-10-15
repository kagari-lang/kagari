//! Per-file replacement under a host-controlled filesystem mutation assumption.

#[cfg(unix)]
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::{
    env,
    fs::{self, File, Metadata, OpenOptions},
    io::{self, Error, ErrorKind, Read, Write},
    path::{Path, PathBuf},
    process,
    sync::atomic::{AtomicU64, Ordering},
};
#[cfg(windows)]
use std::{
    mem::MaybeUninit,
    os::windows::{fs::MetadataExt, io::AsRawHandle},
};
#[cfg(windows)]
use windows_sys::Win32::Storage::FileSystem::{
    BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle,
};

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);
const MAX_BYTES: u64 = 4 * 1024 * 1024;

pub(crate) struct CapturedFile {
    path: PathBuf,
    pub text: String,
    metadata: Metadata,
    identity: Identity,
}

#[derive(PartialEq, Eq)]
struct Identity {
    volume: u64,
    file: u64,
    links: u64,
}

impl CapturedFile {
    pub fn read(path: &Path) -> io::Result<Self> {
        let path = if path.is_absolute() {
            path.to_owned()
        } else {
            env::current_dir()?.join(path)
        };
        if !fs::metadata(&path)?.is_file() {
            return Err(failure("source is not a regular file"));
        }
        let mut file = File::open(&path)?;
        let metadata = file.metadata()?;
        if metadata.len() > MAX_BYTES {
            return Err(failure("source exceeds byte limit"));
        }
        let identity = identity(&file, &metadata)?;
        let mut text = String::new();
        (&mut file).take(MAX_BYTES + 1).read_to_string(&mut text)?;
        if text.len() as u64 > MAX_BYTES || !same_metadata(&metadata, &file.metadata()?) {
            return Err(failure("source changed while reading"));
        }
        Ok(Self {
            path,
            text,
            metadata,
            identity,
        })
    }

    pub fn replace(&self, output: &str) -> io::Result<()> {
        if self.text == output {
            return Ok(());
        }
        self.validate()?;
        let parent = self
            .path
            .parent()
            .ok_or_else(|| failure("source has no parent directory"))?;
        let (temporary, mut file) = loop {
            let path = parent.join(format!(
                ".kg-fmt-{}-{}.tmp",
                process::id(),
                NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
            ));
            let mut options = OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            options.mode(0o600);
            match options.open(&path) {
                Ok(file) => break (Temporary(path), file),
                Err(error) if error.kind() == ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error),
            }
        };
        file.write_all(output.as_bytes())?;
        file.set_permissions(self.metadata.permissions())?;
        file.sync_all()?;
        drop(file);
        self.validate()?;
        // std::fs::rename replaces a file on Unix and Windows. There is no
        // compare-and-swap guarantee against a hostile edit after validation.
        fs::rename(&temporary.0, &self.path)
    }

    fn validate(&self) -> io::Result<()> {
        for path in self.path.ancestors() {
            let metadata = fs::symlink_metadata(path)?;
            if metadata.file_type().is_symlink() || reparse_point(&metadata) {
                return Err(failure(
                    "writeback through symbolic links or directory links is unsupported",
                ));
            }
        }
        if self.metadata.permissions().readonly() {
            return Err(failure("source is read-only"));
        }
        if self.identity.links != 1 {
            return Err(failure("writeback to hard-linked files is unsupported"));
        }
        let current = Self::read(&self.path)?;
        if current.identity != self.identity
            || !same_metadata(&current.metadata, &self.metadata)
            || current.text != self.text
        {
            return Err(failure(
                "source changed concurrently; no replacement was written",
            ));
        }
        Ok(())
    }
}

struct Temporary(PathBuf);

impl Drop for Temporary {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

fn failure(message: &str) -> io::Error {
    Error::other(message)
}

fn same_metadata(left: &Metadata, right: &Metadata) -> bool {
    left.len() == right.len()
        && left.modified().ok() == right.modified().ok()
        && left.permissions() == right.permissions()
}

#[cfg(unix)]
fn identity(_file: &File, metadata: &Metadata) -> io::Result<Identity> {
    Ok(Identity {
        volume: metadata.dev(),
        file: metadata.ino(),
        links: metadata.nlink(),
    })
}

#[cfg(windows)]
fn identity(file: &File, _metadata: &Metadata) -> io::Result<Identity> {
    let mut information = MaybeUninit::<BY_HANDLE_FILE_INFORMATION>::uninit();
    // SAFETY: the live File owns a valid handle; the API initializes the entire
    // output record on success, which is checked before assume_init.
    if unsafe { GetFileInformationByHandle(file.as_raw_handle(), information.as_mut_ptr()) } == 0 {
        return Err(Error::last_os_error());
    }
    // SAFETY: the successful call above initialized every field.
    let information = unsafe { information.assume_init() };
    Ok(Identity {
        volume: information.dwVolumeSerialNumber.into(),
        file: (u64::from(information.nFileIndexHigh) << 32) | u64::from(information.nFileIndexLow),
        links: information.nNumberOfLinks.into(),
    })
}

#[cfg(not(any(unix, windows)))]
fn identity(_file: &File, _metadata: &Metadata) -> io::Result<Identity> {
    Err(failure("source identity is unsupported on this platform"))
}

#[cfg(windows)]
fn reparse_point(metadata: &Metadata) -> bool {
    metadata.file_attributes() & 0x400 != 0
}

#[cfg(not(windows))]
fn reparse_point(_metadata: &Metadata) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source() -> PathBuf {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/format-writeback");
        fs::create_dir_all(&root).unwrap();
        let path = root.join(format!(
            "{}-{}.kgr",
            process::id(),
            NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
        ));
        fs::write(&path, "fn f(){}").unwrap();
        path
    }

    #[test]
    fn replacement_preserves_permissions_and_detects_concurrent_content() {
        let path = source();
        let captured = CapturedFile::read(&path).unwrap();
        captured.replace("fn f() {}\n").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "fn f() {}\n");
        assert_eq!(
            fs::metadata(&path).unwrap().permissions(),
            captured.metadata.permissions()
        );
        assert!(
            captured
                .replace("other")
                .unwrap_err()
                .to_string()
                .contains("concurrently")
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), "fn f() {}\n");
    }
}
