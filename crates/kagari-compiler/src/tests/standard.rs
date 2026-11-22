use crate::source::core_library;
use kagari_common::identity::metadata::DefinitionMetadata;
use kagari_runtime::native::{catalog::DeclarationCatalog, module::NativeModule};
use kagari_stdlib::{StandardLibrary, declarations::StandardDeclarations, io};
use kagari_types::declaration::module::ModuleDecl;
use std::sync::{Arc, LazyLock};

pub(crate) fn standard_modules() -> Vec<Arc<DefinitionMetadata<ModuleDecl>>> {
    static STANDARD: LazyLock<StandardDeclarations> = LazyLock::new(|| {
        StandardLibrary::new(&core_catalog())
            .unwrap()
            .declarations()
            .clone()
    });
    STANDARD.declarations().to_vec()
}

pub(crate) fn io_module() -> &'static NativeModule {
    static IO: LazyLock<NativeModule> =
        LazyLock::new(|| io::module(&core_catalog(), |_, _, _| Ok(())).unwrap());
    &IO
}

fn core_catalog() -> DeclarationCatalog {
    let program = core_library::verified().unwrap();
    let owners = program
        .program()
        .modules
        .iter()
        .map(|module| &module.identity);
    DeclarationCatalog::default()
        .with_verified_modules(program, owners)
        .unwrap()
}
