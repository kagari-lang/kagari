use kagari_common::{
    cancellation::CancellationToken,
    identity::{FileId, ModuleIdentity, PackageId},
    source_database::SourceLayer,
};
use kagari_embed::{CompileOptions, EmbeddingError, KagariEngine};

fn insert(engine: &KagariEngine, name: &str, text: &str) -> FileId {
    let source = format!("mem://{name}");
    engine
        .bind_module(
            &source,
            ModuleIdentity {
                package: PackageId("pkg".into()),
                path: vec![name.into()],
            },
        )
        .unwrap();
    engine
        .set_source(&source, text.into(), SourceLayer::Base)
        .unwrap()
}

#[test]
fn reachable_cycle_diagnostics_keep_the_dependency_file_and_revision() {
    let engine = KagariEngine::default();
    let root = insert(&engine, "root", "use pkg::a; fn main() -> i32 { 7 }");
    let a = insert(&engine, "a", "use pkg::b;");
    let b = insert(&engine, "b", "use pkg::a;");
    let sources = engine.source_snapshot();
    let error = engine
        .compile_snapshot(
            sources.clone(),
            root,
            CompileOptions::default(),
            &CancellationToken::default(),
        )
        .unwrap_err();
    let EmbeddingError::Diagnostics { diagnostics } = error else {
        panic!("expected source diagnostics");
    };
    assert_eq!(diagnostics.len(), 2);
    for diagnostic in diagnostics {
        assert_eq!(diagnostic.code, "KG_RESOLVE_CYCLIC_IMPORT");
        let location = diagnostic.span.unwrap();
        assert!([a, b].contains(&location.file));
        assert!(sources.contains(location));
    }
    let independent = insert(&engine, "independent", "fn main() -> i32 { 42 }");
    assert!(
        engine
            .compile_snapshot(
                engine.source_snapshot(),
                independent,
                CompileOptions::default(),
                &CancellationToken::default()
            )
            .is_ok()
    );
}

#[test]
fn source_dependencies_cannot_be_omitted_from_single_module_artifacts() {
    let engine = KagariEngine::default();
    insert(&engine, "dependency", "pub fn value() -> i32 { 42 }");
    let root = insert(
        &engine,
        "root",
        "use pkg::dependency; fn main() -> i32 { 7 }",
    );
    let error = engine
        .compile_snapshot(
            engine.source_snapshot(),
            root,
            CompileOptions::default(),
            &CancellationToken::default(),
        )
        .unwrap_err();
    assert_eq!(error.code(), "KG_COMPILE_MODULE_LINK_REQUIRED");
}
