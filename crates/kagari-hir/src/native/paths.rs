//! Source spellings of explicit library owners, independent of registration authority.
use crate::{analysis::external::ExternalModule, imports::LocalName};
use kagari_common::identity::{
    ModuleIdentity, PackageId, metadata::DefinitionMetadata, table::DefinitionView,
};
use kagari_types::declaration::module::ModuleDecl;
use std::{
    collections::{BTreeMap, BTreeSet},
    iter,
    sync::Arc,
};

/// Spell a standalone source's encoded family inside generated native declarations.
/// Only an explicitly dependent native importer resolves this spelling.
pub(crate) fn source_package_alias(package: &PackageId) -> Option<String> {
    package
        .0
        .strip_prefix("source-file:")
        .map(|family| format!("__kagari_source_{family}"))
}

/// Presentation and import-scope inputs only. Duplicate owners are rejected by
/// declaration preparation; an alias never creates a module or grants visibility.
#[derive(Debug, Default)]
pub(crate) struct LibraryPaths {
    preferred: BTreeMap<ModuleIdentity, String>,
    packages: BTreeMap<PackageId, BTreeSet<String>>,
}

impl LibraryPaths {
    pub(crate) fn new(
        native: &[Arc<DefinitionMetadata<ModuleDecl>>],
        external: &[ExternalModule],
    ) -> Self {
        let native = native.iter().map(|provider| {
            let module = provider.records();
            (&module.identity, module.package_alias.as_ref())
        });
        let authored = external.iter().map(|module| {
            (
                module.source.module_identity(),
                module.package_alias.as_ref(),
            )
        });
        let mut paths = Self::default();
        for (owner, alias) in native.chain(authored) {
            let aliases = paths.packages.entry(owner.package.clone()).or_default();
            if let Some(alias) = alias.filter(|name| LocalName::new(name).is_some()) {
                aliases.insert(alias.clone());
                paths.preferred.insert(owner.clone(), alias.clone());
            }
        }
        paths
    }

    pub(crate) fn bindings(&self) -> impl Iterator<Item = (&String, &PackageId)> {
        self.packages.iter().flat_map(|(package, aliases)| {
            let canonical = LocalName::new(&package.0).map(|_| &package.0);
            aliases
                .iter()
                .chain(canonical)
                .map(move |name| (name, package))
        })
    }

    pub(crate) fn definition_path(&self, target: DefinitionView<'_>) -> String {
        let mut path = self.module_path(target.module());
        for segment in target.segments() {
            path.push_str("::");
            path.push_str(segment.name);
        }
        path
    }

    pub(crate) fn module_path(&self, identity: &ModuleIdentity) -> String {
        // Package bindings also cover sibling and inline modules. Prefer the
        // owner's choice; otherwise choose deterministically among its package aliases.
        let alias = self
            .preferred
            .get(identity)
            .or_else(|| self.packages.get(&identity.package)?.first());
        let fallback = source_package_alias(&identity.package);
        let package = alias
            .map(String::as_str)
            .or(fallback.as_deref())
            .unwrap_or(&identity.package.0);
        iter::once(package)
            .chain(identity.path.iter().map(String::as_str))
            .collect::<Vec<_>>()
            .join("::")
    }
}
