use crate::{analysis::FileAnalysis, hir::stmt::StmtKind, tests::test_analysis, types::TypeId};
use kagari_common::identity::{
    ModuleIdentity, PackageId, map::DefinitionContext, metadata::DefinitionMetadata,
};
use kagari_source::{
    diagnostic::{Diagnostic, DiagnosticKind},
    source::SourceFile,
    source_database::{SourceDatabase, SourceLayer},
};
use kagari_stdlib::catalog;
use kagari_types::{
    declaration::{
        TypeDef, TypeDefKind,
        host::{HostReflectionPolicy, HostTypeOwnership, HostTypePolicy, PathAccess},
        module::ModuleDecl,
    },
    scalar::BuiltinType,
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
fn declaration_pattern_shadowing_publishes_all_leaves_after_the_initializer() {
    let source = "
const LIMIT: i32 = 7;
struct Names { val None: i32, val LIMIT: i32 }
fn simple() -> i32 { val None = 1; None }
fn read(x: i32) -> i32 {
    val ((x, None), LIMIT) = ((x + 1, 2), LIMIT);
    val Names { None, LIMIT } = Names { None: None, LIMIT: LIMIT };
    x + None + LIMIT
}";
    let file = analyze_source(source);
    assert!(
        file.result().diagnostics().is_empty(),
        "{:?}",
        file.result().diagnostics()
    );
    let parameter = source.find("x: i32").unwrap();
    let rhs = source.find("x + 1").unwrap();
    let declaration = source.find("((x, None)").unwrap() + 2;
    let tail = source.rfind("x + None").unwrap();
    assert_eq!(file.definition_at(rhs), file.definition_at(parameter));
    assert_eq!(file.definition_at(tail), file.definition_at(declaration));
    assert_ne!(file.definition_at(rhs), file.definition_at(tail));
    for name in ["None", "LIMIT"] {
        let site = source.find("val Names { None, LIMIT }").unwrap()
            + "val Names { ".len()
            + if name == "LIMIT" { "None, ".len() } else { 0 };
        let tail = source.rfind(name).unwrap();
        assert_eq!(file.definition_at(tail), file.definition_at(site));
        assert!(
            file.visible_bindings(tail)
                .iter()
                .any(|binding| binding.declaration.name == name)
        );
    }
    let unknown = check_source("fn bad() { val (first, second) = (1, first); }");
    assert!(
        unknown.iter().any(|diagnostic| matches!(
            &diagnostic.kind, DiagnosticKind::UnknownName { name } if name == "first"
        )),
        "{unknown:?}"
    );
    let wildcard = check_source("fn bad() { val _ = 1; _ }");
    assert!(
        !wildcard.is_empty(),
        "wildcard is not a referenceable local"
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
fn declaration_pattern_read_only_and_mutable_leaves_keep_usage_and_captures() {
    let source = "
fn read() -> i32 {
    var (left, right) = (1, 2);
    val bump = || { left += right; left };
    right = 3;
    bump()
}";
    let file = analyze_source(source);
    assert!(
        file.result().diagnostics().is_empty(),
        "{:?}",
        file.result().diagnostics()
    );
    let usage = file.binding_usage(&Default::default()).unwrap();
    for name in ["left", "right"] {
        let binding = usage
            .iter()
            .find(|binding| binding.declaration.name == name)
            .unwrap();
        assert!(
            binding.mutable && binding.initialized && binding.captured,
            "{binding:?}"
        );
        assert_eq!(binding.writes, 1, "{binding:?}");
        assert!(binding.reads > 0, "{binding:?}");
    }
    let diagnostics =
        check_source("fn bad() { val (left, right) = (1, 2); left = 3; right += 1; }");
    assert_eq!(
        diagnostics
            .iter()
            .filter(|diagnostic| matches!(
                diagnostic.kind,
                DiagnosticKind::InvalidAssignmentTarget { .. }
            ))
            .count(),
        2,
        "{diagnostics:?}"
    );
    let source = "fn read() -> i32 { val (left, _) = (1, 2); left }";
    let file = analyze_source(source);
    assert!(file.result().diagnostics().is_empty());
    let usage = file.binding_usage(&Default::default()).unwrap();
    assert_eq!(usage.len(), 1);
    assert!(!usage[0].mutable);
    assert_eq!(usage[0].declaration.name, "left");
    assert!(
        file.visible_bindings(source.rfind("left").unwrap())
            .iter()
            .all(|binding| binding.declaration.name != "_")
    );
}

#[test]
fn declaration_pattern_scoped_values_bind_without_decomposing() {
    for receiver in ["Ref<Point>", "RefMut<Point>"] {
        let source = format!(
            "{POINT} fn read(input: {receiver}) -> i32 {{ val saved: {receiver} = input; val _ = saved; saved.x }}"
        );
        assert_pattern_diagnostics(&source, &[]);
        let source =
            format!("{POINT} fn bad(input: {receiver}) {{ val Point {{ x, .. }} = input; }}");
        let diagnostics = check_source(&source);
        assert!(
            diagnostics.iter().any(|diagnostic| matches!(
                &diagnostic.kind, DiagnosticKind::PatternTypeMismatch { found, .. }
                    if found == "struct pattern `Point`"
            )),
            "{source}: {diagnostics:?}"
        );
    }
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
    let mut modules = catalog::shared().unwrap();
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

#[test]
fn declaration_pattern_canonical_leaf_order_uses_the_first_alternative() {
    let source = format!(
        "{POINT} fn read(input: (Point, i32)) -> i32 {{
            val (Point {{ y: second, x: first }}, (last | last)) = input;
            first + second + last
        }}"
    );
    let file = analyze_source(&source);
    assert!(
        file.result().diagnostics().is_empty(),
        "{:?}",
        file.result().diagnostics()
    );
    let module = &file.result().facts().lowered.module;
    let block = module.block(module.functions[0].body.unwrap());
    let StmtKind::Binding { pattern, .. } = module.stmt(block.statements[0]).kind else {
        panic!("declaration");
    };
    let bindings = module.body.pattern_bindings(pattern);
    assert_eq!(
        bindings.iter().map(|(_, name)| *name).collect::<Vec<_>>(),
        ["second", "first", "last"]
    );
    for (local, name) in bindings {
        let span = file.result().facts().lowered.source_map.local_span(local);
        assert_eq!(&source[span.start..span.end], name);
        assert_eq!(
            file.type_at(source.rfind(name).unwrap()),
            Some(TypeId::Builtin(BuiltinType::I32))
        );
    }
}
