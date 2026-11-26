//! Source-enabled test preparation; the installed Runtime receives only Native modules.
use super::StandardLibrary;
use kagari_compiler::source::core_library;
use kagari_hir::analysis::core_source::packaged_modules;
use kagari_runtime::native::catalog::DeclarationCatalog;
use std::sync::LazyLock;

pub(crate) fn seed() -> DeclarationCatalog {
    DeclarationCatalog::default()
        .with_verified_modules(
            core_library::verified().unwrap(),
            packaged_modules().iter().map(|item| item.module_identity()),
        )
        .unwrap()
}

pub(crate) static STANDARD: LazyLock<StandardLibrary> =
    LazyLock::new(|| StandardLibrary::new(&seed()).unwrap());
