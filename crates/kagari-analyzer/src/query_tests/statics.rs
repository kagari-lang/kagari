use crate::{AnalyzerQueries, query_tests::test_analysis};
use kagari_common::identity::{ModuleIdentity, PackageId};
use kagari_hir::{
    analysis::eligibility::BodyEligibility, resolver::resolved::ResolvedName, types::TypeId,
};
use kagari_source::{
    identity::FileId,
    source_database::{SourceDatabase, SourceLayer},
};
use kagari_types::scalar::BuiltinType;

fn insert(sources: &mut SourceDatabase, name: &str, text: &str) -> FileId {
    let path = format!("mem://{name}");
    sources
        .bind_module(
            &path,
            ModuleIdentity {
                package: PackageId("pkg".into()),
                path: name.split("::").map(str::to_owned).collect(),
            },
        )
        .unwrap();
    sources.set(&path, text.into(), SourceLayer::Base).unwrap()
}

#[test]
fn static_imports_reexports_inline_modules_and_navigation_keep_original_identity() {
    let mut sources = SourceDatabase::default();
    let owner_text = "pub static var counter: i32 = seed(); pub static val fixed: i32 = 4; fn seed() -> i32 { 1 }";
    let owner = insert(&mut sources, "state", owner_text);
    insert(
        &mut sources,
        "facade",
        "pub use pkg::state::counter as exposed;",
    );
    let text = "use pkg::facade::exposed as count; static val start: i32 = count; fn run() -> i32 { count += 1; count } mod child {
    use super::*; pub static val nested: i32 = 2; } fn nested() -> i32 { child::nested }";
    let root = insert(&mut sources, "root", text);
    let snapshot = test_analysis()
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    snapshot.check_program(root, &Default::default()).unwrap();
    let file = snapshot.file(root).unwrap();
    let target = snapshot
        .queries()
        .definition_at(root, text.rfind("count }").unwrap())
        .unwrap();
    assert_eq!(target.source_location().unwrap().file, owner);
    assert_eq!(
        target.source_location().unwrap().range.start,
        owner_text.find("counter").unwrap()
    );
    assert_eq!(
        file.queries().definition_at(text.rfind("count }").unwrap()),
        Some(target)
    );
    assert_eq!(
        file.queries().type_at(text.rfind("count }").unwrap()),
        Some(TypeId::Builtin(BuiltinType::I32))
    );
    for reference in [
        "exposed as",
        "count; static",
        "count; fn",
        "count +=",
        "count }",
    ] {
        let offset = text.find(reference).unwrap();
        assert_eq!(
            snapshot.queries().definition_at(root, offset),
            Some(target),
            "{reference}"
        );
    }
    assert_eq!(
        file.queries().type_at(text.find("count; fn").unwrap()),
        Some(TypeId::Builtin(BuiltinType::I32))
    );
    let nested = file
        .queries()
        .definition_at(text.rfind("nested }").unwrap())
        .unwrap();
    assert_eq!(nested.source_location().unwrap().file, root);
    assert_eq!(
        nested.source_location().unwrap().range.start,
        text.find("nested: i32").unwrap()
    );
    let facts = file.result().facts();
    let references: Vec<_> = facts
        .lowered
        .module
        .nodes
        .expressions()
        .filter_map(|(id, _)| match facts.names.expr_resolution(id) {
            Some(ResolvedName::Source(source)) if facts.imported_statics.get(&source).is_some() => {
                Some(source)
            }
            _ => None,
        })
        .collect();
    assert!(references.len() >= 2);
    assert_eq!(references[0], references[1]);
}

#[test]
fn static_initializer_scopes_navigation_and_recovered_method_completion() {
    let text = r#"
struct Packet { val value: i32 }
impl Packet { fn read(self) -> i32 { self.value } }
static val SEED: i32 = 1;
static val RESULT: i32 = {
    val value = SEED;
    val value = value + 1;
    val callback = |argument: i32| value + argument;
    callback(value)
};
static val EDITING: i32 = { val packet = Packet { value: SEED }; packet. };
"#;
    let mut sources = SourceDatabase::default();
    let root = insert(&mut sources, "tooling", text);
    let snapshot = test_analysis()
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    let file = snapshot.file(root).unwrap();
    let outer = file
        .queries()
        .definition_at(text.find("value + 1").unwrap())
        .unwrap();
    assert_eq!(
        outer.source_location().unwrap().range.start,
        text.find("val value = SEED").unwrap() + 4
    );
    let capture_offset = text.find("value + argument").unwrap();
    let inner = file.queries().definition_at(capture_offset).unwrap();
    assert_ne!(outer.id, inner.id);
    let visible = file.queries().visible_bindings(capture_offset);
    for name in ["value", "argument"] {
        let binding = visible.iter().find(|b| b.declaration.name == name).unwrap();
        assert_eq!(binding.ty, TypeId::Builtin(BuiltinType::I32), "{name}");
    }
    assert_eq!(
        file.queries()
            .definition_at(text.find("callback(value)").unwrap() + 9),
        Some(inner)
    );
    let candidates = file
        .queries()
        .method_completions(text.find("packet. }").unwrap() + 7);
    let read = candidates
        .iter()
        .find(|candidate| candidate.name == "read")
        .unwrap();
    let method = snapshot.declaration(&read.declaration).unwrap();
    assert_eq!(
        method.source_location().unwrap().range.start,
        text.find("read(self)").unwrap()
    );
    assert_eq!(
        file.queries()
            .definition_at(text.find("value: SEED").unwrap())
            .unwrap()
            .source_location()
            .unwrap()
            .range
            .start,
        text.find("value: i32").unwrap()
    );
    // Recoverable navigation facts do not authorize semantic lints when shared
    // parser inputs are invalid, even for the otherwise healthy initializer.
    assert!(!file.declarations_eligible());
    let statics = &file.result().facts().lowered.module.statics;
    assert_eq!(
        file.static_initializer_eligibility(statics[1].id),
        BodyEligibility::Unavailable
    );
    assert_eq!(
        file.static_initializer_eligibility(statics[2].id),
        BodyEligibility::Unavailable
    );
}
