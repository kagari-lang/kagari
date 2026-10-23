use super::*;
use crate::{
    declarations::DeclarationId, hir::expr::ExprKind, tests::test_analysis,
    typeck::scalar::ScalarValue,
};
use kagari_source::source_database::{SourceDatabase, SourceLayer};
use kagari_types::scalar::{BuiltinType, IntegerType};

#[test]
fn character_constants_and_patterns_keep_checked_scalar_facts() {
    let text = r"
const FIRST: char = 'a';
const LAST: char = 'z';
const ORDERED: bool = '\0' < '\u{10ffff}';
trait Marker { const VALUE: char = '🙂'; }
struct Letter {}
impl Marker for Letter {}
fn marker<T: Marker>(value: T) -> char { <T as Marker>::VALUE }
fn classify(value: char) -> char {
    match value { FIRST..=LAST => 'a', '🙂' => '🙂', _ => '\0' }
}
";
    let mut sources = SourceDatabase::default();
    let file = sources
        .set("characters.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let mut db = test_analysis();
    let snapshot = db
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    let result = snapshot.file(file).unwrap().result();
    assert!(
        result.diagnostics().is_empty(),
        "{:?}",
        result.diagnostics()
    );
    let facts = result.facts();
    for expected in [
        ScalarValue::Char('a'),
        ScalarValue::Char('z'),
        ScalarValue::Char('🙂'),
        ScalarValue::Bool(true),
    ] {
        assert!(
            facts
                .typed
                .const_values
                .values()
                .any(|value| value == &expected),
            "{expected:?}"
        );
    }
    let arms = facts
        .lowered
        .module
        .body
        .expressions()
        .find_map(|(_, expr)| {
            if let ExprKind::Match { arms, .. } = &expr.kind {
                Some(arms)
            } else {
                None
            }
        })
        .unwrap();
    assert_eq!(
        facts.typed.type_table.pattern_range(arms[0].pattern),
        Some(&(ScalarValue::Char('a'), ScalarValue::Char('z')))
    );
    assert_eq!(
        facts.typed.type_table.pattern_scalar_value(arms[1].pattern),
        Some(&ScalarValue::Char('🙂'))
    );
}

#[test]
fn character_typing_rejects_numeric_operations_casts_and_mixed_patterns() {
    for body in [
        "fn bad() -> char { 97u32 }",
        "fn bad() -> u32 { 'a' }",
        "fn bad() -> u32 { 'a' as u32 }",
        "fn bad() -> char { 97u32 as char }",
        "fn bad() -> char { 'a' + 'b' }",
        "fn bad() -> char { !'a' }",
        "fn bad() -> char { -'a' }",
        "fn bad() -> char { 'a' & 'b' }",
        "fn bad() -> bool { 'a' == 97u32 }",
        "fn bad(c: char) -> i32 { match c { 'a'..=122 => 1, _ => 0 } }",
        "fn bad(c: u32) -> i32 { match c { 'a' => 1, _ => 0 } }",
        "fn bad() { val r = 'a'..'z'; }",
    ] {
        let mut sources = SourceDatabase::default();
        let file = sources
            .set("characters.kgr", body.into(), SourceLayer::Base)
            .unwrap();
        let mut db = test_analysis();
        let snapshot = db
            .snapshot(sources.snapshot(), &Default::default())
            .unwrap();
        assert!(
            !snapshot
                .file(file)
                .unwrap()
                .result()
                .diagnostics()
                .is_empty(),
            "{body}"
        );
    }
}

#[test]
fn associated_constant_targets_survive_cached_body_rebasing_and_revision_changes() {
    let text = "trait Limit { const VALUE: i32 = 21; } struct N {} impl Limit for N {} fn edit() -> i32 { 1 } fn keep<T: Limit>(x: T) -> i32 { <T as Limit>::VALUE }";
    let mut sources = SourceDatabase::default();
    let file = sources
        .set("cache.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let mut db = test_analysis();
    let snapshot = db
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    let analysis = snapshot.file(file).unwrap();
    assert!(analysis.result().diagnostics().is_empty());
    let declaration = analysis
        .definition_at(text.rfind("VALUE").unwrap())
        .unwrap();
    assert_eq!(
        declaration.location.range.start,
        text.find("VALUE").unwrap()
    );
    assert_eq!(
        analysis.type_at(text.rfind("VALUE").unwrap()),
        Some(TypeId::Builtin(BuiltinType::I32))
    );
    let DeclarationId::Definition(owner) = &analysis
        .result()
        .facts()
        .declarations
        .iter()
        .find(|declaration| declaration.name == "keep")
        .unwrap()
        .id
    else {
        panic!("function identity")
    };
    let old = db
        .body(sources.snapshot(), owner, &Default::default())
        .unwrap()
        .unwrap();
    let id = old
        .lowered()
        .module
        .body
        .expressions()
        .find(|(id, _)| old.type_table().associated_const(*id).is_some())
        .unwrap()
        .0;
    let edited = text.replace("{ 1 }", "{ val n = 2; n }");
    sources
        .set("cache.kgr", edited.clone(), SourceLayer::Overlay)
        .unwrap();
    let reused = db
        .body(sources.snapshot(), owner, &Default::default())
        .unwrap()
        .unwrap();
    assert_eq!(reused.reused_bodies(), 1);
    assert!(reused.type_table().associated_const(id).is_none());
    assert!(
        reused
            .lowered()
            .module
            .body
            .expressions()
            .any(|(id, _)| reused.type_table().associated_const(id).is_some())
    );
    sources
        .set(
            "cache.kgr",
            edited.replace("= 21", "= 42"),
            SourceLayer::Overlay,
        )
        .unwrap();
    let snapshot = db
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    let facts = snapshot.file(file).unwrap().result().facts();
    assert!(
        facts
            .typed
            .const_values
            .values()
            .any(|value| value == &ScalarValue::integer(42, IntegerType::I32).unwrap())
    );
    assert!(
        analysis
            .result()
            .facts()
            .typed
            .const_values
            .values()
            .any(|value| value == &ScalarValue::integer(21, IntegerType::I32).unwrap())
    );
}
