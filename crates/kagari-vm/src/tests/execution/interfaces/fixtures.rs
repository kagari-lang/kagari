use kagari_common::identity::metadata::DefinitionMetadata;
use kagari_runtime::native::{builder::ModuleBuilder, module::NativeModule};
use kagari_stdlib::declarations::StandardDeclarations;
use kagari_types::{declaration::module::ModuleDecl, language::Protocol};

pub(super) fn closure_constraint_provider() -> NativeModule {
    let standard = StandardDeclarations::default();
    let source = standard
        .declarations()
        .unwrap()
        .iter()
        .find(|declaration| declaration.records().identity == Protocol::Fn.module())
        .unwrap();
    // Canonical operator roles are checked as a group. Keep their real schemas,
    // without importing unrelated foundation implementations and callbacks.
    let mut declaration = ModuleDecl::new(Protocol::Fn.module());
    declaration.prelude = true;
    declaration.traits = source.records().traits.clone();
    declaration.types = source.records().types.clone();
    let declaration = DefinitionMetadata::checked(
        source.definitions().clone(),
        declaration,
        &Default::default(),
    )
    .unwrap();
    ModuleBuilder::from_declaration(declaration, &Default::default(), Default::default())
        .unwrap()
        .finish()
        .unwrap()
}
