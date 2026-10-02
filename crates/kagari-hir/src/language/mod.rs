//! Compiler-owned language declarations. The portable records are authoritative;
//! generated text is used only for spans, navigation and completion.
mod collections;
mod contracts;
mod defaults;
pub mod semantics;

use kagari_abi::{language, native_api::NativeModule};

pub fn declarations() -> NativeModule {
    let mut module = NativeModule::new(language::module_identity());
    module.package_alias = Some("core".into());
    contracts::declare(&mut module);
    collections::declare(&mut module);
    defaults::declare(&mut module);
    module
}
