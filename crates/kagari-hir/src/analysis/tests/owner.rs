use crate::analysis::*;
use crate::{
    hir::{
        expr::ExprKind,
        ids::{BodyOwner, ExprId, FunctionId, HirOwner, ParamId},
        stmt::StmtKind,
    },
    resolver::{collect::resolve_names, resolved::ResolvedName},
    tests::test_analysis,
    types::TypeId,
};
use kagari_source::source_database::{SourceDatabase, SourceLayer};
use kagari_types::scalar::BuiltinType;

#[test]
fn lowering_records_owners_for_interleaved_functions_and_constants() {
    let text = include_str!("../../../tests/fixtures/analysis/parameter_owners.kgr");
    let mut sources = SourceDatabase::default();
    let id = sources
        .set("owners.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let snapshot = test_analysis()
        .snapshot(sources.snapshot(), &Default::default())
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
        assert_eq!(function.body.unwrap().owner(), expected);
        for (index, parameter) in function.params.iter().enumerate() {
            assert_eq!(parameter.id.index(), index);
            assert!(std::ptr::eq(module.parameter(parameter.id), parameter));
            let span = facts.lowered.source_map.param_span(parameter.id);
            assert_eq!(&text[span.start..span.end], parameter.name);
            assert_eq!(parameter.id.owner(), expected);
            assert_eq!(parameter.ty.owner(), expected);
        }
        for statement in &module.block(function.body.unwrap()).statements {
            assert_eq!(statement.owner(), expected);
            match &module.stmt(*statement).kind {
                StmtKind::Binding {
                    pattern,
                    initializer,
                    ..
                } => {
                    assert_eq!(pattern.owner(), expected);
                    for (local, _) in module.body.pattern_bindings(*pattern) {
                        assert_eq!(local.owner(), expected);
                    }
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
    let left = module.functions[0].params[0].id;
    let right = module.functions[1].params[0].id;
    assert_eq!((left.index(), right.index()), (0, 0));
    assert_ne!(left, right);
    assert_eq!(module.parameter(left).name, "x");
    assert_eq!(module.parameter(right).name, "flag");
    assert_eq!(
        facts
            .typed
            .type_table
            .type_ref(module.parameter(left).ty)
            .map(|record| &record.ty),
        Some(&TypeId::Builtin(BuiltinType::I32))
    );
    assert_eq!(
        facts
            .typed
            .type_table
            .type_ref(module.parameter(right).ty)
            .map(|record| &record.ty),
        Some(&TypeId::Builtin(BuiltinType::Bool))
    );
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
                ResolvedName::Param(id) => id.owner(),
                ResolvedName::Local(id) => id.owner(),
                _ => panic!("body binding"),
            };
            assert_eq!(owner, HirOwner::Body(scope.owner));
        }
    }
}

#[test]
fn implicit_receiver_shares_declaration_type_without_losing_method_ownership() {
    let text = include_str!("../../../tests/fixtures/lower/receiver_recovery.kgr");
    let lowered = crate::lower::lower_module(&SourceFile::new("receiver.kgr", text));
    let implementation = &lowered.module.impls[0];
    let method = lowered
        .module
        .functions
        .iter()
        .find(|f| f.id == implementation.methods[0].function)
        .unwrap();
    for function in &lowered.module.functions {
        for (index, parameter) in function.params.iter().enumerate() {
            assert_eq!(parameter.id.index(), index);
            assert!(std::ptr::eq(
                lowered.module.parameter(parameter.id),
                parameter
            ));
            let span = lowered.source_map.param_span(parameter.id);
            assert_eq!(&text[span.start..span.end], parameter.name);
        }
    }
    assert_eq!(method.params[0].name, "self");
    assert_eq!(method.params[1].name, "value");
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
        matches!(data.kind, ExprKind::Missing)
            && id.owner() == HirOwner::Body(BodyOwner::Function(missing.id))
    }));
    // Synthetic/missing source ranges do not decide ownership.
    let names = resolve_names(&lowered).unwrap();
    assert!(!names.facts().scopes().is_empty());
}

#[test]
fn resolver_rejects_cross_body_edges_even_inside_the_same_arena() {
    let mut lowered = crate::lower::lower_module(&SourceFile::new(
        "edges.kgr",
        "fn first() -> i32 { 1 } fn second() -> i32 { 2 }",
    ));
    let first = lowered.module.functions[0].body.unwrap();
    let second = lowered.module.functions[1].body.unwrap();
    assert_eq!(first.arena(), second.arena());
    assert_ne!(first.owner(), second.owner());
    let other_expr = lowered.module.block(second).tail_expr.unwrap();
    lowered.module.body.blocks[first.index()].1.tail_expr = Some(other_expr);
    assert!(std::panic::catch_unwind(|| resolve_names(&lowered).unwrap()).is_err());
}

#[test]
fn stored_node_and_source_owners_reject_an_internally_mistagged_id() {
    let lowered = crate::lower::lower_module(&SourceFile::new(
        "tags.kgr",
        include_str!("../../../tests/fixtures/analysis/parameter_lookup.kgr"),
    ));
    let first = lowered.module.functions[0].body.unwrap();
    let second = lowered.module.functions[1].body.unwrap();
    let expr = lowered.module.block(first).tail_expr.unwrap();
    let mistagged = ExprId::new(expr.arena(), second.owner(), expr.index());
    assert_ne!(expr, mistagged);
    assert!(std::panic::catch_unwind(|| lowered.module.expr(mistagged)).is_err());
    assert!(std::panic::catch_unwind(|| lowered.source_map.expr_span(mistagged)).is_err());

    let module = &lowered.module;
    let parameter = module.functions[0].params[0].id;
    let other = module.functions[1].params[0].id;
    assert_eq!((parameter.index(), other.index()), (0, 0));
    assert_eq!(module.parameter(parameter).name, "x");
    assert_eq!(module.parameter(other).name, "flag");
    for invalid in [
        ParamId::new(parameter.arena(), HirOwner::Declaration, 0),
        ParamId::new(
            parameter.arena(),
            HirOwner::Body(BodyOwner::Const(module.consts[0].id)),
            0,
        ),
        ParamId::new(
            parameter.arena(),
            HirOwner::Body(BodyOwner::Function(FunctionId::new(module.functions.len()))),
            0,
        ),
        ParamId::new(parameter.arena(), second.owner(), 1),
        ParamId::new(
            parameter.arena(),
            HirOwner::Body(BodyOwner::Function(module.functions[2].id)),
            0,
        ),
    ] {
        assert!(
            std::panic::catch_unwind(|| module.parameter(invalid)).is_err(),
            "{invalid:?}"
        );
        assert!(
            std::panic::catch_unwind(|| lowered.source_map.param_span(invalid)).is_err(),
            "{invalid:?}"
        );
    }
    let mut corrupted = module.clone();
    corrupted.functions[0].params.swap(0, 1);
    assert!(std::panic::catch_unwind(|| corrupted.parameter(parameter)).is_err());
    let mut corrupted = module.clone();
    corrupted.functions.swap(0, 1);
    assert!(std::panic::catch_unwind(|| corrupted.parameter(parameter)).is_err());
}
