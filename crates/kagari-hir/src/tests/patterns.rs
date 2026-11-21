use crate::tests::standard::standard_modules;
use crate::{analysis::FileAnalysis, tests::test_analysis};
use kagari_common::identity::{
    ModuleIdentity, PackageId, map::DefinitionContext, metadata::DefinitionMetadata,
};
use kagari_source::{
    diagnostic::{Diagnostic, DiagnosticKind},
    source::SourceFile,
    source_database::{SourceDatabase, SourceLayer},
};
use kagari_types::declaration::{
    TypeDef, TypeDefKind,
    host::{HostReflectionPolicy, HostTypeOwnership, HostTypePolicy, PathAccess},
    module::ModuleDecl,
};
use std::sync::Arc;

const POINT: &str = "struct Point { val x: i32, val y: i32 }";
const NESTED: &str = "
struct Inner<T> { val value: T, val spare: bool }
struct Outer<T> { val value: Inner<T>, val spare: i32 }
";

fn analyze_source(source: &str) -> Arc<FileAnalysis> {
    let mut sources = SourceDatabase::default();
    let root = sources
        .set("patterns.kgr", source.into(), SourceLayer::Base)
        .unwrap();
    let snapshot = test_analysis()
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    snapshot.file(root).unwrap().clone()
}

fn check_source(source: &str) -> Vec<Diagnostic> {
    analyze_source(source).result().diagnostics().to_vec()
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

/* Temporarily paused: stdlib restoration (NC01-NC06).
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
*/

/* Temporarily paused: stdlib restoration (NC01-NC06).
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
*/

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

#[test]
fn declaration_pattern_accepts_nested_generic_and_ignored_values() {
    let source = format!(
        "{NESTED}
        fn read<T>(input: (Outer<T>, bool)) -> T {{
            val (Outer {{ value: Inner {{ value: answer, .. }}, .. }}, _) = input;
            answer
        }}
        struct Empty {{}}
        fn discard() {{
            val () = ();
            val Empty {{}} = Empty {{}};
            val _ = (1, true);
            val (only,) = (1,);
            var ((left | left), right) = (only, 2);
            left += right;
        }}"
    );
    assert_pattern_diagnostics(&source, &[]);
    assert_pattern_diagnostics(
        &format!("{POINT} fn read(input: Point) {{ val Point {{ x }} = input; }}"),
        &["missing fields: `y`"],
    );
    assert_pattern_diagnostics(
        "fn wrong() { val (left, right) = (1,); }",
        &["tuple of 2 elements"],
    );
}

#[test]
fn declaration_pattern_duplicate_names_are_checked_across_nested_fields() {
    assert_pattern_diagnostics(
        &format!(
            "{POINT} fn bad(input: (Point, i32)) {{ val (Point {{ x: same, y: _ }}, same) = input; }}"
        ),
        &["duplicate binding `same`"],
    );
    let diagnostics = check_source("fn bad() { val ((same | same), same) = (1, 2); }");
    assert!(
        diagnostics.iter().any(|diagnostic| matches!(
            &diagnostic.kind, DiagnosticKind::PatternTypeMismatch { found, .. }
                if found == "duplicate binding `same`"
        )),
        "{diagnostics:?}"
    );
    let diagnostics = check_source("fn bad() { val ((left | right), other) = (1, 2); }");
    assert!(
        diagnostics.iter().any(|diagnostic| matches!(
            &diagnostic.kind, DiagnosticKind::PatternTypeMismatch { expected, .. }
                if expected == "the same bindings in every alternative"
        )),
        "{diagnostics:?}"
    );
}

#[test]
fn declaration_pattern_refutable_forms_remain_rejected() {
    for source in [
        "fn bad() { val 1 = 1; }",
        "fn bad() { val 0..=2 = 1; }",
        "fn bad(input: Option<i32>) { val Some(value) = input; }",
        "fn bad(input: Option<i32>) { val Option::None = input; }",
        "enum Only { One(i32) } fn bad(input: Only) { val Only::One(value) = input; }",
        "fn bad(input: (Option<i32>, i32)) { val (Some(value), other) = input; }",
    ] {
        let diagnostics = check_source(source);
        assert!(
            diagnostics.iter().any(|diagnostic| matches!(
                &diagnostic.kind, DiagnosticKind::PatternTypeMismatch { expected, found }
                    if expected == "irrefutable declaration pattern" && found == "refutable pattern"
            )),
            "{source}: {diagnostics:?}"
        );
    }
}

#[test]
fn declaration_pattern_host_values_bind_without_decomposing() {
    let context = DefinitionContext::new().unwrap();
    let mut module = ModuleDecl::new(ModuleIdentity {
        package: PackageId("host".into()),
        path: vec![],
    });
    module.types.push(TypeDef {
        name: "Item".into(),
        kind: TypeDefKind::Host(HostTypePolicy {
            ownership: HostTypeOwnership::Opaque,
            reflection: HostReflectionPolicy::Hidden,
            path_access: PathAccess::None,
        }),
        generic_params: vec![],
        bounds: vec![],
        fields: vec![],
        variants: vec![],
    });
    module.collect_definitions(&context).unwrap();
    let mut modules = standard_modules();
    modules.push(Arc::new(
        DefinitionMetadata::checked(context.snapshot(), module, &Default::default()).unwrap(),
    ));
    for (body, valid) in [
        ("val saved = input; val _ = saved;", true),
        ("val host::Item { .. } = input;", false),
    ] {
        let source = format!("fn read(input: host::Item) {{ {body} }}");
        let analysis = crate::analyze_source(
            &SourceFile::new("host-pattern.kgr", source.clone()),
            modules.clone(),
        )
        .unwrap();
        assert_eq!(
            analysis.diagnostics().is_empty(),
            valid,
            "{source}: {:?}",
            analysis.diagnostics()
        );
        assert_eq!(analysis.into_codegen().is_ok(), valid);
    }
}

#[test]
fn declaration_pattern_contextual_coercion_and_annotation_holes_apply_to_the_whole_value() {
    let source = "
trait Read { fn read(self) -> i32; }
struct Number { val value: i32 }
impl Read for Number { fn read(self) -> i32 { self.value } }
fn check_wide(value: i64) {}
fn read() -> i32 {
    val (reader, wide): (Read, i64) = (Number { value: 7 }, 8);
    val (integer, flag): _ = (1, true);
    val (inferred, _): (_, bool) = (integer, flag);
    check_wide(wide);
    reader.read() + inferred
}";
    assert_pattern_diagnostics(source, &[]);
    let diagnostics = check_source("fn bad() { val (integer, flag): (i32, bool) = (1, 2); }");
    assert!(!diagnostics.is_empty());
}

#[test]
fn declaration_pattern_noncompletion_retains_static_validation_and_required_initializers() {
    for source in [
        "fn read() -> i32 { val (left, right): (i32, bool) = loop {}; 0 }",
        "fn read() -> i32 { val (left, right) = loop {}; 0 }",
        "fn read() -> i32 { val saved = loop {}; 0 }",
        "fn read() -> i32 { val _ = if true { return 1; } else { return 2; }; 0 }",
    ] {
        assert_pattern_diagnostics(source, &[]);
    }
    assert_pattern_diagnostics(
        &format!("{POINT} fn read() {{ val Point {{ unknown: _, .. }} = loop {{}}; }}"),
        &["unknown field `unknown`"],
    );
    for source in [
        "fn bad() { val (left, right); }",
        "fn bad() { val (left, right) = ; }",
    ] {
        let file = analyze_source(source);
        assert!(!file.result().diagnostics().is_empty(), "{source}");
        assert!(file.result().clone().into_codegen().is_err(), "{source}");
    }
}
