//! Immutable portable declarations for the compiler-owned language foundation.
//! Registration and tooling consume the same records; no Rust bodies live here.
mod collections;
mod contracts;
mod defaults;
use crate::{declaration::ModuleDecl, language};
use std::sync::{Arc, OnceLock};

pub fn declarations() -> ModuleDecl {
    let mut module = ModuleDecl::new(language::module_identity());
    module.package_alias = Some(language::SOURCE_PACKAGE.into());
    contracts::declare(&mut module);
    collections::declare(&mut module);
    defaults::declare(&mut module);
    module
}

pub fn shared() -> Arc<ModuleDecl> {
    static CONTRACTS: OnceLock<Arc<ModuleDecl>> = OnceLock::new();
    CONTRACTS.get_or_init(|| Arc::new(declarations())).clone()
}
