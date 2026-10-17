//! Source package spelling from the installed declarations.
use kagari_common::identity::{ModuleIdentity, PackageId};
use kagari_types::declaration::module::ModuleDecl;
use std::iter;

/// Spell a standalone source's encoded family inside generated native declarations.
/// Only an explicitly dependent native importer resolves this spelling.
pub(crate) fn source_package_alias(package: &PackageId) -> Option<String> {
    package
        .0
        .strip_prefix("source-file:")
        .map(|family| format!("__kagari_source_{family}"))
}

pub(crate) fn module_path<'a>(
    identity: &ModuleIdentity,
    mut providers: impl Iterator<Item = &'a ModuleDecl>,
) -> String {
    let source_alias = source_package_alias(&identity.package);
    let package = providers
        .find(|module| module.identity == *identity)
        .and_then(|module| module.package_alias.as_deref())
        .or(source_alias.as_deref())
        .unwrap_or(&identity.package.0);
    iter::once(package)
        .chain(identity.path.iter().map(String::as_str))
        .collect::<Vec<_>>()
        .join("::")
}
