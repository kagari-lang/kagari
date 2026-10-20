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
        self.commit(&temporary)
    }

    fn commit(&self, temporary: &Temporary) -> io::Result<()> {
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
        source_in(&root, &NEXT_TEMP)
    }

    fn source_in(root: &Path, sequence: &AtomicU64) -> PathBuf {
        fs::create_dir_all(root).unwrap();
        loop {
            let directory = root.join(format!(
                "{}-{}",
                process::id(),
                sequence.fetch_add(1, Ordering::Relaxed)
            ));
            // Restored caches and reused process IDs can leave old fixture links.
            // Claim the whole namespace rather than overwriting just its source.
            match fs::create_dir(&directory) {
                Ok(()) => {
                    let path = directory.join("source.kgr");
                    fs::write(&path, "fn f(){}").unwrap();
                    return path;
                }
                Err(error) if error.kind() == ErrorKind::AlreadyExists => continue,
                Err(error) => panic!("cannot create writeback fixture: {error}"),
            }
        }
    }

    #[test]
    fn reused_fixture_names_preserve_existing_sources_and_links() {
        let parent = source();
        let root = parent.parent().unwrap().join("reused-names");
        let first = source_in(&root, &AtomicU64::new(0));
        let linked = first.with_extension("link.kgr");
        fs::hard_link(&first, &linked).unwrap();
        fs::write(&first, "retained source").unwrap();

        let second = source_in(&root, &AtomicU64::new(0));
        assert_ne!(first.parent(), second.parent());
        assert_eq!(fs::read_to_string(&first).unwrap(), "retained source");
        assert_eq!(fs::read_to_string(&linked).unwrap(), "retained source");
        assert_eq!(fs::read_to_string(&second).unwrap(), "fn f(){}");
        fs::hard_link(&second, second.with_extension("link.kgr")).unwrap();
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

    #[test]
    fn unchanged_readonly_and_hardlinked_sources_never_need_replacement() {
        let path = source();
        let permissions = fs::metadata(&path).unwrap().permissions();
        let mut readonly = permissions.clone();
        readonly.set_readonly(true);
        fs::set_permissions(&path, readonly).unwrap();
        let captured = CapturedFile::read(&path).unwrap();
        captured.replace(&captured.text).unwrap();
        assert!(
            captured
                .replace("changed")
                .unwrap_err()
                .to_string()
                .contains("read-only")
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), captured.text);
        fs::set_permissions(&path, permissions).unwrap();
        let linked = path.with_extension("link.kgr");
        fs::hard_link(&path, &linked).unwrap();
        let captured = CapturedFile::read(&path).unwrap();
        captured.replace(&captured.text).unwrap();
        assert!(
            captured
                .replace("changed")
                .unwrap_err()
                .to_string()
                .contains("hard-linked")
        );
        assert_eq!(fs::read_to_string(&linked).unwrap(), captured.text);
    }

    #[test]
    fn failed_replacement_leaves_original_bytes_and_no_staged_file() {
        let path = source();
        let captured = CapturedFile::read(&path).unwrap();
        let absent = Temporary(path.with_extension("absent"));
        assert!(captured.commit(&absent).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), captured.text);
        assert!(!absent.0.exists());
    }

    #[cfg(any(unix, windows))]
    #[test]
    fn symbolic_file_and_directory_links_keep_their_relationship() {
        let path = source().canonicalize().unwrap();
        let link = path.with_extension("symlink.kgr");
        file_link(&path, &link);
        let captured = CapturedFile::read(&link).unwrap();
        captured.replace(&captured.text).unwrap();
        assert!(
            captured
                .replace("changed")
                .unwrap_err()
                .to_string()
                .contains("links")
        );
        assert!(
            fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), captured.text);
        let parent_link = path.with_extension("directory");
        directory_link(path.parent().unwrap(), &parent_link);
        let through_directory =
            CapturedFile::read(&parent_link.join(path.file_name().unwrap())).unwrap();
        assert!(
            through_directory
                .replace("changed")
                .unwrap_err()
                .to_string()
                .contains("links")
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), captured.text);
    }

    #[cfg(unix)]
    fn file_link(source: &Path, target: &Path) {
        use std::os::unix::fs::symlink;
        symlink(source, target).unwrap();
    }

    #[cfg(unix)]
    fn directory_link(source: &Path, target: &Path) {
        file_link(source, target);
    }

    #[cfg(windows)]
    fn file_link(source: &Path, target: &Path) {
        use std::os::windows::fs::symlink_file;
        symlink_file(source, target).expect("CI requires symlink capability");
    }

    #[cfg(windows)]
    fn directory_link(source: &Path, target: &Path) {
        use std::os::windows::fs::symlink_dir;
        symlink_dir(source, target).expect("CI requires directory symlink capability");
    }

    #[cfg(windows)]
    #[test]
    fn directory_junctions_are_rejected_without_replacing_the_referent() {
        use std::process::Command;
        let path = source().canonicalize().unwrap();
        let junction = path.with_extension("junction");
        let result = Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(&junction)
            .arg(path.parent().unwrap())
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        let captured = CapturedFile::read(&junction.join(path.file_name().unwrap())).unwrap();
        assert!(
            captured
                .replace("changed")
                .unwrap_err()
                .to_string()
                .contains("links")
        );
        assert_eq!(fs::read_to_string(path).unwrap(), captured.text);
    }
}
