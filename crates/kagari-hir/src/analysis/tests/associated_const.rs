use crate::types::*;
use crate::{
    hir::{expr::ExprKind, pattern::PatternKind},
    tests::test_analysis,
    typeck::scalar::ScalarValue,
};
use kagari_source::{
    diagnostic::DiagnosticKind,
    source_database::{SourceDatabase, SourceLayer},
};
use kagari_types::scalar::{BuiltinType, IntegerType};

#[test]
fn inherent_constant_checks_unused_definitions_and_rejects_unapplied_owners() {
    let mut sources = SourceDatabase::default();
    let file = sources
        .set(
            "limits.kgr",
            include_str!("../../../tests/fixtures/analysis/inherent-constant-errors.kgr").into(),
            SourceLayer::Base,
        )
        .unwrap();
    let snapshot = test_analysis()
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    let diagnostics = snapshot.file(file).unwrap().result().diagnostics();
    assert!(diagnostics.iter().any(|d| matches!(&d.kind, DiagnosticKind::ConstCycle { const_name } if const_name == "CYCLE")), "{diagnostics:?}");
    assert!(diagnostics.iter().any(|d| matches!(&d.kind, DiagnosticKind::InvalidConstInitializer { const_name, .. } if const_name == "MISMATCH")), "{diagnostics:?}");
    for (member, cause) in [
        ("MISSING", "initializer"),
        ("DUP", "overlapping"),
        ("WIDTH", "explicit type arguments"),
    ] {
        assert!(diagnostics.iter().any(|d| matches!(&d.kind, DiagnosticKind::InvalidAssociatedConst { name, reason } if name == member && reason.contains(cause))), "{diagnostics:?}");
    }
}

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
        .nodes
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
fn byte_literals_keep_u8_constants_patterns_and_typed_fresh_array_nodes() {
    let text = r##"
const FIRST: u8 = b'A';
trait Marker { const VALUE: u8 = b'\xFF'; }
struct Byte {}
impl Marker for Byte {}
fn marker<T: Marker>(value: T) -> u8 { <T as Marker>::VALUE }
fn classify(value: u8) -> u8 {
    match value { b'A' | b'Z' => FIRST, b'\xFF' => b'\0', _ => b'B' }
}
fn arrays() {
    val a = b"\x00\xFF";
    val empty = b"";
    val raw = br#"\xFF{name}"#;
    val raw_empty = br"";
}
"##;
    let mut sources = SourceDatabase::default();
    let file = sources
        .set("bytes.kgr", text.into(), SourceLayer::Base)
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
    let byte = |value| ScalarValue::integer(value, IntegerType::U8).unwrap();
    assert!(
        facts
            .typed
            .const_values
            .values()
            .any(|value| value == &byte(65))
    );
    let mut arrays = Vec::new();
    for (id, expr) in facts.lowered.module.nodes.expressions() {
        if let ExprKind::Array {
            elements,
            element_type,
        } = &expr.kind
        {
            assert_eq!(*element_type, Some(BuiltinType::U8));
            assert_eq!(
                facts.typed.type_table.expr_type(id),
                Some(TypeId::Array(Box::new(TypeId::Builtin(BuiltinType::U8))))
            );
            assert!(facts.typed.type_table.scalar_value(id).is_none());
            let span = facts.lowered.module.expr(id).origin.diagnostic_span();
            let values = elements
                .iter()
                .map(|item| {
                    assert_eq!(
                        facts.lowered.module.expr(*item).origin.diagnostic_span(),
                        span
                    );
                    let Some(ScalarValue::Integer { value, ty }) =
                        facts.typed.type_table.scalar_value(*item)
                    else {
                        panic!("byte item has a checked integer fact");
                    };
                    assert_eq!(*ty, IntegerType::U8);
                    *value
                })
                .collect::<Vec<_>>();
            arrays.push(values);
        }
        if let ExprKind::Match { arms, .. } = &expr.kind {
            let PatternKind::Or(alternatives) = &facts.lowered.module.pattern(arms[0].pattern).kind
            else {
                panic!("byte alternatives remain scalar literal patterns");
            };
            assert_eq!(alternatives.len(), 2);
            for (pattern, expected) in alternatives.iter().zip([byte(65), byte(90)]) {
                assert_eq!(
                    facts.typed.type_table.pattern_scalar_value(*pattern),
                    Some(&expected)
                );
            }
            assert_eq!(
                facts.typed.type_table.pattern_scalar_value(arms[1].pattern),
                Some(&byte(255))
            );
        }
    }
    assert_eq!(
        arrays,
        [
            vec![0, 255],
            vec![],
            b"\\xFF{name}"
                .iter()
                .map(|value| i128::from(*value))
                .collect(),
            vec![]
        ]
    );
}

#[test]
fn byte_literals_reject_contextual_retyping_and_array_scalar_constants() {
    for text in [
        "fn bad() -> i32 { b'A' }",
        "fn bad() -> char { b'A' }",
        "fn bad() -> u8 { -b'A' }",
        "fn bad() -> [i32] { b\"A\" }",
        "fn bad() -> [i32] { b\"\" }",
        "fn bad() -> [i32] { br\"\" }",
        "fn bad(value: i32) { match value { b'A' => (), _ => () } }",
        "fn bad(value: i32) { match value { b'A'..=b'Z' => (), _ => () } }",
        "fn bad(value: u8) { match value { b'A'..=90 => (), _ => () } }",
        "fn bad(value: u8) { match value { b'A'..='Z' => (), _ => () } }",
        "const BAD: [u8] = b\"A\";",
        "const BAD: [u8] = b\"\";",
        "trait Bad { const VALUE: [u8] = br\"\"; }",
        "fn bad() -> u8 { b'é' }",
        "fn bad() -> [u8] { b\"中文\" }",
        "fn bad() -> [u8] { br\"é\" }",
        r"fn bad() -> u8 { b'\u{41}' }",
        r#"fn bad() -> [u8] { b"\u{41}" }"#,
    ] {
        let mut sources = SourceDatabase::default();
        let file = sources
            .set("bytes.kgr", text.into(), SourceLayer::Base)
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
            "{text}"
        );
    }
}

#[test]
fn byte_range_patterns_remain_outside_the_supported_scalar_domains() {
    for bounds in ["b'A'..=b'Z'", "b'A'..b'Z'", "65u8..=90u8", "FIRST..=LAST"] {
        let text = format!(
            "const FIRST: u8 = b'A'; const LAST: u8 = b'Z'; fn bad(value: u8) {{ match value {{ {bounds} => (), _ => () }} }}"
        );
        let mut sources = SourceDatabase::default();
        let file = sources
            .set("byte-ranges.kgr", text, SourceLayer::Base)
            .unwrap();
        let mut db = test_analysis();
        let snapshot = db
            .snapshot(sources.snapshot(), &Default::default())
            .unwrap();
        let result = snapshot.file(file).unwrap().result();
        assert!(
            result.diagnostics().iter().any(|diagnostic| matches!(
                &diagnostic.kind,
                DiagnosticKind::PatternTypeMismatch { found, .. }
                    if found == "i32 or char range pattern"
            )),
            "{bounds}: {:?}",
            result.diagnostics()
        );
    }
}
