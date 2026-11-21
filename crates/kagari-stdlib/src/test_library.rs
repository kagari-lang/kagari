//! Source-enabled fixtures only. Production callers supply their own verified seed.
use crate::{StandardLibrary, declarations::StandardDeclarations};
#[path = "../tests/standard_library.rs"]
mod standard_library;
use kagari_runtime::native::{
    binding::NativeResult, catalog::DeclarationCatalog, module::NativeModule,
};
use standard_library::STANDARD;

pub(crate) fn seed() -> DeclarationCatalog {
    standard_library::seed()
}

pub(crate) fn standard() -> &'static StandardDeclarations {
    STANDARD.declarations()
}

pub(crate) fn modules() -> NativeResult<Vec<NativeModule>> {
    STANDARD.modules()
}
