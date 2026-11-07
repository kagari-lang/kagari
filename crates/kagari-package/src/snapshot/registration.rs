//! One semantic registration policy for strict and recovering source captures.
use crate::{
    error::PackageError, identity::PackageSelection, modules::SourceText, snapshot::hash_selection,
};
use blake3::Hasher;
use kagari_common::{cancellation::CancellationToken, identity::ModuleIdentity};
use kagari_source::{
    package::{PackageEnvironment, PackageScope},
    source_database::{CapturedSource, SourceDatabase, SourceSnapshot},
};
use std::collections::BTreeMap;

pub(crate) fn source(module: ModuleIdentity, file: &SourceText) -> CapturedSource {
    CapturedSource {
        key: format!(
            "package://{}/{}/source.kgr",
            module.package.0,
            module.path.join("/")
        ),
        module,
        source: file.source.clone(),
        display_name: file.origin.clone(),
    }
}

pub(crate) fn scope(
    selection: &PackageSelection,
    dependencies: &BTreeMap<String, PackageSelection>,
) -> PackageScope {
    let mut hash = Hasher::new_derive_key("kagari selected package input v1");
    hash_selection(&mut hash, selection);
    PackageScope {
        dependencies: dependencies
            .iter()
            .map(|(alias, selection)| (alias.clone(), selection.family.package_id()))
            .collect(),
        selection: hash.finalize().to_hex().to_string(),
    }
}

pub(crate) fn commit(
    database: &mut SourceDatabase,
    captured: Vec<CapturedSource>,
    environment: PackageEnvironment,
    cancel: &CancellationToken,
) -> Result<SourceSnapshot, PackageError> {
    database
        .replace_captured(captured, environment, cancel)
        .map_err(|message| {
            if cancel.check().is_err() {
                PackageError::Cancelled
            } else {
                PackageError::input("package sources", message)
            }
        })
}
