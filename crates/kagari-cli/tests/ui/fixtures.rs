use super::{CASES, Case, wire};
use std::{collections::BTreeSet, fs, path::Path};

pub(super) struct Expected {
    pub source: String,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub exit: i32,
}

pub(super) fn load(root: &Path, case: &Case) -> Result<Expected, String> {
    let read = |name: &str| -> Result<Vec<u8>, String> {
        let path = root.join(name);
        let metadata =
            fs::symlink_metadata(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        if !metadata.file_type().is_file() {
            return Err(format!("not a regular fixture: {}", path.display()));
        }
        fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))
    };
    let source =
        String::from_utf8(read(&format!("{}.kgr", case.source))?).map_err(|e| e.to_string())?;
    let stdout = read(&format!("{}.stdout", case.stem))?;
    let stderr = read(&format!("{}.stderr", case.stem))?;
    std::str::from_utf8(&stderr).map_err(|e| e.to_string())?;
    let exit = read(&format!("{}.exit", case.stem))?;
    let exit_text = std::str::from_utf8(&exit).map_err(|e| e.to_string())?;
    let exit: u8 = exit_text
        .strip_suffix('\n')
        .ok_or("exit must end with LF")?
        .parse()
        .map_err(|_| "invalid exit code")?;
    if exit_text != format!("{exit}\n") {
        return Err("exit must be a canonical decimal line".into());
    }
    if case.json {
        wire::expected(&stdout, &source, i32::from(exit))?;
    } else if !stdout.is_empty() || stderr.is_empty() {
        return Err("human diagnostic fixture requires empty stdout and nonempty stderr".into());
    }
    Ok(Expected {
        source,
        stdout,
        stderr,
        exit: i32::from(exit),
    })
}

pub(super) fn inventory(root: &Path) -> Result<(), String> {
    let mut expected = BTreeSet::from([".gitattributes".to_owned()]);
    for case in &CASES {
        expected.insert(format!("{}.kgr", case.source));
        for suffix in ["stdout", "stderr", "exit"] {
            expected.insert(format!("{}.{suffix}", case.stem));
        }
        load(root, case)?;
    }
    let actual = fs::read_dir(root)
        .map_err(|e| e.to_string())?
        .map(|entry| {
            let entry = entry.map_err(|e| e.to_string())?;
            if !entry.file_type().map_err(|e| e.to_string())?.is_file() {
                return Err("fixture inventory contains a nonregular file".into());
            }
            entry
                .file_name()
                .into_string()
                .map_err(|_| "non-UTF8 fixture name".to_owned())
        })
        .collect::<Result<BTreeSet<_>, String>>()?;
    if actual != expected {
        return Err(format!(
            "missing or orphaned UI fixtures\nactual: {actual:?}\nexpected: {expected:?}"
        ));
    }
    Ok(())
}
