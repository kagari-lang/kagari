use super::*;
use crate::hir::{BodyOwner, HirOwner, StmtKind};
use kagari_common::source_database::{SourceDatabase, SourceLayer};

#[test]
fn lowering_records_owners_for_interleaved_functions_and_constants() {
    let text = "const N: i32 = 2; fn first(x: i32) -> i32 { var local = x; local += N; match local { 2 => 3, other => other } } fn second() -> i32 { 4 }";
    let mut sources = SourceDatabase::default();
    let id = sources
        .set("owners.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let snapshot = AnalysisDatabase::default()
        .snapshot(sources.snapshot(), Default::default(), &Default::default())
        .unwrap();
    let file = snapshot.file(id).unwrap();
    assert!(
        file.result().diagnostics().is_empty(),
        "{:?}",
        file.result().diagnostics()
    );
    let facts = file.result().facts();
    let module = &facts.lowered.module;
    for function in &module.functions {
        let expected = HirOwner::Body(BodyOwner::Function(function.id));
        assert_eq!(function.body.owner(), expected);
        for parameter in &function.params {
            assert_eq!(parameter.id.owner(), expected);
            assert_eq!(parameter.ty.owner(), expected);
        }
        for statement in &module.block(function.body).statements {
            assert_eq!(statement.owner(), expected);
            match &module.stmt(*statement).kind {
                StmtKind::Binding {
                    local, initializer, ..
                } => {
                    assert_eq!(local.owner(), expected);
                    assert_eq!(initializer.owner(), expected);
                }
                StmtKind::Assign { target, value, .. } => {
                    assert_eq!(target.owner(), expected);
                    assert_eq!(value.owner(), expected);
                }
                _ => {}
            }
        }
    }
    let constant = &module.consts[0];
    assert_eq!(
        constant.initializer.owner(),
        HirOwner::Body(BodyOwner::Const(constant.id))
    );
    assert_eq!(constant.ty.unwrap().owner(), constant.initializer.owner());
    for (expr, _) in module.body.expressions() {
        assert_ne!(expr.owner(), HirOwner::Declaration);
    }
    for scope in facts.names.scopes() {
        for binding in &scope.bindings {
            let owner = match binding.resolved {
                crate::resolver::ResolvedName::Param(id) => id.owner(),
                crate::resolver::ResolvedName::Local(id) => id.owner(),
                _ => panic!("body binding"),
            };
            assert_eq!(owner, HirOwner::Body(scope.owner));
        }
    }
}

#[test]
fn implicit_receiver_shares_declaration_type_without_losing_method_ownership() {
    let text = "struct P { val n: i32 } trait Show { fn show(self) -> i32; } impl Show for P { fn show(self) -> i32 { self.n } } fn missing(value) { val x = ; }";
    let lowered = crate::lower::lower_module(&SourceFile::new("receiver.kgr", text));
    let implementation = &lowered.module.impls[0];
    let method = lowered
        .module
        .functions
        .iter()
        .find(|f| f.id == implementation.methods[0].function)
        .unwrap();
    assert_eq!(method.params[0].ty, implementation.for_type.unwrap());
    assert_eq!(method.params[0].ty.owner(), HirOwner::Declaration);
    assert_eq!(
        method.params[0].id.owner(),
        HirOwner::Body(BodyOwner::Function(method.id))
    );
    let missing = lowered
        .module
        .functions
        .iter()
        .find(|f| f.name == "missing")
        .unwrap();
    assert_eq!(
        missing.params[0].ty.owner(),
        HirOwner::Body(BodyOwner::Function(missing.id))
    );
    assert!(lowered.module.body.expressions().any(|(id, data)| {
        matches!(data.kind, crate::hir::ExprKind::Missing)
            && id.owner() == HirOwner::Body(BodyOwner::Function(missing.id))
    }));
    // Synthetic/missing source ranges do not decide ownership.
    let names = crate::resolver::resolve_names(&lowered);
    assert!(!names.facts().scopes().is_empty());
}

#[test]
fn resolver_rejects_cross_body_edges_even_inside_the_same_arena() {
    let mut lowered = crate::lower::lower_module(&SourceFile::new(
        "edges.kgr",
        "fn first() -> i32 { 1 } fn second() -> i32 { 2 }",
    ));
    let first = lowered.module.functions[0].body;
    let second = lowered.module.functions[1].body;
    assert_eq!(first.arena(), second.arena());
    assert_ne!(first.owner(), second.owner());
    let other_expr = lowered.module.block(second).tail_expr.unwrap();
    lowered.module.body.blocks[first.index()].1.tail_expr = Some(other_expr);
    assert!(std::panic::catch_unwind(|| crate::resolver::resolve_names(&lowered)).is_err());
}

#[test]
fn stored_node_and_source_owners_reject_an_internally_mistagged_id() {
    let lowered = crate::lower::lower_module(&SourceFile::new(
        "tags.kgr",
        "fn first() -> i32 { 1 } fn second() -> i32 { 2 }",
    ));
    let first = lowered.module.functions[0].body;
    let second = lowered.module.functions[1].body;
    let expr = lowered.module.block(first).tail_expr.unwrap();
    let mistagged = crate::hir::ExprId::new(expr.arena(), second.owner(), expr.index());
    assert_ne!(expr, mistagged);
    assert!(std::panic::catch_unwind(|| lowered.module.expr(mistagged)).is_err());
    assert!(std::panic::catch_unwind(|| lowered.source_map.expr_span(mistagged)).is_err());
}
