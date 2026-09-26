use super::*;
use crate::{hir::TypeKind, types::BuiltinType};
use kagari_common::{
    identity::{ModuleIdentity, PackageId},
    source_database::{SourceDatabase, SourceLayer},
};

fn snapshot(db: &mut AnalysisDatabase, sources: &SourceDatabase) -> AnalysisSnapshot {
    db.snapshot(sources.snapshot(), Default::default(), &Default::default())
        .unwrap()
}

#[test]
fn explicit_empty_applications_are_not_erased_to_bare_types() {
    for name in ["i32", "Point", "Mode", "View", "T"] {
        let text = format!(
            "struct Point {{}} enum Mode {{ Ready }} trait View {{}} fn bad<T>(value: {name}<>) {{}} fn good(x: i32) -> i32 {{ x }}"
        );
        let mut sources = SourceDatabase::default();
        let file = sources
            .set("empty.kgr", text.clone(), SourceLayer::Base)
            .unwrap();
        let analysis = snapshot(&mut AnalysisDatabase::default(), &sources);
        let file = analysis.file(file).unwrap();
        let facts = file.result().facts();
        let start = text.find(&format!("{name}<>")).unwrap();
        let reference = facts
            .lowered
            .module
            .functions
            .iter()
            .find(|f| f.name == "bad")
            .unwrap()
            .params[0]
            .ty;
        assert!(matches!(&facts.lowered.module.type_ref(reference).kind,
            TypeKind::Generic { name: base, args, .. } if base == name && args.is_empty()));
        assert_eq!(file.type_at(start), Some(TypeId::Error), "{name}");
        assert_eq!(
            file.definition_at(start).map(|d| d.name.as_str()),
            (name != "i32").then_some(name)
        );
        assert_eq!(
            file.type_at(text.rfind(" x }").unwrap() + 1),
            Some(TypeId::Builtin(BuiltinType::I32))
        );
        assert!(
            file.result()
                .diagnostics()
                .iter()
                .any(|d| d.kind.code() == "KG_TYPE_UNKNOWN_TYPE")
        );
        assert!(file.result().clone().into_codegen().is_err());
    }
}

#[test]
fn annotation_navigation_uses_type_names_not_application_punctuation() {
    let text = "// 中文 😀\r\nstruct Point {} struct Holder<T> { val value: T } fn inspect(value: Holder<Point>) -> [Point] { [Point {}] }";
    let mut sources = SourceDatabase::default();
    let id = sources
        .set("type-punctuation.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let snapshot = snapshot(&mut AnalysisDatabase::default(), &sources);
    let file = snapshot.file(id).unwrap();
    assert!(file.result().diagnostics().is_empty());

    let application = text.find("Holder<Point>").unwrap();
    assert_eq!(file.definition_at(application).unwrap().name, "Holder");
    assert!(file.definition_at(application + "Holder".len()).is_none());
    let argument = application + "Holder<".len();
    assert_eq!(file.definition_at(argument).unwrap().name, "Point");
    assert!(file.definition_at(argument + "Point".len()).is_none());
    let array = text.find("[Point]").unwrap();
    assert!(file.definition_at(array).is_none());
    assert_eq!(file.definition_at(array + 1).unwrap().name, "Point");
    assert!(file.definition_at(array + "[Point".len()).is_none());
}

#[test]
fn explicit_bindings_shadow_all_standard_type_constructors() {
    for (name, args) in [
        ("MutableMap", "i32, String"),
        ("Set", "i32"),
        ("Option", "i32"),
        ("Result", "i32, String"),
    ] {
        for declaration in [
            format!("struct {name} {{}}"),
            format!("enum {name} {{ Ready }}"),
            format!("trait {name} {{}}"),
            format!("fn {name}() {{}}"),
            format!("const {name}: i32 = 1;"),
            format!("use absent::{name};"),
        ] {
            let text = format!("{declaration} fn bad(x: {name}<{args}>) {{}}");
            let source = SourceFile::new("shadow.kgr", &text);
            let analysis = crate::analyze_source(&source, Default::default());
            let facts = analysis.facts();
            let function = facts
                .typed
                .functions
                .iter()
                .find(|f| f.name == "bad")
                .unwrap();
            assert_eq!(function.params[0].ty, TypeId::Error, "{text}");
            assert!(analysis.into_codegen().is_err(), "{text}");
        }
        let text = format!("fn bad<{name}>(x: {name}<{args}>) {{}}");
        let mut sources = SourceDatabase::default();
        let id = sources
            .set("binder.kgr", text.clone(), SourceLayer::Base)
            .unwrap();
        let analysis = snapshot(&mut AnalysisDatabase::default(), &sources);
        let file = analysis.file(id).unwrap();
        let application = text.find("x: ").unwrap() + 3;
        assert_eq!(file.type_at(application), Some(TypeId::Error));
        assert_eq!(
            file.definition_at(application)
                .unwrap()
                .location
                .range
                .start,
            "fn bad<".len()
        );
    }
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
    let analysis = snapshot(&mut AnalysisDatabase::default(), &sources);
    let file = analysis.file(root).unwrap();
    let start = text.find("Map<").unwrap();
    let base = file.definition_at(start).unwrap();
    assert_eq!(base.name, "Map");
    assert_eq!(base.location.file, library);
    assert_eq!(file.type_at(start), Some(TypeId::Error));
    assert!(file.definition_at(text.find("Missing").unwrap()).is_none());
    let argument = text.find(", Point>").unwrap() + 2;
    assert_eq!(file.definition_at(argument).unwrap().location.file, library);
    assert!(matches!(file.type_at(argument), Some(TypeId::Struct(_))));
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
    let mut db = AnalysisDatabase::default();
    let old = snapshot(&mut db, &sources);
    let edit = text.replace("fn before() {}", "fn before() { val n = 1; }");
    sources
        .set("edit.kgr", edit.clone(), SourceLayer::Overlay)
        .unwrap();
    let changed = snapshot(&mut db, &sources);
    let file = changed.file(id).unwrap();
    assert!(file.signatures_reused());
    let target = file.definition_at(edit.find("T<>").unwrap()).unwrap();
    assert_eq!(target.location.range.start, edit.find("bad<T").unwrap() + 4);
    assert_eq!(file.type_at(edit.find("T<>").unwrap()), Some(TypeId::Error));
    let fresh = snapshot(&mut AnalysisDatabase::default(), &sources);
    file.result()
        .facts()
        .typed
        .type_table
        .assert_same_source_facts(
            &fresh.file(id).unwrap().result().facts().typed.type_table,
            file.result().facts().lowered.module.body.arena(),
            fresh
                .file(id)
                .unwrap()
                .result()
                .facts()
                .lowered
                .module
                .body
                .arena(),
        );
    sources
        .set("edit.kgr", edit.replace("T<>", "T"), SourceLayer::Overlay)
        .unwrap();
    let fixed = snapshot(&mut db, &sources);
    assert!(!fixed.file(id).unwrap().signatures_reused());
    assert!(fixed.file(id).unwrap().result().diagnostics().is_empty());
    assert_eq!(
        old.file(id).unwrap().type_at(text.find("T<>").unwrap()),
        Some(TypeId::Error)
    );
}
