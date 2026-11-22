//! Authored core evidence for source tests and an isolated role for synthetic bytecode.
use crate::native::{
    builder::ModuleBuilder, catalog::DeclarationCatalog, module::NativeModule,
    storage::NativeStorage,
};
use kagari_compiler::source::core_library;
use kagari_types::language::binding::future_declaration;

pub(crate) fn catalog() -> DeclarationCatalog {
    DeclarationCatalog::default()
        .with_verified_modules(
            core_library::verified().unwrap(),
            [&future_declaration().module],
        )
        .unwrap()
}

pub(crate) fn isolated_provider() -> NativeModule {
    let mut module = ModuleBuilder::new("test::isolated_future", &Default::default()).unwrap();
    let mut future = module.define_type("Future").unwrap();
    future.type_parameter("T").unwrap();
    future.native_storage(NativeStorage::future()).unwrap();
    future.finish().unwrap();
    module.finish().unwrap()
}
