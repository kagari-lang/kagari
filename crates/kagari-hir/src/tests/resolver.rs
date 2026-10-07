use kagari_source::{diagnostic::DiagnosticKind, source::SourceFile};
use kagari_stdlib::catalog as foundation_catalog;

use crate::{
    analyze_source,
    hir::{expr::ExprKind, pattern::PatternKind, stmt::StmtKind},
    resolver::{collect::resolve_names, resolved::ResolvedName, table::NameResolution},
    tests::common,
    typeck::table::CallTarget,
};

#[test]
fn standard_item_spellings_require_installed_declarations() {
    for name in [
        "core",
        "std::collections",
        "Eq",
        "Some",
        "None",
        "Option::Some",
        "Vec::new",
        "demo::native::choose",
    ] {
        let lowered = common::lower_ok(&format!("fn main() {{ {name}; }}"));
        let resolved = resolve_names(&lowered);
        let function = &lowered.module.functions[0];
        let statement = lowered.module.block(function.body.unwrap()).statements[0];
        let StmtKind::Expr(expression) = lowered.module.stmt(statement).kind else {
            panic!("name expression statement");
        };
        assert_eq!(resolved.facts().expr_resolution(expression), None, "{name}");
        let checked = common::check_module(&lowered, resolved.facts(), None);
        assert!(checked.diagnostics().iter().any(|diagnostic| {
            matches!(&diagnostic.kind, DiagnosticKind::UnknownName { name: missing } if missing == name)
        }), "{name}: {:?}", checked.diagnostics());
    }
}

#[test]
fn reports_duplicate_function_names() {
    let lowered = common::lower_ok(
        r#"
fn foo() {}
fn foo() {}
"#,
    );

    let diagnostics = resolve_names(&lowered)
        .into_checked()
        .expect_err("resolver should reject duplicates");

    assert_eq!(diagnostics.len(), 1);
    assert_eq!(
        diagnostics[0].kind,
        DiagnosticKind::DuplicateDeclaration {
            name: "foo".to_string(),
        }
    );
    assert_eq!(
        diagnostics[0].to_string(),
        "Error: ambiguous declaration `foo` at 13..24"
    );
}

#[test]
fn resolves_params_and_locals_in_function_body() {
    let lowered = common::lower_ok("fn main(value: i32) -> i32 { val next: i32 = value; next }");
    let resolved = resolve_names(&lowered)
        .into_checked()
        .expect("resolver should succeed");
    let function = &lowered.module.functions[0];

    let block = lowered.module.block(function.body.unwrap());
    let let_stmt = lowered.module.stmt(block.statements[0]);
    let (let_local, init_expr) = match &let_stmt.kind {
        StmtKind::Binding {
            local, initializer, ..
        } => (*local, *initializer),
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
}

#[test]
fn resolves_named_match_pattern_bindings_inside_arm() {
    let lowered = common::lower_ok("fn main(value: i32) -> i32 { match value { bound => bound } }");
    let resolved = resolve_names(&lowered)
        .into_checked()
        .expect("resolver should succeed");
    let function = &lowered.module.functions[0];
    let block = lowered.module.block(function.body.unwrap());
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
        .into_checked()
        .expect("resolver should succeed");
    let function = lowered
        .module
        .functions
        .iter()
        .find(|function| function.name == "main")
        .expect("expected main function");
    let block = lowered.module.block(function.body.unwrap());
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
    let result = resolve_names(&lowered);
    assert!(
        result
            .diagnostics()
            .iter()
            .any(|diagnostic| matches!(diagnostic.kind, DiagnosticKind::UnknownName { .. }))
    );
    let resolved = result.facts();

    assert_eq!(
        resolved.items.lookup("gameplay"),
        Some(NameResolution::Unresolved),
        "a missing module keeps its blocking name without inventing a namespace",
    );
    assert!(
        resolved
            .items
            .lookup("Display")
            .is_some_and(|r| matches!(r.target(), Some(ResolvedName::Trait(_))))
    );
    assert!(
        resolved
            .items
            .lookup("Player")
            .is_some_and(|r| matches!(r.target(), Some(ResolvedName::Struct(_))))
    );
    assert!(
        resolved
            .items
            .lookup("main")
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
    let checked = analyze_source(&module, foundation_catalog::shared())
        .expect("installed declaration analysis")
        .into_codegen()
        .expect("resolved user function does not need reflection permission");
    let main = &checked.lowered.module.functions[1];
    let call = checked
        .lowered
        .module
        .block(main.body.unwrap())
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
