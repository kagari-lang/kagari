use crate::{
    AnalyzerQueries,
    query_tests::{common::assert_source_spelling, test_analysis},
};
use kagari_hir::{analysis::*, types::*};

use kagari_common::identity::{ModuleIdentity, PackageId};
use kagari_source::source_database::{SourceDatabase, SourceLayer};
use kagari_types::scalar::BuiltinType;

fn snapshot(db: &mut AnalysisDatabase, sources: &SourceDatabase) -> AnalysisSnapshot {
    db.snapshot(sources.snapshot(), &Default::default())
        .unwrap()
}

#[test]
fn core_type_annotations_resolve_aliases_and_qualified_source_declarations() {
    let text = include_str!("../../tests/fixtures/core-type-annotations.kgr");
    let mut sources = SourceDatabase::default();
    let id = sources
        .set("native-types.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let snapshot = snapshot(&mut test_analysis(), &sources);
    let file = snapshot.file(id).unwrap();
    assert!(
        file.result().diagnostics().is_empty(),
        "{:?}",
        file.result().diagnostics()
    );
    let annotation = |parameter: &str| text.find(parameter).unwrap() + parameter.len();
    for (left, right, uri, spelling) in [
        ("a: ", "b: ", "kagari-alloc:///string.kgr", "String"),
        ("a: ", "c: ", "kagari-alloc:///string.kgr", "String"),
        ("d: ", "e: ", "kagari-core:///option.kgr", "Option"),
        ("f: ", "g: ", "kagari-core:///ops.kgr", "RangeFull"),
    ] {
        let left = annotation(left);
        let right = annotation(right);
        let declaration = snapshot.queries().definition_at(id, left).unwrap();
        assert_eq!(
            declaration.id,
            snapshot.queries().definition_at(id, right).unwrap().id
        );
        assert_source_spelling(&snapshot, declaration, uri, spelling);
        assert_eq!(file.queries().type_at(left), file.queries().type_at(right));
        assert!(
            file.queries()
                .type_at(left)
                .is_some_and(|ty| !ty.is_unresolved())
        );
    }
    assert_eq!(
        file.queries().type_at(annotation("a: ")),
        Some(TypeId::Builtin(BuiltinType::String))
    );
}

#[test]
fn source_bindings_shadow_native_types_and_the_standard_namespace() {
    for name in ["String", "ParseError", "Ordering", "RangeFull"] {
        let text = format!("struct {name} {{}} fn inspect(value: {name}) {{}}");
        let mut sources = SourceDatabase::default();
        let id = sources
            .set("shadow.kgr", text.clone(), SourceLayer::Base)
            .unwrap();
        let snapshot = snapshot(&mut test_analysis(), &sources);
        let offset = text.find("value: ").unwrap() + "value: ".len();
        let file = snapshot.file(id).unwrap();
        assert!(file.result().diagnostics().is_empty(), "{text}");
        assert!(
            matches!(file.queries().type_at(offset), Some(TypeId::Struct(_))),
            "{text}"
        );
        assert_eq!(
            snapshot
                .queries()
                .definition_at(id, offset)
                .unwrap()
                .source_location()
                .unwrap()
                .file,
            id
        );
    }
    for binding in [
        "struct language {}",
        "use alloc::vec::Vec as language;",
        "use absent::language;",
    ] {
        let text = format!(
            "use std::ops::{{RangeFull}};\n{binding} fn bad(value: language::RangeFull) {{}}"
        );
        let mut sources = SourceDatabase::default();
        let id = sources
            .set("namespace-shadow.kgr", text.clone(), SourceLayer::Base)
            .unwrap();
        let snapshot = snapshot(&mut test_analysis(), &sources);
        let offset = text.find("value: ").unwrap() + "value: ".len();
        assert_eq!(
            snapshot.file(id).unwrap().queries().type_at(offset),
            Some(TypeId::Error),
            "{text}"
        );
        assert!(
            snapshot
                .file(id)
                .unwrap()
                .result()
                .clone()
                .into_codegen()
                .is_err()
        );
    }
}

#[test]
fn nested_namespace_types_keep_distinct_declaration_identities() {
    let mut sources = SourceDatabase::default();
    for name in ["left", "right", "facade", "main"] {
        sources
            .bind_module(
                name,
                ModuleIdentity {
                    package: PackageId("pkg".into()),
                    path: vec![name.into()],
                },
            )
            .unwrap();
    }
    let left = sources
        .set("left", "pub struct Same {}".into(), SourceLayer::Base)
        .unwrap();
    let right = sources
        .set("right", "pub struct Same {}".into(), SourceLayer::Base)
        .unwrap();
    sources
        .set(
            "facade",
            "pub use pkg::left; pub use pkg::right;".into(),
            SourceLayer::Base,
        )
        .unwrap();
    let text = "use pkg::facade as library; fn inspect(a: library::left::Same, b: library::right::Same) {}";
    let id = sources.set("main", text.into(), SourceLayer::Base).unwrap();
    let snapshot = snapshot(&mut test_analysis(), &sources);
    let file = snapshot.file(id).unwrap();
    assert!(
        file.result().diagnostics().is_empty(),
        "{:?}",
        file.result().diagnostics()
    );
    let a = text.find("library::left::Same").unwrap();
    let b = text.find("library::right::Same").unwrap();
    assert_eq!(
        snapshot
            .queries()
            .definition_at(id, a)
            .unwrap()
            .source_location()
            .unwrap()
            .file,
        left
    );
    assert_eq!(
        snapshot
            .queries()
            .definition_at(id, b)
            .unwrap()
            .source_location()
            .unwrap()
            .file,
        right
    );
    assert!(matches!(file.queries().type_at(a), Some(TypeId::Struct(_))));
    assert!(matches!(file.queries().type_at(b), Some(TypeId::Struct(_))));
    assert_ne!(file.queries().type_at(a), file.queries().type_at(b));
}

#[test]
fn annotation_navigation_uses_type_names_not_application_punctuation() {
    let text = "// 中文 😀\r\nstruct Point {} struct Holder<T> { val value: T } fn inspect(value: Holder<Point>) -> [Point] { [Point {}] }";
    let mut sources = SourceDatabase::default();
    let id = sources
        .set("type-punctuation.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let snapshot = snapshot(&mut test_analysis(), &sources);
    let file = snapshot.file(id).unwrap();
    assert!(file.result().diagnostics().is_empty());

    let application = text.find("Holder<Point>").unwrap();
    assert_eq!(
        file.queries().definition_at(application).unwrap().name,
        "Holder"
    );
    assert!(
        file.queries()
            .definition_at(application + "Holder".len())
            .is_none()
    );
    let argument = application + "Holder<".len();
    assert_eq!(
        file.queries().definition_at(argument).unwrap().name,
        "Point"
    );
    assert!(
        file.queries()
            .definition_at(argument + "Point".len())
            .is_none()
    );
    let array = text.find("[Point]").unwrap();
    assert!(file.queries().definition_at(array).is_none());
    assert_eq!(
        file.queries().definition_at(array + 1).unwrap().name,
        "Point"
    );
    assert!(
        file.queries()
            .definition_at(array + "[Point".len())
            .is_none()
    );
}

#[test]
fn invalid_imported_application_retains_base_and_all_argument_facts() {
    let mut sources = SourceDatabase::default();
    for name in ["lib", "facade", "main"] {
        sources
            .bind_module(
                name,
                ModuleIdentity {
                    package: PackageId("pkg".into()),
                    path: vec![name.into()],
                },
            )
            .unwrap();
    }
    let library = sources
        .set(
            "lib",
            "pub struct Map {} pub struct Point {}".into(),
            SourceLayer::Base,
        )
        .unwrap();
    sources
        .set(
            "facade",
            "pub use pkg::lib::Map; pub use pkg::lib::Point;".into(),
            SourceLayer::Base,
        )
        .unwrap();
    let text = "use pkg::facade::Map; use pkg::facade::Point; fn bad(x: Map<Missing, Point>) {} fn good(p: Point) -> Point { p }";
    let root = sources.set("main", text.into(), SourceLayer::Base).unwrap();
    let analysis = snapshot(&mut test_analysis(), &sources);
    let file = analysis.file(root).unwrap();
    let start = text.find("Map<").unwrap();
    let base = file.queries().definition_at(start).unwrap();
    assert_eq!(base.name, "Map");
    assert_eq!(base.source_location().unwrap().file, library);
    assert_eq!(file.queries().type_at(start), Some(TypeId::Error));
    assert!(
        file.queries()
            .definition_at(text.find("Missing").unwrap())
            .is_none()
    );
    let argument = text.find(", Point>").unwrap() + 2;
    assert_eq!(
        file.queries()
            .definition_at(argument)
            .unwrap()
            .source_location()
            .unwrap()
            .file,
        library
    );
    assert!(matches!(
        file.queries().type_at(argument),
        Some(TypeId::Struct(_))
    ));
    assert_eq!(file.result().diagnostics().len(), 1);
    assert!(file.result().clone().into_codegen().is_err());
}

#[test]
fn erroneous_applications_rebase_signature_targets_and_repair_without_stale_errors() {
    let text = "fn before() {} fn bad<T>(x: T<>) {}";
    let mut sources = SourceDatabase::default();
    let id = sources
        .set("edit.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let mut db = test_analysis();
    let old = snapshot(&mut db, &sources);
    let edit = text.replace("fn before() {}", "fn before() { val n = 1; }");
    sources
        .set("edit.kgr", edit.clone(), SourceLayer::Overlay)
        .unwrap();
    let changed = snapshot(&mut db, &sources);
    let file = changed.file(id).unwrap();
    assert!(file.signatures_reused());
    let target = file
        .queries()
        .definition_at(edit.find("T<>").unwrap())
        .unwrap();
    assert_eq!(
        target.source_location().unwrap().range.start,
        edit.find("bad<T").unwrap() + 4
    );
    assert_eq!(
        file.queries().type_at(edit.find("T<>").unwrap()),
        Some(TypeId::Error)
    );
    let _fresh = snapshot(&mut test_analysis(), &sources);

    sources
        .set("edit.kgr", edit.replace("T<>", "T"), SourceLayer::Overlay)
        .unwrap();
    let fixed = snapshot(&mut db, &sources);
    assert!(!fixed.file(id).unwrap().signatures_reused());
    assert!(fixed.file(id).unwrap().result().diagnostics().is_empty());
    assert_eq!(
        old.file(id)
            .unwrap()
            .queries()
            .type_at(text.find("T<>").unwrap()),
        Some(TypeId::Error)
    );
}
