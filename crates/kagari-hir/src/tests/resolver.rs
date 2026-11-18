use kagari_common::identity::map::DefinitionContext;
use kagari_source::{diagnostic::DiagnosticKind, source::SourceFile};
use kagari_stdlib::catalog as foundation_catalog;
use kagari_types::declaration::names::NameNamespace;

use crate::{
    analyze_source,
    hir::{
        expr::ExprKind, ids::BodyOwner, pattern::PatternKind, stmt::StmtKind,
        writeability::Writeability,
    },
    resolver::{collect::resolve_names, resolved::ResolvedName, table::NameResolution},
    tests::common,
    typeck::table::CallTarget,
};

#[test]
fn standard_item_spellings_require_installed_declarations() {
    let context = DefinitionContext::new().unwrap();
    for name in [
        "core",
        "std::collections",
        "Eq",
        "Some",
        "None",
        "Option::Some",
        "Vec::new",
        "demo::bindings::choose",
    ] {
        let lowered = common::lower_ok(&format!("fn main() {{ {name}; }}"));
        let resolved = resolve_names(&lowered).unwrap();
        let function = &lowered.module.functions[0];
        let statement = lowered
            .module
            .block(function.implementation.script_body().unwrap())
            .statements[0];
        let StmtKind::Expr(expression) = lowered.module.stmt(statement).kind else {
            panic!("name expression statement");
        };
        assert_eq!(resolved.facts().expr_resolution(expression), None, "{name}");
        let checked = common::check_module(&context, &lowered, resolved.facts(), None);
        assert!(checked.diagnostics().iter().any(|diagnostic| {
            matches!(&diagnostic.kind, DiagnosticKind::UnknownName { name: missing } if missing == name)
        }), "{name}: {:?}", checked.diagnostics());
    }
}

#[test]
fn resolves_params_and_locals_in_function_body() {
    let text = include_str!("../../tests/fixtures/analysis/canonical_bindings.kgr");
    let lowered = common::lower_ok(text);
    let resolved = resolve_names(&lowered)
        .unwrap()
        .into_checked()
        .expect("resolver should succeed");
    let function = &lowered.module.functions[0];

    let block = lowered
        .module
        .block(function.implementation.script_body().unwrap());
    let let_stmt = lowered.module.stmt(block.statements[0]);
    let (let_local, init_expr) = match &let_stmt.kind {
        StmtKind::Binding {
            pattern,
            initializer,
            ..
        } => (
            lowered.module.nodes.pattern_bindings(*pattern)[0].0,
            *initializer,
        ),
        other => panic!("unexpected stmt kind: {other:?}"),
    };

    let tail_expr = block.tail_expr.expect("tail expr");

    assert_eq!(
        resolved.expr_resolution(init_expr),
        Some(ResolvedName::Param(function.params[0].id))
    );
    assert_eq!(
        resolved.expr_resolution(tail_expr),
        Some(ResolvedName::Local(let_local))
    );
    let mut names = resolved
        .bindings()
        .map(|binding| binding.name.as_str())
        .collect::<Vec<_>>();
    names.sort_unstable();
    assert_eq!(
        names,
        [
            "callback", "helper", "next", "unused", "unused", "unused", "unused", "value"
        ]
    );
    for binding in resolved.bindings() {
        assert!(std::ptr::eq(
            resolved.binding(&binding.resolved).unwrap(),
            binding
        ));
        let mutable = matches!(binding.resolved, ResolvedName::Local(local)
            if lowered.module.nodes.local(local).origin.diagnostic_span().start == text.find("(unused, _)").unwrap() + 1);
        assert_eq!(
            binding.writeability,
            if mutable {
                Writeability::Var
            } else {
                Writeability::Val
            }
        );
        let expected = if matches!(binding.resolved, ResolvedName::Param(id) if id == lowered.module.functions[1].params[0].id)
        {
            lowered.module.functions[1].id
        } else {
            function.id
        };
        assert_eq!(binding.owner, BodyOwner::Function(expected));
    }
    let closure = lowered
        .module
        .nodes
        .expressions()
        .find_map(|(id, expr)| matches!(expr.kind, ExprKind::Closure { .. }).then_some(id))
        .unwrap();
    assert_eq!(
        resolved.closure_captures(closure),
        [ResolvedName::Local(let_local)]
    );
    let foreign = common::lower_ok(text);
    let foreign_names = resolve_names(&foreign).unwrap();
    for binding in foreign_names.facts().bindings() {
        if matches!(
            binding.resolved,
            ResolvedName::Local(_) | ResolvedName::Param(_)
        ) {
            assert!(resolved.binding(&binding.resolved).is_none());
        }
    }
}

#[test]
fn resolves_named_match_pattern_bindings_inside_arm() {
    let lowered = common::lower_ok("fn main(value: i32) -> i32 { match value { bound => bound } }");
    let resolved = resolve_names(&lowered)
        .unwrap()
        .into_checked()
        .expect("resolver should succeed");
    let function = &lowered.module.functions[0];
    let block = lowered
        .module
        .block(function.implementation.script_body().unwrap());
    let tail_expr = block.tail_expr.expect("tail expr");

    let (pattern_local, arm_expr) = match &lowered.module.expr(tail_expr).kind {
        ExprKind::Match { arms, .. } => {
            let arm = &arms[0];
            let pattern_local = match &lowered.module.pattern(arm.pattern).kind {
                PatternKind::Name { local, .. } => *local,
                other => panic!("unexpected pattern kind: {other:?}"),
            };
            (pattern_local, arm.expr)
        }
        other => panic!("unexpected expr kind: {other:?}"),
    };

    assert_eq!(
        resolved.expr_resolution(arm_expr),
        Some(ResolvedName::Local(pattern_local))
    );
}

#[test]
fn resolves_const_names_in_function_body() {
    let lowered = common::lower_ok(
        r#"
const VERSION: i32 = 1;

fn main() -> i32 { VERSION }
"#,
    );
    let resolved = resolve_names(&lowered)
        .unwrap()
        .into_checked()
        .expect("resolver should succeed");
    let function = lowered
        .module
        .functions
        .iter()
        .find(|function| function.name == "main")
        .expect("expected main function");
    let block = lowered
        .module
        .block(function.implementation.script_body().unwrap());
    let tail_expr = block.tail_expr.expect("tail expr");

    assert_eq!(
        resolved.expr_resolution(tail_expr),
        Some(ResolvedName::Const(lowered.module.consts[0].id))
    );
}

#[test]
fn collects_module_trait_impl_and_type_namespaces() {
    let lowered = common::lower_ok(
        r#"
mod gameplay;

trait Display {
    fn show(self) -> String;
}

struct Player {
    val name: String,
}

impl Display for Player {
    fn show(self) -> String {
        self.name
    }
}

fn main() -> i32 { 1 }
"#,
    );
    let result = resolve_names(&lowered).unwrap();
    assert!(
        result
            .diagnostics()
            .iter()
            .any(|diagnostic| matches!(diagnostic.kind, DiagnosticKind::UnknownName { .. }))
    );
    let resolved = result.facts();

    assert_eq!(
        resolved.items.lookup("gameplay", NameNamespace::Type),
        Some(NameResolution::Unresolved),
        "a missing module keeps its blocking name without inventing a namespace",
    );
    assert!(
        resolved
            .items
            .lookup("Display", NameNamespace::Type)
            .is_some_and(|r| matches!(r.target(), Some(ResolvedName::Trait(_))))
    );
    assert!(
        resolved
            .items
            .lookup("Player", NameNamespace::Type)
            .is_some_and(|r| matches!(r.target(), Some(ResolvedName::Struct(_))))
    );
    assert!(
        resolved
            .items
            .lookup("main", NameNamespace::Value)
            .is_some_and(|r| matches!(r.target(), Some(ResolvedName::Function(_))))
    );
    assert_eq!(resolved.items.impl_count(), 1);
}

#[test]
fn same_named_user_functions_are_not_reflection_helpers() {
    let module = SourceFile::new(
        "profile.kgr",
        "fn type_of(value: i32) -> i32 { value + 1 } fn main() -> i32 { type_of(41) }",
    );
    let checked = analyze_source(&module, foundation_catalog::shared().unwrap())
        .expect("installed declaration analysis")
        .into_codegen()
        .expect("resolved user function does not need reflection permission");
    let main = &checked.lowered.module.functions[1];
    let call = checked
        .lowered
        .module
        .block(main.implementation.script_body().unwrap())
        .tail_expr
        .unwrap();
    assert_eq!(
        checked
            .typed
            .type_table
            .call_resolution(call)
            .unwrap()
            .target,
        CallTarget::Function(checked.lowered.module.functions[0].id)
    );
}
