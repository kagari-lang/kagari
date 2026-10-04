//! Immutable portable declarations for the compiler-owned language foundation.
//! Registration and tooling consume the same records; no Rust bodies live here.
mod collections;
mod construction;
mod construction_defaults;
mod contracts;
mod defaults;
mod key;
mod list_methods;
mod partition;
mod strings;
use crate::library::namespaces;
use kagari_common::identity::ModuleIdentity;
use kagari_types::declaration::module::ModuleDecl;
use std::sync::{Arc, OnceLock};

// Assembly is transient authoring state. It is never installed, exported or
// validated as a module; partition assigns the final owners before publication.
fn assembly_identity() -> ModuleIdentity {
    namespaces::module("std", "__foundation")
}

pub fn declarations() -> Vec<ModuleDecl> {
    let mut module = ModuleDecl::new(assembly_identity());
    contracts::declare(&mut module);
    construction::declare(&mut module);
    collections::declare(&mut module);
    list_methods::declare(&mut module);
    defaults::declare(&mut module);
    construction_defaults::declare(&mut module);
    list_methods::configure_overrides(&mut module);
    strings::declare(&mut module);
    partition::finish(module)
}

pub fn shared() -> Vec<Arc<ModuleDecl>> {
    static CONTRACTS: OnceLock<Vec<Arc<ModuleDecl>>> = OnceLock::new();
    CONTRACTS
        .get_or_init(|| declarations().into_iter().map(Arc::new).collect())
        .clone()
}
