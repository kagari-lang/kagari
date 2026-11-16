//! Explicit import-scope inputs, independent of declaration representation.

use kagari_common::identity::ModuleIdentity;
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
