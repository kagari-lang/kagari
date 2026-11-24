use crate::{AnalyzerQueries, query_tests::test_analysis};
use kagari_common::identity::DefinitionPath;
use kagari_hir::analysis::{AnalysisDatabase, body::FunctionAnalysis};
use kagari_hir::{declarations::DeclarationId, types::TypeId};
use kagari_source::source_database::{SourceDatabase, SourceLayer};
use kagari_source::{diagnostic::DiagnosticKind, identity::FileId};
use kagari_types::scalar::BuiltinType;
use std::sync::Arc;

#[test]
fn signature_navigation_uses_checked_type_targets_before_body_analysis() {
    let text = "// 中文 😀\r\nstruct Box<T> { val data: T }\r\nfn bad(x: Box<Missing>) {}\r\nfn good<T>(value: Box<T>) -> Box<T> { missing }";
    let mut sources = SourceDatabase::default();
    let id = sources
        .set("signature-targets.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let mut db = test_analysis();
    let signatures = db
        .signatures(sources.snapshot(), &Default::default())
        .unwrap();
    let file = signatures.file(id).unwrap();
    assert_eq!(file.diagnostics().len(), 1);

    assert!(
        file.queries()
            .type_at(text.find("{ missing }").unwrap() + 2)
            .is_none()
    );
    let good_annotation = text.find("Box<T>)").unwrap();
    assert!(matches!(
        file.queries().type_at(good_annotation),
        Some(TypeId::Struct(_))
    ));
    let bad_annotation = text.find("Box<Missing>").unwrap();
    let structure = file.queries().definition_at(bad_annotation).unwrap();
    assert_eq!(structure.name, "Box");
    assert_eq!(
        &text[structure.source_location().unwrap().range.start
            ..structure.source_location().unwrap().range.end],
        "Box"
    );
    assert!(
        file.queries()
            .definition_at(bad_annotation + "Box".len())
            .is_none()
    );
    assert!(
        file.queries()
            .definition_at(text.find("Missing>").unwrap())
            .is_none()
    );
    assert_eq!(
        file.queries()
            .definition_at(text.find("fn good").unwrap() + 3)
            .unwrap()
            .name,
        "good"
    );
    assert_eq!(
        file.queries()
            .definition_at(text.find("Box<T>)").unwrap() + "Box<".len())
            .unwrap()
            .source_location()
            .unwrap()
            .range
            .start,
        text.find("good<T>").unwrap() + "good<".len()
    );
    assert!(
        file.queries()
            .definition_at(text.find("{ missing }").unwrap() + 2)
            .is_none()
    );

    let full = db
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    assert_eq!(
        full.file(id)
            .unwrap()
            .queries()
            .definition_at(bad_annotation),
        Some(structure)
    );
}

#[test]
fn selected_queries_route_only_their_body_and_keep_incomplete_receivers() {
    let text = include_str!("../../tests/fixtures/selected-body.kgr");
    let mut sources = SourceDatabase::default();
    let id = sources
        .set("selected.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let mut database = test_analysis();
    let declarations = database
        .declarations(sources.snapshot(), &Default::default())
        .unwrap();
    let file = declarations.file(id).unwrap();
    let member = file
        .queries()
        .member_at(text.find("value: i32").unwrap())
        .unwrap();
    assert_eq!(member.name, "value");
    assert!(
        file.queries()
            .member_at(text.find("missing").unwrap())
            .is_none()
    );
    let owner = |name: &str| {
        let declaration = file.declarations().iter().find(|d| d.name == name).unwrap();
        let DeclarationId::Definition(id) = declaration.id else {
            panic!("function identity")
        };
        id
    };
    let good = database
        .body(sources.snapshot(), &owner("good"), &Default::default())
        .unwrap()
        .unwrap();
    let queries = good.queries();
    assert_eq!(
        queries.type_at(text.rfind("answer").unwrap()),
        Some(TypeId::Builtin(BuiltinType::I32))
    );
    assert!(queries.type_at(text.find("missing").unwrap()).is_none());
    assert!(queries.type_at(text.find("41").unwrap()).is_none());
    let incomplete = database
        .body(
            sources.snapshot(),
            &owner("incomplete"),
            &Default::default(),
        )
        .unwrap()
        .unwrap();
    let receiver = incomplete
        .queries()
        .member_receiver_type(text.rfind("value.").unwrap() + "value.".len())
        .unwrap();
    assert!(matches!(receiver, TypeId::Struct(_)));
    assert!(
        incomplete
            .queries()
            .member_receiver_type(text.find("answer").unwrap())
            .is_none()
    );
}

#[test]
fn assignment_member_receivers_survive_errors_and_snapshot_revisions() {
    for target in ["p.inner.value", "p.inner.missing", "p.inner."] {
        let text = format!(
            "struct Inner {{ var value: i32 }} struct Outer {{ val inner: Inner }} fn edit(p: Outer) {{ {target} = 1; }} fn good() -> i32 {{ 42 }}"
        );
        let mut sources = SourceDatabase::default();
        let file = sources
            .set("place-member.kgr", text.clone(), SourceLayer::Base)
            .unwrap();
        let mut db = test_analysis();
        let edit = owner(&mut db, &sources, file, "edit");
        let original = query(&mut db, &sources, &edit);
        let offset = text.find(target).unwrap() + "p.inner.".len();
        let receiver = original
            .queries()
            .member_receiver_type(offset)
            .expect("assignment receiver");
        assert!(
            matches!(&receiver, TypeId::Struct(ty) if original.definitions().resolve(ty.declaration).unwrap().last().unwrap().name == "Inner")
        );
        let valid = target == "p.inner.value";
        assert_eq!(original.diagnostics().is_empty(), valid);
        sources
            .set(
                "place-member.kgr",
                format!("// moved 😀\r\n{text}"),
                SourceLayer::Overlay,
            )
            .unwrap();
        let moved = query(&mut db, &sources, &edit);

        assert_eq!(
            moved
                .queries()
                .member_receiver_type(offset + "// moved 😀\r\n".len()),
            Some(receiver.clone())
        );
        assert_eq!(
            original.queries().member_receiver_type(offset),
            Some(receiver)
        );
        let snapshot = db
            .snapshot(sources.snapshot(), &Default::default())
            .unwrap();
        assert_eq!(
            snapshot
                .file(file)
                .unwrap()
                .queries()
                .member_receiver_type(offset + "// moved 😀\r\n".len()),
            moved
                .queries()
                .member_receiver_type(offset + "// moved 😀\r\n".len())
        );
        assert_eq!(
            snapshot.check_program(file, &Default::default()).is_ok(),
            valid
        );
    }
}

#[test]
fn associated_definition_changes_refresh_selected_body_types_and_keep_old_snapshot() {
    let text = "trait Read { type Item; fn read(self) -> Self::Item; } struct N {} impl Read for N { type Item = i32; fn read(self) -> Self::Item { 42 } } fn read<R: Read>(r: R) -> R::Item { r.read() } fn main() -> i32 { read(N {}) }";
    let mut sources = SourceDatabase::default();
    let file = sources
        .set("associated-edit.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let mut db = test_analysis();
    let main = owner(&mut db, &sources, file, "main");
    let first = query(&mut db, &sources, &main);
    assert!(first.diagnostics().is_empty(), "{:?}", first.diagnostics());
    assert_eq!(
        first.queries().type_at(text.rfind("read(N").unwrap()),
        Some(TypeId::Builtin(BuiltinType::I32))
    );
    let edit = text
        .replace("type Item = i32", "type Item = bool")
        .replace("{ 42 }", "{ true }");
    sources
        .set("associated-edit.kgr", edit.clone(), SourceLayer::Overlay)
        .unwrap();
    let second = query(&mut db, &sources, &main);
    assert!(!second.diagnostics().is_empty());
    assert_eq!(
        second.queries().type_at(edit.rfind("read(N").unwrap()),
        Some(TypeId::Builtin(BuiltinType::Bool))
    );
    assert!(first.diagnostics().is_empty());
    let fresh = query(&mut test_analysis(), &sources, &main);
    assert_eq!(second.diagnostics(), fresh.diagnostics());
    let call = edit.rfind("read(N").unwrap();
    assert_eq!(
        second.queries().type_at(call),
        fresh.queries().type_at(call)
    );
}

#[test]
fn selected_local_function_reconstructs_static_ancestry_without_checking_parent() {
    let mut sources = SourceDatabase::default();
    let text = "fn outer(hidden: i32) { val broken = missing; fn inner() -> i32 { helper() } fn helper() -> i32 { 42 } }";
    let file = sources
        .set("local.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let mut db = test_analysis();
    let inner = owner(&mut db, &sources, file, "inner");
    let result = query(&mut db, &sources, &inner);
    assert!(
        result.diagnostics().is_empty(),
        "{:?}",
        result.diagnostics()
    );
    assert_eq!(
        result.queries().type_at(text.find("helper()").unwrap()),
        Some(TypeId::Builtin(BuiltinType::I32))
    );
    let visible = crate::scope::ScopeIndex::new(result.lowered(), result.names())
        .visible_bindings(text.find("helper()").unwrap());
    assert!(visible.iter().any(|binding| binding.name == "helper"));
    assert!(
        visible
            .iter()
            .all(|binding| binding.name != "hidden" && binding.name != "broken")
    );

    let complete = db
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    assert!(
        complete
            .file(file)
            .unwrap()
            .result()
            .diagnostics()
            .iter()
            .any(|d| matches!(&d.kind, DiagnosticKind::UnknownName { name } if name == "missing"))
    );
}

fn owner(
    database: &mut AnalysisDatabase,
    sources: &SourceDatabase,
    file: FileId,
    name: &str,
) -> DefinitionPath {
    let declarations = database
        .declarations(sources.snapshot(), &Default::default())
        .unwrap();
    let file = declarations.file(file).unwrap();
    let declaration = file.declarations().iter().find(|d| d.name == name).unwrap();
    let DeclarationId::Definition(id) = declaration.id else {
        panic!("function identity")
    };
    file.declarations()
        .definitions()
        .resolve(id)
        .unwrap()
        .to_path()
}

fn query(
    database: &mut AnalysisDatabase,
    sources: &SourceDatabase,
    owner: &DefinitionPath,
) -> Arc<FunctionAnalysis> {
    let declarations = database
        .declarations(sources.snapshot(), &Default::default())
        .unwrap();
    let id = declarations
        .files()
        .find_map(|file| file.declarations().definitions().lookup(owner))
        .unwrap();
    database
        .body(sources.snapshot(), &id, &Default::default())
        .unwrap()
        .unwrap()
}
