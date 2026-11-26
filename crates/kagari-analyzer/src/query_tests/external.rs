//! Query integration samples the source-backed offline route; HIR owns semantic matrices.

use crate::{
    AnalyzerQueries,
    query_tests::{native_fixture::function_module, publish_native_documents},
};
use kagari_common::{
    identity::{DefinitionKind, ModuleIdentity, PackageId},
    span::Span,
};
use kagari_hir::{
    analysis::AnalysisDatabase,
    declarations::DeclarationId,
    imports::environment::{ImportEnvironment, ModuleScope},
    semantic::signatures::FunctionImplementation,
    types::TypeId,
};
use kagari_source::{
    diagnostic::DiagnosticKind,
    source_database::{SourceDatabase, SourceLayer},
};
use kagari_types::{scalar::BuiltinType, ty::Ty};
use std::slice;

// One semantic fixture owner; these assertions test navigation/call/documentation
// projections rather than duplicating the HIR mutability and error matrix.
const LIBRARY: &str =
    include_str!("../../../kagari-hir/tests/fixtures/analysis/external/library.kgr");
const CLIENT: &str =
    include_str!("../../../kagari-hir/tests/fixtures/analysis/external/client.kgr");

#[test]
fn score_selects_number_from_registration_or_generated_document() {
    const SCORE: &str = include_str!("../../tests/fixtures/score.kgr");
    let api = ModuleIdentity {
        package: PackageId("game".into()),
        path: vec!["api".into()],
    };
    let provider = function_module(api.clone(), "number", vec![], Ty::Builtin(BuiltinType::I32));

    let cancel = Default::default();
    let [registered, from_text] = [false, true].map(|editor| {
        let mut db = AnalysisDatabase::default();
        let mut sources = SourceDatabase::default();
        sources
            .bind_module(
                "score.kgr",
                ModuleIdentity {
                    package: api.package.clone(),
                    path: vec!["client".into()],
                },
            )
            .unwrap();
        let root = sources
            .set("score.kgr", SCORE.into(), SourceLayer::Base)
            .unwrap();
        let expected_location = if editor {
            let (editor, views) =
                publish_native_documents(&mut sources, slice::from_ref(&provider));
            db = editor;
            let generated = &views[0];
            let document = sources.snapshot().module(&api).unwrap().id();
            let start = generated.text.find("fn number(").unwrap() + "fn ".len();
            Some(
                sources
                    .snapshot()
                    .file(document)
                    .unwrap()
                    .span(Span::new(start, start + "number".len()))
                    .unwrap(),
            )
        } else {
            db.set_native_modules(vec![provider.clone()]);
            None
        };
        let snapshot = db.snapshot(sources.snapshot(), &cancel).unwrap();
        if editor {
            assert!(snapshot.inputs().native_modules().is_empty());
        }
        let file = snapshot.file(root).unwrap();
        assert!(
            file.result().diagnostics().is_empty(),
            "{:?}",
            file.result().diagnostics()
        );
        snapshot.check_program(root, &cancel).unwrap();
        let offset = SCORE.find("number()").unwrap();
        let call = file.queries().call_signature_at(offset).unwrap();
        let definition = snapshot.queries().definition_at(root, offset).unwrap();
        assert_eq!(definition.id, call.declaration);
        assert_eq!(definition.source_location(), expected_location);
        let DeclarationId::Definition(id) = call.declaration else {
            panic!("expected a named function");
        };
        let path = file.definitions().resolve(id).unwrap().to_path();

        sources
            .set(
                "score.kgr",
                SCORE.replace("number()", "number(1)"),
                SourceLayer::Overlay,
            )
            .unwrap();
        let invalid = db.snapshot(sources.snapshot(), &cancel).unwrap();
        let diagnostics = invalid.file(root).unwrap().result().diagnostics();
        let [diagnostic] = diagnostics else {
            panic!("expected one arity diagnostic, got {diagnostics:?}");
        };
        assert_eq!(
            diagnostic.kind,
            DiagnosticKind::CallArityMismatch {
                function_name: "number".into(),
                expected: 0,
                found: 1,
            }
        );
        (path, call.result)
    });
    assert_eq!(registered, from_text);
    let (target, result) = registered;
    assert_eq!(target.module, api);
    assert!(
        matches!(target.path.as_slice(), [member] if member.kind == DefinitionKind::Function && member.name == "number")
    );
    assert_eq!(result, TypeId::Builtin(BuiltinType::I32));
}

#[test]
fn offline_native_queries_retain_real_declarations_and_complete_script_origins() {
    let mut sources = SourceDatabase::new("/exports").unwrap();
    let api = ModuleIdentity {
        package: PackageId("api".into()),
        path: vec![],
    };
    sources.bind_module("api.kgr", api.clone()).unwrap();
    let library = sources
        .set("api.kgr", LIBRARY.into(), SourceLayer::Base)
        .unwrap();
    let root = sources
        .set("/project/main.kgr", CLIENT.into(), SourceLayer::Base)
        .unwrap();
    let mut db = AnalysisDatabase::new("/project").unwrap();
    let sources = sources.snapshot();
    db.set_import_environment(ImportEnvironment {
        modules: [(
            sources.file(root).unwrap().module_identity().clone(),
            ModuleScope {
                dependencies: [api].into(),
                ..Default::default()
            },
        )]
        .into(),
    });
    let snapshot = db.snapshot(sources, &Default::default()).unwrap();
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
        FunctionImplementation::Native(Default::default())
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
