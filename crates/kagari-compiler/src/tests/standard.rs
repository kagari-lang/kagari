use crate::source::core_library;
use kagari_common::identity::metadata::DefinitionMetadata;
use kagari_runtime::native::catalog::DeclarationCatalog;
use kagari_stdlib::{StandardLibrary, declarations::StandardDeclarations};
use kagari_types::declaration::module::ModuleDecl;
use std::sync::{Arc, LazyLock};

pub(crate) fn standard_modules() -> Vec<Arc<DefinitionMetadata<ModuleDecl>>> {
    static STANDARD: LazyLock<StandardDeclarations> = LazyLock::new(|| {
        let program = core_library::verified().unwrap();
        let owners = program
            .program()
            .modules
            .iter()
            .map(|module| &module.identity);
        let seed = DeclarationCatalog::default()
            .with_verified_modules(program, owners)
            .unwrap();
        StandardLibrary::new(&seed).unwrap().declarations().clone()
    });
    STANDARD.declarations().to_vec()
}
