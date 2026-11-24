use crate::analysis::*;
use crate::tests::{common::import_facts, test_analysis};
use kagari_common::identity::map::DefinitionContext;
use kagari_source::{
    diagnostic::DiagnosticKind,
    source_database::{SourceDatabase, SourceLayer},
};

fn snapshot(db: &mut AnalysisDatabase, sources: &SourceDatabase) -> AnalysisSnapshot {
    db.snapshot(sources.snapshot(), &Default::default())
        .unwrap()
}

fn assert_fresh(file: &FileAnalysis, sources: &SourceDatabase) {
    let fresh_snapshot = snapshot(&mut test_analysis(), sources);
    let fresh = fresh_snapshot.file(file.source().id()).unwrap();
    file.signatures().as_ref().facts().assert_same_source_facts(
        file.definitions(),
        fresh.signatures().as_ref().facts(),
        fresh.definitions(),
        file.result().facts().lowered.module.nodes.arena(),
        fresh.result().facts().lowered.module.nodes.arena(),
    );
    assert_eq!(
        file.signatures().diagnostics(),
        fresh.signatures().diagnostics()
    );
    assert_eq!(file.result().diagnostics(), fresh.result().diagnostics());
    let context = DefinitionContext::new().unwrap();
    let authoring = import_facts(
        &file.result().facts().typed.type_table,
        file.definitions(),
        &context,
    );
    let authoring_fresh = import_facts(
        &fresh.result().facts().typed.type_table,
        fresh.definitions(),
        &context,
    );
    // Compare type-reference facts using each analysis's own local IDs.
    let types = |file: &FileAnalysis| {
        file.result()
            .facts()
            .lowered
            .module
            .nodes
            .types()
            .map(|(id, _)| id)
            .collect::<Vec<_>>()
    };
    let old_ids = types(file);
    let fresh_ids = types(fresh);
    assert_eq!(old_ids.len(), fresh_ids.len());
    for (id, fresh_id) in old_ids.into_iter().zip(fresh_ids) {
        assert_eq!(
            authoring.records().type_ref(id),
            authoring_fresh.records().type_ref(fresh_id)
        );
    }
}

#[test]
fn signature_types_and_parameter_spans_follow_each_source_revision() {
    let mut sources = SourceDatabase::default();
    let text = "fn first() -> i32 { 1 }\r\nstruct Point { var x: i32 }\r\ntrait Show { fn show(self) -> i32; }\r\nimpl Show for Point { fn show(self) -> i32 { self.x } }\r\nfn keep<T: Show>(value: T) -> T { value }\r\nfn read(p: Point) -> i32 { p.x }";
    let id = sources
        .set("signatures.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let mut db = test_analysis();
    let first = snapshot(&mut db, &sources);
    assert!(
        first.file(id).unwrap().result().diagnostics().is_empty(),
        "{:?}",
        first.file(id).unwrap().result().diagnostics()
    );

    let edit = text
        .replace(
            "{ 1 }",
            "{ val label: String = \"中文😀\"; val pair: (i32, bool) = (1, true); 2 }",
        )
        .replace("{ self.x }", "{ val added: [i32] = [2]; self.x }");
    sources
        .set("signatures.kgr", edit.clone(), SourceLayer::Overlay)
        .unwrap();
    let second = snapshot(&mut db, &sources);
    let file = second.file(id).unwrap();
    assert_fresh(file, &sources);
    let old = &first.file(id).unwrap().result().facts().lowered;
    let current = &file.result().facts().lowered;
    for function in file.result().facts().typed.signatures.functions() {
        for (index, parameter) in function.params.iter().enumerate() {
            assert_eq!(parameter.id.index(), index);
            let record = current.module.parameter(parameter.id);
            assert_eq!(record.name, parameter.name);
            let span = current.module.parameter(parameter.id).binding_span();
            assert_eq!(&edit[span.start..span.end], parameter.name);
            let old_parameter = old.module.functions[function.id.index()].params[index].id;
            assert_ne!(old_parameter, parameter.id);
            assert!(std::panic::catch_unwind(|| current.module.parameter(old_parameter)).is_err());
            assert_eq!(old.module.parameter(old_parameter).name, parameter.name);
        }
    }

    // The edited field type also changes the body-checking inputs.
    sources
        .set(
            "signatures.kgr",
            edit.replace("var x: i32", "var x: bool"),
            SourceLayer::Overlay,
        )
        .unwrap();
    let third = snapshot(&mut db, &sources);
    assert_fresh(third.file(id).unwrap(), &sources);
}

#[test]
fn signature_and_body_errors_follow_their_current_source_sites() {
    let mut sources = SourceDatabase::default();
    let text = "fn first() { missing; }\r\nfn bad(x: Absent) -> Absent { x }\r\npub fn generic<T>(x: T) -> T { x }";
    let id = sources
        .set("errors.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let mut db = test_analysis();
    let first = snapshot(&mut db, &sources);
    let edit = text
        .replace("missing;", "val note: String = \"中文😀\";")
        .replace("-> T { x }", "-> T { unknown }");
    sources
        .set("errors.kgr", edit.clone(), SourceLayer::Overlay)
        .unwrap();
    let second = snapshot(&mut db, &sources);
    assert_fresh(second.file(id).unwrap(), &sources);
    for (snapshot, source, missing_name) in [
        (&first, text, "missing"),
        (&second, edit.as_str(), "unknown"),
    ] {
        let file = snapshot.file(id).unwrap();
        let diagnostics = file.signatures().diagnostics();
        assert_eq!(diagnostics.len(), 2);
        for diagnostic in diagnostics {
            assert!(
                matches!(&diagnostic.kind, DiagnosticKind::UnknownType { type_name, .. } if type_name == "Absent")
            );
            let span = diagnostic.span.unwrap();
            assert_eq!(&source[span.start..span.end], "Absent");
        }
        let diagnostic = file.result().diagnostics().iter().find(|diagnostic| matches!(&diagnostic.kind, DiagnosticKind::UnknownName { name } if name == missing_name)).unwrap();
        let span = diagnostic.span.unwrap();
        assert_eq!(&source[span.start..span.end], missing_name);
    }
}
