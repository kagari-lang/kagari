//! Optional algorithms over the language-owned ArrayList storage.
mod mapping;
mod sorting;
use crate::native::{
    binding::NativeResult, builder::ModuleBuilder, language::LanguageContracts,
    module::NativeModule,
};

pub fn module() -> NativeResult<NativeModule> {
    let language = LanguageContracts::default();
    let mut module = ModuleBuilder::new("std::collections", &language);
    sorting::register(&mut module, &language)?;
    mapping::register(&mut module, &language)?;
    module.finish()
}
