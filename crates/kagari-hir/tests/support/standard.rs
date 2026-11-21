//! Verified stdlib authoring input for tests and benches below the compiler boundary.
//! Regenerate from the workspace root with `cargo run -p kagari-compiler --example
//! export_core_fixture -- crates/kagari-hir/tests/support/core.kbc`.
use kagari_bytecode::{artifact::KbcArtifact, program::verified::VerifiedBytecodeProgram};
use kagari_common::identity::{map::DefinitionContext, metadata::DefinitionMetadata};
use kagari_runtime::native::catalog::DeclarationCatalog;
use kagari_stdlib::{StandardLibrary, declarations::StandardDeclarations};
use kagari_types::declaration::module::ModuleDecl;
use std::sync::{Arc, LazyLock};

pub(crate) fn standard_modules() -> Vec<Arc<DefinitionMetadata<ModuleDecl>>> {
    static STANDARD: LazyLock<StandardDeclarations> = LazyLock::new(|| {
        let context = DefinitionContext::new().unwrap();
        let program = verified_core(&context);
        let owners = program
            .program()
            .modules
            .iter()
            .map(|module| &module.identity);
        let seed = DeclarationCatalog::default()
            .with_verified_modules(&program, owners)
            .unwrap();
        StandardLibrary::new(&seed).unwrap().declarations().clone()
    });
    STANDARD.declarations().to_vec()
}

pub(crate) fn verified_core(context: &DefinitionContext) -> VerifiedBytecodeProgram {
    let cancel = Default::default();
    KbcArtifact::from_bytes(include_bytes!("core.kbc"))
        .unwrap()
        .import_into(context, &cancel)
        .unwrap()
        .into_records()
        .into_verified(&Default::default(), context, &cancel)
        .unwrap()
        .into_bytecode()
}
