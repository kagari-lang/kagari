use crate::modules::VirtualSources;
use kagari_common::cancellation::CancellationToken;
use std::{
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{SystemTime, UNIX_EPOCH},
};

#[cfg(windows)]
use std::{io, process::Command};

#[cfg(unix)]
use std::os::unix::fs::symlink;

pub fn sources(files: &[(&str, &str)]) -> VirtualSources {
    VirtualSources::new(
        files
            .iter()
            .map(|(name, text)| (name.to_string(), Arc::from(*text))),
        &CancellationToken::default(),
    )
    .unwrap()
}

pub fn scratch() -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/package-inputs")
        .join(format!(
            "{}-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
    fs::create_dir_all(&root).unwrap();
    root.canonicalize().unwrap()
}

/// Windows junctions exercise canonical-path aliasing without symlink privileges.
#[cfg(windows)]
pub fn link_directory(target: &Path, link: &Path) -> io::Result<()> {
    let quote = |path: &Path| {
        let displayed = path.display().to_string();
        // Windows PowerShell's junction cmdlet does not accept verbatim targets.
        let ordinary = if let Some(unc) = displayed.strip_prefix("\\\\?\\UNC\\") {
            format!("\\\\{unc}")
        } else {
            displayed
                .strip_prefix("\\\\?\\")
                .unwrap_or(&displayed)
                .to_owned()
        };
        ordinary.replace('\'', "''")
    };
    let script = format!(
        "$ErrorActionPreference = 'Stop'; New-Item -ItemType Junction -Path '{}' -Target '{}' | Out-Null",
        quote(link),
        quote(target)
    );
    let output = Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command"])
        .arg(script)
        .output()?;
    if output.status.success() {
        Ok(())
    } else {
        Err(io::Error::other(
            String::from_utf8_lossy(&output.stderr).into_owned(),
        ))
    }
}

#[cfg(unix)]
pub fn link_directory(target: &Path, link: &Path) -> std::io::Result<()> {
    symlink(target, link)
}
