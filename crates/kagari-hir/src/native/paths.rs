//! Source package spelling from the installed declarations.
use kagari_common::identity::ModuleIdentity;
use kagari_types::declaration::module::ModuleDecl;

pub(crate) fn module_path<'a>(
    identity: &ModuleIdentity,
    mut providers: impl Iterator<Item = &'a ModuleDecl>,
) -> String {
    let package = providers
        .find(|module| module.identity == *identity)
        .and_then(|module| module.package_alias.as_deref())
        .unwrap_or(&identity.package.0);
    format!("{}::{}", package, identity.path.join("::"))
}
