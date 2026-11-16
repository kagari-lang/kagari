use crate::query_tests::native as fixture;
use crate::query_tests::native_fixture;
use crate::{AnalyzerQueries, query_tests::test_analysis};
use kagari_common::identity::{DefinitionKind, ModuleIdentity, PackageId};
use kagari_hir::{
    declarations::DeclarationId,
    hir::{expr::ExprKind, stmt::StmtKind},
    semantic::signatures::FunctionImplementation,
    typeck::table::CallTarget,
    types::TypeId,
};
use kagari_source::source_database::{SourceDatabase, SourceLayer};
use kagari_stdlib::catalog as foundation_catalog;
use kagari_types::{scalar::BuiltinType, ty::Ty};

#[test]
fn native_generic_scalar_calls_keep_their_exact_declared_types() {
    let text = "use demo::bindings::{echo, choose}; fn narrow() { echo(1i8); choose(1i8, 2i8, 3i8); } fn wide() { echo(1u64); choose(1u64, 2u64, 3u64); }";
    let mut sources = SourceDatabase::default();
    let root = sources
        .set("numeric-bindings.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let snapshot = fixture::database()
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    let analysis = snapshot.file(root).unwrap();
    assert!(
        analysis.result().diagnostics().is_empty(),
        "{:?}",
        analysis.result().diagnostics()
    );
    let authoring_facts = analysis.to_unverified(&Default::default()).unwrap();
    let facts = authoring_facts.records().facts();
    let mut seen = 0;
    for function in &facts.lowered.module.functions {
        let expected = TypeId::Builtin(if function.name == "narrow" {
            BuiltinType::I8
        } else {
            BuiltinType::U64
        });
        let block = facts
            .lowered
            .module
            .block(function.implementation.script_body().unwrap());
        for statement in &block.statements {
            let StmtKind::Expr(expression) = facts.lowered.module.stmt(*statement).kind else {
                panic!("call statement");
            };
            let call = facts
                .typed
                .type_table()
                .call_resolution(expression)
                .unwrap();
            let CallTarget::SourceFunction(id) = &call.target else {
                panic!("checked native declaration");
            };
            let imported = facts.imported_functions.target(id).unwrap();
            assert_eq!(
                *facts
                    .declarations
                    .definitions()
                    .resolve(imported.declaration)
                    .unwrap()
                    .module(),
                native_fixture::module().records().identity
            );
            assert!(matches!(
                imported.signature.implementation,
                FunctionImplementation::Native(_)
            ));
            assert_eq!(
                call.type_arguments.as_slice(),
                std::slice::from_ref(&expected)
            );
            let applied = call.signature.as_ref().unwrap();
            assert_eq!(applied.return_type, expected);
            assert!(applied.params.iter().all(|ty| *ty == expected));
            assert_eq!(
                facts.typed.type_table().expr_type(expression),
                Some(expected.clone())
            );
            assert_eq!(
                analysis
                    .queries()
                    .call_signature_at(facts.lowered.source_map.expr_span(expression).start)
                    .unwrap()
                    .declaration,
                imported.site.id
            );
            seen += 1;
        }
    }
    assert_eq!(seen, 4);
}

#[test]
fn native_generic_calls_use_source_signatures_and_ordinary_inference() {
    let text = r#"
use demo::bindings::{choose as bound, echo};
fn identity<T>(value: T) -> T { value }
fn main() -> i32 {
    val value = bound(identity(42), 0, 100);
    echo(value)
}
"#;
    let mut sources = SourceDatabase::default();
    let root = sources
        .set("callables.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let snapshot = fixture::database()
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    let analysis = snapshot.file(root).unwrap();
    assert!(
        analysis.result().diagnostics().is_empty(),
        "{:?}",
        analysis.result().diagnostics()
    );
    let authoring_facts = analysis.to_unverified(&Default::default()).unwrap();
    let facts = authoring_facts.records().facts();
    let mut seen = Vec::new();
    for (id, expression) in facts.lowered.module.nodes.expressions() {
        let ExprKind::Call { callee, .. } = &expression.kind else {
            continue;
        };
        let name = match &facts.lowered.module.expr(*callee).kind {
            ExprKind::Name { name, .. } | ExprKind::Field { name, .. } => name.as_str(),
            _ => continue,
        };
        let expected = match name {
            "identity" => FunctionImplementation::Script,
            "bound" | "echo" => FunctionImplementation::Native(Default::default()),
            _ => continue,
        };
        let call = facts.typed.type_table().call_resolution(id).unwrap();
        let signature = match &call.target {
            CallTarget::Function(target) if name == "identity" => facts
                .typed
                .signatures()
                .functions()
                .iter()
                .find(|function| function.id == *target)
                .unwrap(),
            CallTarget::SourceFunction(target) if name != "identity" => {
                let imported = facts.imported_functions.target(target).unwrap();
                assert!(imported.site.source_location().is_none());
                assert_eq!(
                    *facts
                        .declarations
                        .definitions()
                        .resolve(imported.declaration)
                        .unwrap()
                        .module(),
                    native_fixture::module().records().identity
                );
                &imported.signature
            }
            other => panic!("{name} must retain its checked declaration target: {other:?}"),
        };
        assert_eq!(signature.implementation, expected, "{name}");
        let applied = call
            .signature
            .as_ref()
            .expect("retained callable application");
        assert_eq!(applied.return_type, TypeId::Builtin(BuiltinType::I32));
        let scalar = TypeId::Builtin(BuiltinType::I32);
        let expected_params = match name {
            "bound" => vec![scalar.clone(); 3],
            "identity" => vec![scalar.clone()],
            "echo" => vec![scalar],
            _ => unreachable!(),
        };
        assert_eq!(applied.params, expected_params, "{name}");
        let queried = analysis
            .queries()
            .call_signature_at(facts.lowered.source_map.expr_span(*callee).start)
            .unwrap();
        assert_eq!(queried.result, TypeId::Builtin(BuiltinType::I32));
        assert_eq!(
            queried.parameters.len(),
            if name == "bound" { 3 } else { 1 }
        );
        assert!(
            queried
                .parameters
                .iter()
                .all(|(_, ty)| *ty == TypeId::Builtin(BuiltinType::I32))
        );
        assert_eq!(
            call.type_arguments,
            [TypeId::Builtin(BuiltinType::I32)],
            "{name}"
        );
        assert_eq!(
            facts.typed.type_table().expr_type(id),
            Some(TypeId::Builtin(BuiltinType::I32))
        );
        seen.push(name);
    }
    seen.sort_unstable();
    assert_eq!(seen, ["bound", "echo", "identity"]);
}

#[test]
fn call_signature_queries_keep_declared_types_for_invalid_source_trait_and_host_calls() {
    let text = "trait Check { fn verify(self, flag:bool)->i32; } struct Item {} impl Check for Item { fn verify(self, flag:bool)->i32 { 1 } } fn script(value:bool, count:i32)->bool { value } fn main() { script(1); demo::echo(false); Item {}.verify(1); }";
    let mut sources = SourceDatabase::default();
    let root = sources
        .set("signature-help.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let host = native_fixture::function_module(
        ModuleIdentity {
            package: PackageId("demo".into()),
            path: vec![],
        },
        "echo",
        vec![("input", Ty::Builtin(BuiltinType::I32))],
        Ty::Builtin(BuiltinType::I32),
    );
    let expected = host
        .records()
        .definition(DefinitionKind::Function, "echo", host.definitions())
        .unwrap();
    let expected = host.definitions().resolve(expected).unwrap().to_path();
    let mut database = test_analysis();
    let mut modules = foundation_catalog::shared().unwrap();
    modules.push(host);
    database.set_native_modules(modules);
    let snapshot = database
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    let file = snapshot.file(root).unwrap();
    assert!(!file.result().diagnostics().is_empty());
    let script = file
        .queries()
        .call_signature_at(text.find("script(1)").unwrap())
        .unwrap();
    assert_eq!(
        script.parameters,
        [
            ("value".into(), TypeId::Builtin(BuiltinType::Bool)),
            ("count".into(), TypeId::Builtin(BuiltinType::I32))
        ]
    );
    assert_eq!(script.result, TypeId::Builtin(BuiltinType::Bool));
    let signature = file
        .queries()
        .call_signature_at(text.find("demo::echo").unwrap())
        .unwrap();
    assert_eq!(
        signature.declaration,
        DeclarationId::Definition(file.definitions().lookup(&expected).unwrap())
    );
    assert_eq!(
        signature.parameters,
        [("input".into(), TypeId::Builtin(BuiltinType::I32))]
    );
    assert_eq!(signature.result, TypeId::Builtin(BuiltinType::I32));
    let method = file
        .queries()
        .call_signature_at(text.find("verify(1)").unwrap())
        .unwrap();
    assert_eq!(
        method.parameters,
        [("flag".into(), TypeId::Builtin(BuiltinType::Bool))]
    );
    assert_eq!(method.result, TypeId::Builtin(BuiltinType::I32));
    let authoring_facts = file.to_unverified(&Default::default()).unwrap();
    let facts = authoring_facts.records().facts();
    let mut checked = 0;
    for (site, expression) in facts.lowered.module.nodes.expressions() {
        let ExprKind::Call { callee, .. } = &expression.kind else {
            continue;
        };
        let Some(call) = facts.typed.type_table().call_resolution(site) else {
            continue;
        };
        let Some(applied) = &call.signature else {
            continue;
        };
        let queried = file
            .queries()
            .call_signature_at(facts.lowered.source_map.expr_span(*callee).start)
            .unwrap();
        let parameters = applied.params[usize::from(call.receiver.is_some())..].to_vec();
        assert_eq!(
            parameters,
            queried
                .parameters
                .into_iter()
                .map(|(_, ty)| ty)
                .collect::<Vec<_>>()
        );
        assert_eq!(applied.return_type, queried.result);
        checked += 1;
    }
    assert_eq!(checked, 3);
}
