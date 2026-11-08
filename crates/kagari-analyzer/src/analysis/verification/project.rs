//! Concrete two-member disk fixture shared by acquisition/analysis probes.
use std::{
    fs,
    path::{Path, PathBuf},
};

pub(super) fn create(directory: &Path, app: &str, api: &str) -> PathBuf {
    for (name, text) in [
        (
            "Kagari.toml",
            include_str!("../../../tests/fixtures/workspace/workspace.toml"),
        ),
        (
            "app/Kagari.toml",
            include_str!("../../../tests/fixtures/workspace/app.toml"),
        ),
        (
            "api/Kagari.toml",
            include_str!("../../../tests/fixtures/workspace/api.toml"),
        ),
        ("app/src/lib.kgr", app),
        ("api/src/lib.kgr", api),
    ] {
        let path = directory.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }
    directory.to_owned()
}
