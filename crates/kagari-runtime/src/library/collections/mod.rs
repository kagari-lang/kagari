//! Always-installed lazy adapters over language-owned collection storage.
mod mapping;
use crate::native::{binding::NativeResult, builder::ModuleBuilder, language::LanguageContracts};

pub(crate) fn register(
    module: &mut ModuleBuilder,
    language: &LanguageContracts,
) -> NativeResult<()> {
    mapping::register(module, language)
}
