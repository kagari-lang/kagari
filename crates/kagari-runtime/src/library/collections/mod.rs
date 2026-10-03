//! Always-installed lazy adapters over language-owned collection storage.
mod mapping;
use crate::native::{
    binding::NativeResult, builder::ModuleBuilder, language::LanguageContracts,
    module::NativeModule,
};
use std::cell::OnceCell;

thread_local! {
    static MODULE: OnceCell<NativeResult<NativeModule>> = const { OnceCell::new() };
}

pub fn module() -> NativeResult<NativeModule> {
    MODULE.with(|module| module.get_or_init(build).clone())
}

fn build() -> NativeResult<NativeModule> {
    let language = LanguageContracts::default();
    let mut module = ModuleBuilder::new("std::collections", &language);
    mapping::register(&mut module, &language)?;
    module.finish()
}
