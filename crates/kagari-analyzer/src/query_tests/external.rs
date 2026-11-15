//! Query integration samples the source-backed offline route; HIR owns semantic matrices.

use crate::AnalyzerQueries;
use kagari_common::identity::{ModuleIdentity, PackageId};
use kagari_hir::{
    analysis::{AnalysisDatabase, external::ExternalModule},
    declarations::DeclarationId,
    semantic::signatures::FunctionImplementation,
    types::TypeId,
};
use kagari_source::{
    source::SourceFile,
    source_database::{SourceDatabase, SourceLayer, SourceSnapshot},
};
use kagari_types::scalar::BuiltinType;
use std::sync::Arc;

// One semantic fixture owner; these assertions test navigation/call/documentation
// projections rather than duplicating the HIR mutability and error matrix.
const LIBRARY: &str =
    include_str!("../../../kagari-hir/tests/fixtures/analysis/external/library.kgr");
const CLIENT: &str =
    include_str!("../../../kagari-hir/tests/fixtures/analysis/external/client.kgr");

#[test]
fn offline_native_queries_retain_real_declarations_and_complete_script_origins() {
    let mut sources = SourceDatabase::new("/exports").unwrap();
    sources
        .bind_module(
            "api.kgr",
            ModuleIdentity {
                package: PackageId("api".into()),
                path: vec![],
            },
        )
        .unwrap();
    let library = sources
        .set("api.kgr", LIBRARY.into(), SourceLayer::Base)
        .unwrap();
    let source = sources.snapshot().file(library).unwrap().clone();
    let client = Arc::new(SourceFile::new("/project/main.kgr", CLIENT));
    let root = client.id();
    let mut db = AnalysisDatabase::new("/project").unwrap();
    db.set_external_modules(vec![ExternalModule::new(source.clone())]);
    let snapshot = db
        .snapshot(SourceSnapshot::single_file(client), &Default::default())
        .unwrap();
    let file = snapshot.file(root).unwrap();
    assert!(
        file.result().diagnostics().is_empty(),
        "{:?}",
        file.result().diagnostics()
    );
    let queries = file.queries();
    let child_offset = CLIENT.find("counter.child").unwrap() + "counter.".len();
    let child = snapshot
        .queries()
        .definition_at(root, child_offset)
        .unwrap();
    assert_eq!(child.name, "child");
    assert_eq!(child.source_location().unwrap().file, library);
    assert_eq!(
        &LIBRARY[child.source_location().unwrap().range.start
            ..child.source_location().unwrap().range.end],
        "child"
    );
    assert_eq!(
        queries
            .field_at(child_offset)
            .map(DeclarationId::Definition)
            .as_ref(),
        Some(&child.id)
    );
    assert!(matches!(
        queries.type_at(child_offset),
        Some(TypeId::Struct(_))
    ));

    let new_offset = CLIENT.find("Counter::new").unwrap() + "Counter::".len();
    let new = queries.source_function_at(new_offset).unwrap();
    assert_eq!(
        new.signature.implementation,
        FunctionImplementation::NativeDeclaration
    );
    assert_eq!(new.site.source_location().unwrap().file, library);
    let docs = snapshot
        .queries()
        .documentation_at(root, new_offset)
        .unwrap();
    assert!(docs.documentation.contains("offline executable binding"));
    assert!(
        docs.written_signature
            .as_deref()
            .unwrap()
            .contains("native fn new")
    );
    assert_eq!(docs.declaration, new.site);

    let echo = queries
        .call_signature_at(CLIENT.find("echo(true)").unwrap())
        .unwrap();
    assert_eq!(
        echo.parameters,
        vec![("value".into(), TypeId::Builtin(BuiltinType::Bool))]
    );
    assert_eq!(echo.result, TypeId::Builtin(BuiltinType::Bool));
    for witness in ["read()", "defaulted()", "embedded(kept)"] {
        let signature = queries
            .call_signature_at(CLIENT.find(witness).unwrap())
            .unwrap();
        assert_eq!(signature.result, TypeId::Builtin(BuiltinType::I32));
    }
    let candidates = queries.method_completions(CLIENT.find("counter.read").unwrap());
    assert!(candidates.iter().any(|candidate| candidate.name == "echo"));
    assert!(
        candidates
            .iter()
            .any(|candidate| candidate.name == "defaulted")
    );
    assert!(
        !candidates
            .iter()
            .any(|candidate| candidate.name == "hidden")
    );

    let signature_file = snapshot.signature_snapshot().file(library).unwrap();
    let target = signature_file
        .queries()
        .definition_at(LIBRARY.find("child: Child").unwrap() + "child: ".len())
        .unwrap();
    assert_eq!(target.name, "Child");
    assert_eq!(target.source_location().unwrap().file, library);
    let default_offset = LIBRARY.find("defaulted(self)").unwrap();
    let default = snapshot
        .file(library)
        .unwrap()
        .queries()
        .definition_at(default_offset)
        .unwrap();
    assert_eq!(default.name, "defaulted");
    assert_eq!(default.source_location().unwrap().file, library);
    drop(db);
    assert_eq!(
        snapshot
            .source(library)
            .unwrap()
            .span(docs.declaration.source_location().unwrap().range),
        Some(docs.declaration.source_location().unwrap())
    );
}
