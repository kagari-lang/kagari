use crate::tests::test_analysis;
use kagari_source::{
    diagnostic::{Diagnostic, DiagnosticKind},
    source_database::{SourceDatabase, SourceLayer},
};

const POINT: &str = "struct Point { val x: i32, val y: i32 }";
const NESTED: &str = "
struct Inner<T> { val value: T, val spare: bool }
struct Outer<T> { val value: Inner<T>, val spare: i32 }
";

fn check_source(source: &str) -> Vec<Diagnostic> {
    let mut sources = SourceDatabase::default();
    let root = sources
        .set("patterns.kgr", source.into(), SourceLayer::Base)
        .unwrap();
    let snapshot = test_analysis()
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    snapshot.file(root).unwrap().result().diagnostics().to_vec()
}

fn assert_pattern_diagnostics(source: &str, expected: &[&str]) {
    let diagnostics = check_source(source);
    let found = diagnostics
        .iter()
        .map(|diagnostic| match &diagnostic.kind {
            DiagnosticKind::PatternTypeMismatch { found, .. } => found.as_str(),
            other => panic!("unexpected diagnostic for {source}: {other:?}"),
        })
        .collect::<Vec<_>>();
    assert_eq!(found, expected, "{source}: {diagnostics:?}");
}

#[test]
fn struct_pattern_rest_allows_partial_empty_and_redundant_rest() {
    for body in [
        "match input { Point { x, .. } => x }",
        "match input { Point { .. } => 0 }",
        "match input { Point { x, y: _, .. } => x }",
        "for Point { x, .. } in [input] { return x; } 0",
        "if val Point { x, .. } = input { x } else { 0 }",
        "while val Point { x, .. } = input { return x; } 0",
    ] {
        let source = format!("{POINT} fn read(input: Point) -> i32 {{ {body} }}");
        assert_pattern_diagnostics(&source, &[]);
    }
}

#[test]
fn struct_pattern_full_requires_every_declared_field() {
    for body in [
        "match input { Point { x, y } => x + y }",
        "match input { Point { y: _, x: renamed } => renamed }",
    ] {
        let source = format!("{POINT} fn read(input: Point) -> i32 {{ {body} }}");
        assert_pattern_diagnostics(&source, &[]);
    }
    assert_pattern_diagnostics(
        "struct Empty {} fn read(input: Empty) { match input { Empty {} => () } }",
        &[],
    );
    for body in [
        "match input { Point { x } => x }",
        "for Point { x } in [input] { return x; } 0",
        "if val Point { x } = input { x } else { 0 }",
        "while val Point { x } = input { return x; } 0",
    ] {
        let source = format!("{POINT} fn read(input: Point) -> i32 {{ {body} }}");
        assert_pattern_diagnostics(&source, &["missing fields: `y`"]);
    }
    assert_pattern_diagnostics(
        &format!("{POINT} fn read(input: Point) {{ match input {{ Point {{}} => () }} }}"),
        &["missing fields: `x`, `y`"],
    );
}

#[test]
fn struct_pattern_nested_generic_checks_completeness_and_substitution() {
    for (pattern, expected) in [
        (
            "Outer { value: Inner { value: answer, spare: _ }, spare: _ }",
            &[][..],
        ),
        ("Outer { value: Inner { value: answer, .. }, .. }", &[][..]),
        (
            "Outer { value: Inner { value: answer }, .. }",
            &["missing fields: `spare`"][..],
        ),
        (
            "Outer { value: Inner { value: answer, .. } }",
            &["missing fields: `spare`"][..],
        ),
        (
            "Outer { value: Inner { value: answer } }",
            &["missing fields: `spare`", "missing fields: `spare`"][..],
        ),
    ] {
        let source = format!(
            "{NESTED} fn read<T>(input: Outer<T>) -> T {{ match input {{ {pattern} => answer }} }}"
        );
        assert_pattern_diagnostics(&source, expected);
    }
    let source = format!(
        "{NESTED} fn read(input: Outer<i32>) -> i32 {{
            match input {{ Outer {{ value: Inner {{ value: true, .. }}, .. }} => 1, _ => 0 }}
        }}"
    );
    assert_pattern_diagnostics(&source, &["bool"]);
}

#[test]
fn struct_pattern_private_fields_require_rest_and_explicit_access() {
    let declarations = "
mod model {
    pub struct Packet { pub val visible: i32, val hidden: i32 }
}
use self::model::Packet;
";
    for (pattern, expected) in [
        ("Packet { visible, .. }", &[][..]),
        ("Packet { .. }", &[][..]),
        ("Packet { visible }", &["missing fields: `hidden`"][..]),
        (
            "Packet { visible: _, hidden: _, .. }",
            &["unknown field `hidden`"][..],
        ),
        (
            "Packet { visible: _, hidden: _ }",
            &["unknown field `hidden`", "missing fields: `hidden`"][..],
        ),
    ] {
        let source = format!(
            "{declarations} fn read(input: Packet) -> i32 {{ match input {{ {pattern} => 0 }} }}"
        );
        assert_pattern_diagnostics(&source, expected);
    }
}

#[test]
fn struct_pattern_duplicate_fields_do_not_satisfy_completeness() {
    for (pattern, expected) in [
        (
            "Point { x: _, x: _ }",
            &["duplicate field `x`", "missing fields: `y`"][..],
        ),
        ("Point { x: _, x: _, .. }", &["duplicate field `x`"][..]),
    ] {
        let source =
            format!("{POINT} fn read(input: Point) {{ match input {{ {pattern} => () }} }}");
        assert_pattern_diagnostics(&source, expected);
    }
}

#[test]
fn struct_pattern_unknown_fields_do_not_satisfy_completeness() {
    for (pattern, expected) in [
        (
            "Point { x: _, unknown: _ }",
            &["unknown field `unknown`", "missing fields: `y`"][..],
        ),
        (
            "Point { x: _, unknown: _, .. }",
            &["unknown field `unknown`"][..],
        ),
    ] {
        let source =
            format!("{POINT} fn read(input: Point) {{ match input {{ {pattern} => () }} }}");
        assert_pattern_diagnostics(&source, expected);
    }
}
