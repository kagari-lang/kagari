//! Always-installed lazy adapters over language-owned collection storage.
mod mapping;
use crate::native::{
    binding::NativeResult, builder::ModuleBuilder, language::LanguageContracts,
    module::NativeModule,
};

pub fn module() -> NativeResult<NativeModule> {
    let language = LanguageContracts::default();
    let mut module = ModuleBuilder::new("std::collections", &language);
    mapping::register(&mut module, &language)?;
    module.finish()
}
