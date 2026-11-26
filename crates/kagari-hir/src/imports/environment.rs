//! Explicit import-scope inputs, independent of declaration representation.

use kagari_common::identity::{ModuleIdentity, PackageId};
use kagari_source::package::PackageScope;
use std::collections::{BTreeMap, BTreeSet};

/// Non-authored import inputs for one module. These grant no member access.
///
/// Package bindings use the same alias-to-package vocabulary as source packages.
/// Dependencies include explicit edges even when no authored use mentions them.
/// A selected prelude contributes only accessible implicit bindings.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ModuleScope {
    /// Package aliases and producer selection used in this module scope.
    pub packages: PackageScope,
    /// Explicit declaration dependencies, independent of authored use leaves.
    pub dependencies: BTreeSet<ModuleIdentity>,
    /// Implicit prelude selection; multiple selections are diagnosed as a conflict.
    pub preludes: BTreeSet<ModuleIdentity>,
}

/// Spell a standalone source's encoded family for an explicit module dependency.
pub(crate) fn source_package_alias(package: &PackageId) -> Option<String> {
    package
        .0
        .strip_prefix("source-file:")
        .map(|family| format!("__kagari_source_{family}"))
}

/// Explicit bindings shadow even a dependency's canonical package spelling.
/// Both graph lookup and the inverse operation (choosing a spelling) use this rule.
pub(crate) fn package_targets(
    scope: Option<&ModuleScope>,
    importer: &ModuleIdentity,
    name: &str,
) -> BTreeSet<PackageId> {
    if let Some(target) = scope.and_then(|scope| scope.packages.dependencies.get(name)) {
        return [target.clone()].into();
    }
    if name == importer.package.0 {
        return [importer.package.clone()].into();
    }
    scope
        .into_iter()
        .flat_map(|scope| &scope.dependencies)
        .filter(|id| {
            id.package.0 == name || source_package_alias(&id.package).as_deref() == Some(name)
        })
        .map(|id| id.package.clone())
        .collect()
}

/// Immutable module import selections supplied before graph construction.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ImportEnvironment {
    /// Import selections keyed by their actual importing module identity.
    pub modules: BTreeMap<ModuleIdentity, ModuleScope>,
}

impl ImportEnvironment {
    pub(crate) fn scope(&self, module: &ModuleIdentity) -> Option<&ModuleScope> {
        self.modules.get(module)
    }
}
