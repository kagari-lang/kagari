//! Struct update typing and the checked constructor/base handoff.
use crate::{analyze_source, hir::expr::ExprKind, typeck::table::CallTarget};
use kagari_source::{diagnostic::DiagnosticKind, source::SourceFile};
use kagari_stdlib::catalog as foundation_catalog;

const FACTORIES: &str = r#"
trait Factory { fn make() -> Self; }
struct Player {}
impl Factory for Player { fn make() -> Self { Player {} } }
struct Pair<T> { val value: T, val flag: bool }
impl<T: Factory> Factory for Pair<T> {
    fn make() -> Self { Pair { value: T::make(), flag: false } }
}
fn take(value: Pair<Player>) {}
"#;

#[test]
fn struct_update_fields_keep_explicit_order_and_omitted_declaration_order() {
    for (fields, explicit, omitted) in [
        ("..base", vec![], vec!["first", "second", "third"]),
        (
            "third: 3, first: 1, ..base",
            vec!["third", "first"],
            vec!["second"],
        ),
        (
            "third: 3, first: 1, second: 2, ..base",
            vec!["third", "first", "second"],
            vec![],
        ),
    ] {
        let source = SourceFile::new(
            "update-facts.kgr",
            format!(
                "struct Item {{ val first: i32, val second: i32, val third: i32 }} fn main(base: Item) -> Item {{ Item {{ {fields} }} }}"
            ),
        );
        let analysis = analyze_source(&source, foundation_catalog::shared().unwrap()).unwrap();
        assert!(
            analysis.diagnostics().is_empty(),
            "{fields}: {:?}",
            analysis.diagnostics()
        );
        let checked = analysis.into_codegen().expect("checked update");
        let (id, _) = checked
            .lowered
            .module
            .body
            .expressions()
            .find(|(_, expr)| matches!(expr.kind, ExprKind::StructInit { base: Some(_), .. }))
            .unwrap();
        let resolved = checked.typed.type_table.struct_init(id).unwrap();
        let names = |ids: Vec<_>| {
            ids.into_iter()
                .map(|id| checked.aggregates.field(&id).unwrap().name.as_str())
                .collect::<Vec<_>>()
        };
        assert_eq!(
            names(resolved.fields.iter().map(|field| field.unwrap()).collect()),
            explicit
        );
        assert_eq!(names(resolved.base_fields.clone()), omitted);
    }
    let source = SourceFile::new(
        "empty-update.kgr",
        "struct Empty {} fn main(base: Empty) -> Empty { Empty { ..base } }",
    );
    analyze_source(&source, foundation_catalog::shared().unwrap())
        .unwrap()
        .into_codegen()
        .expect("empty struct base");
}

#[test]
fn struct_update_context_links_fields_bases_and_factory_receivers() {
    for body in [
        "val item = Pair { ..base };",
        "val item: Pair<Player> = Pair { ..Factory::make() };",
        "val item = Pair<Player> { ..Factory::make() };",
        "val item = Pair<_> { ..base };",
        "val item = Pair { value: Player {}, ..Factory::make() };",
        "val item = Pair { value: Factory::make(), ..base };",
        "val item: Pair<Player> = Pair { flag: true, ..Factory::make() };",
        "val item = Pair { ..Factory::make() }; take(item);",
    ] {
        let source = SourceFile::new(
            "update-context.kgr",
            format!("{FACTORIES} fn main(base: Pair<Player>) {{ {body} }}"),
        );
        let analysis = analyze_source(&source, foundation_catalog::shared().unwrap()).unwrap();
        assert!(
            analysis.diagnostics().is_empty(),
            "{body}: {:?}",
            analysis.diagnostics()
        );
        let checked = analysis.into_codegen().expect("bounded contextual update");
        for (id, _) in checked.lowered.module.body.expressions() {
            if let Some(call) = checked.typed.type_table.call_resolution(id)
                && matches!(call.target, CallTarget::TraitMethod { .. })
                && call.receiver.is_none()
            {
                assert!(checked.typed.type_table.protocol_receiver(id).is_some());
            }
        }
    }
    let source = SourceFile::new(
        "update-generic.kgr",
        format!(
            r#"
{FACTORIES}
fn generic<T: Factory>(base: Pair<T>) -> Pair<T> {{ Pair {{ value: Factory::make(), ..base }} }}
fn numeric(base: Pair<u8>) -> Pair<u8> {{ Pair {{ value: 1, ..base }} }}
trait Pick {{ type Item; }}
struct Tag {{}}
impl Pick for Tag {{ type Item = i32; }}
struct Cell<T> {{ val value: T }}
impl Factory for Cell<i32> {{ fn make() -> Self {{ Cell {{ value: 42 }} }} }}
fn projected(base: Cell<i32>) -> Cell<i32> {{ Cell<<Tag as Pick>::Item> {{ ..base }} }}
fn projected_factory() -> Cell<i32> {{ Cell<<Tag as Pick>::Item> {{ ..Factory::make() }} }}
"#
        ),
    );
    let analysis = analyze_source(&source, foundation_catalog::shared().unwrap()).unwrap();
    assert!(
        analysis.diagnostics().is_empty(),
        "{:?}",
        analysis.diagnostics()
    );
    analysis
        .into_codegen()
        .expect("caller bounds and base before numeric defaults");
}

#[test]
fn struct_update_bases_require_exact_owned_nominal_applications() {
    for (body, reason) in [
        ("Pair<Player> { ..other };", "update base must have type"),
        (
            "Pair<Player> { value: Player {}, flag: true, ..wrong };",
            "update base must have type",
        ),
        ("Pair<Player> { ..42 };", "update base must have type"),
        (
            "Pair<Player> { ..Some(Player {}) };",
            "update base must have type",
        ),
        ("Pair<Player> { ..view };", "update base must have type"),
        (
            "Pair<Player> { value: Player {}, value: Player {}, ..pair };",
            "duplicate field",
        ),
        ("Pair<Player> { absent: true, ..pair };", "unknown field"),
        ("Pair<Player> { value: Player {} };", "missing field"),
        ("Pair<Player> { value: false, ..pair };", "assignment"),
    ] {
        let source = SourceFile::new(
            "update-mismatch.kgr",
            format!(
                r#"
{FACTORIES}
struct Other {{ val value: Player, val flag: bool }}
trait View {{ fn read(self) -> bool; }}
fn main(pair: Pair<Player>, wrong: Pair<bool>, other: Other, view: View) {{ {body} }}
"#
            ),
        );
        let analysis = analyze_source(&source, foundation_catalog::shared().unwrap()).unwrap();
        assert!(
            analysis
                .diagnostics()
                .iter()
                .any(|diagnostic| match &diagnostic.kind {
                    DiagnosticKind::InvalidStructInitializer { reason: actual, .. } =>
                        actual.starts_with(reason),
                    DiagnosticKind::AssignmentTypeMismatch { .. } => reason == "assignment",
                    _ => false,
                }),
            "{body}: {:?}",
            analysis.diagnostics()
        );
        assert!(analysis.into_codegen().is_err(), "{body}");
    }
}

#[test]
fn struct_update_context_does_not_choose_implementation_arguments_or_grant_bounds() {
    for declaration in [
        "fn main() { val item = Pair { ..Factory::make() }; }",
        "fn main() { val item = Pair<bool> { ..Factory::make() }; }",
        "fn main<T>(base: Pair<T>) { Pair { value: Factory::make(), ..base }; }",
    ] {
        let source = SourceFile::new("update-bound.kgr", format!("{FACTORIES} {declaration}"));
        let analysis = analyze_source(&source, foundation_catalog::shared().unwrap()).unwrap();
        assert!(
            analysis.diagnostics().iter().any(|diagnostic| matches!(
                diagnostic.kind,
                DiagnosticKind::CannotInferGenericArgument { .. }
                    | DiagnosticKind::GenericBoundNotSatisfied { .. }
            )),
            "{declaration}: {:?}",
            analysis.diagnostics()
        );
        assert!(analysis.into_codegen().is_err());
    }
}

#[test]
fn struct_update_completion_uses_only_normally_produced_operands() {
    for (body, valid) in [
        ("Pair { flag: true, ..{ return 42; } };", true),
        ("Pair { value: { return 42; }, ..base };", true),
        ("Pair { value: { return false; }, ..base };", false),
        ("Pair { value: { return 42; }, ..unknown };", false),
        ("Pair { value: { return 42; }, ..false };", false),
        ("Pair { value: { return 42; }, ..Player {} };", false),
        ("Pair<Player> { value: { return 42; }, ..false };", false),
        ("Pair { flag: 1, ..{ return 42; } };", false),
    ] {
        let source = SourceFile::new(
            "update-completion.kgr",
            format!("{FACTORIES} fn main(base: Pair<Player>) -> i32 {{ {body} 0 }}"),
        );
        let analysis = analyze_source(&source, foundation_catalog::shared().unwrap()).unwrap();
        assert_eq!(
            analysis.diagnostics().is_empty(),
            valid,
            "{body}: {:?}",
            analysis.diagnostics()
        );
        if valid {
            let facts = analysis.facts();
            for (id, expr) in facts.lowered.module.body.expressions() {
                if matches!(expr.kind, ExprKind::StructInit { base: Some(_), .. }) {
                    assert!(facts.typed.type_table.expr_type(id).unwrap().is_never());
                }
            }
        }
        assert_eq!(analysis.into_codegen().is_ok(), valid, "{body}");
    }
}

#[test]
fn struct_update_missing_base_preserves_the_written_marker_and_other_errors() {
    let source = SourceFile::new(
        "missing-update.kgr",
        "struct Item { val value: i32 } fn main() { Item { value: false, .. }; Missing { value: unknown, ..absent }; }",
    );
    let analysis = analyze_source(&source, foundation_catalog::shared().unwrap()).unwrap();
    let facts = analysis.facts();
    let base = facts
        .lowered
        .module
        .body
        .expressions()
        .find_map(|(_, expr)| match expr.kind {
            ExprKind::StructInit {
                base: Some(base), ..
            } if matches!(facts.lowered.module.expr(base).kind, ExprKind::Missing) => Some(base),
            _ => None,
        })
        .expect("written update without its operand");
    assert!(matches!(
        facts.lowered.module.expr(base).kind,
        ExprKind::Missing
    ));
    for name in ["unknown", "absent"] {
        assert!(
            analysis.diagnostics().iter().any(|diagnostic| matches!(
                &diagnostic.kind, DiagnosticKind::UnknownName { name: actual } if actual == name
            )),
            "{:?}",
            analysis.diagnostics()
        );
    }
    assert!(analysis.diagnostics().iter().any(|diagnostic| matches!(
        diagnostic.kind,
        DiagnosticKind::AssignmentTypeMismatch { .. }
    )));
    assert!(analysis.into_codegen().is_err());
}

#[test]
fn struct_update_bases_remain_nonconstant_in_recovery() {
    // A scalar declared type reaches the expression validator instead of stopping
    // at the existing heap-backed-const gate for the constructed struct itself.
    let source = SourceFile::new(
        "const-update.kgr",
        "struct Item { val value: i32 } fn make() -> Item { Item { value: 1 } } const BAD: i32 = Item { value: 2, ..make() };",
    );
    let analysis = analyze_source(&source, foundation_catalog::shared().unwrap()).unwrap();
    assert!(analysis.diagnostics().iter().any(|diagnostic| matches!(
        &diagnostic.kind, DiagnosticKind::InvalidConstInitializer { reason, .. } if reason == "unsupported const initializer expression"
    ) && diagnostic.span.is_some_and(|span| &source.text()[span.start..span.end] == "make()")), "{:?}", analysis.diagnostics());
    assert!(analysis.into_codegen().is_err());
}
