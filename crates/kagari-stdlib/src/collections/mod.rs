//! Always-installed lazy adapters over language-owned collection storage.
mod mapping;
use {
    crate::declarations::StandardDeclarations,
    kagari_runtime::native::{binding::NativeResult, builder::ModuleBuilder},
};

pub(crate) fn register(
    module: &mut ModuleBuilder,
    language: &StandardDeclarations,
) -> NativeResult<()> {
    mapping::register(module, language)
}
