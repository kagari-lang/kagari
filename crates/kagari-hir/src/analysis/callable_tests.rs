use crate::{
    declarations::DeclarationId,
    hir::{expr::ExprKind, stmt::StmtKind},
    host::HostDeclarations,
    native::NativeBinding,
    resolver::resolved::ResolvedName,
    tests::{native as fixture, test_analysis},
    typeck::{FunctionImplementation, table::CallTarget},
    types::TypeId,
};
use kagari_source::{
    diagnostic::DiagnosticKind,
    source::SourceFile,
    source_database::{SourceDatabase, SourceLayer},
};
use kagari_stdlib::catalog as foundation_catalog;
use kagari_types::{
    callable::NativeDefaultApplication,
    host_interface::{
        HostFunctionDeclaration, HostInterface, HostParameter, HostPassingStyle,
        type_declaration::{HostTypeDeclaration, HostTypeOwnership},
        value_type::HostValueType,
    },
    scalar::BuiltinType,
};

#[test]
fn async_callable_typing_contract() {
    let text = r#"
async fn identity<T>(value: T) -> T { return value; }
async fn nested() -> Future<i32> { identity(1) }
async fn task_result<T>(task: Task<T>) -> T { task.await }
async fn nested_task(task: Task<Future<i32>>) -> Future<i32> { task.await }
async fn empty<T>() -> Vec<T> { [] }
async fn inferred_future() -> Vec<i32> { empty().await }
async fn diverging() -> i32 { (loop {}).await }
async fn business(value: Result<i32, i32>) -> Result<i32, i32> {
    Ok(identity(value?).await)
}
fn accept<F: Fn(i32) -> Future<i32>>(callback: F) -> Future<i32> { callback(1) }
fn submit<T, F: Fn() -> Future<T>>(scope: TaskScope, factory: F) -> Result<Task<T>, SpawnError> { scope.spawn(factory) }
fn inferred_task(scope: TaskScope) -> Result<Task<i32>, SpawnError> { submit(scope, async || 42) }
fn stop<T>(task: Task<T>) { task.cancel(); }
fn factory() -> Future<i32> {
    val callback: fn(i32) -> Future<i32> = async |x| identity(x).await;
    val fallible: fn(Result<i32, i32>) -> Future<Result<i32, i32>> = async |x| { return Ok(identity(x?).await); };
    accept(async |x| identity(x).await);
    accept(callback)
}
async fn looping(items: Vec<i32>) -> i32 {
    var total = 0;
    for item in items { total += identity(item).await; }
    total
}
fn plain() -> Future<i32> { (|| identity(1))() }
"#;
    let mut sources = SourceDatabase::default();
    let root = sources
        .set("async-types.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let mut database = test_analysis();
    let snapshot = database
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    let analysis = snapshot.file(root).unwrap();
    assert!(
        analysis.result().diagnostics().is_empty(),
        "{:?}",
        analysis.result().diagnostics()
    );
    let facts = analysis.result().facts();
    let nested = facts
        .typed
        .functions
        .iter()
        .find(|f| f.name == "nested")
        .unwrap();
    let TypeId::NativeObject(outer) = &nested.return_type else {
        panic!("cold Future");
    };
    let TypeId::NativeObject(inner) = &outer.arguments[0] else {
        panic!("nested Future");
    };
    assert_eq!(outer.declaration, inner.declaration);
    assert_eq!(inner.arguments, [TypeId::Builtin(BuiltinType::I32)]);

    sources.set("async-types.kgr", "struct Future<T> { val value: T } async fn value() -> i32 { 1 } fn get() -> core::future::Future<i32> { value() }".into(), SourceLayer::Base).unwrap();
    let snapshot = database
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    assert!(
        snapshot
            .file(root)
            .unwrap()
            .result()
            .diagnostics()
            .is_empty(),
        "{:?}",
        snapshot.file(root).unwrap().result().diagnostics()
    );

    // Reusing a body edit must retain the factory signature; removing the
    // modifier must invalidate it even when the completed body is unchanged.
    for (text, valid) in [
        (
            "async fn value() -> i32 { 1 } fn get() -> Future<i32> { value() }",
            true,
        ),
        (
            "async fn value() -> i32 { 2 } fn get() -> Future<i32> { value() }",
            true,
        ),
        (
            "fn value() -> i32 { 2 } fn get() -> Future<i32> { value() }",
            false,
        ),
    ] {
        sources
            .set("async-types.kgr", text.into(), SourceLayer::Base)
            .unwrap();
        let snapshot = database
            .snapshot(sources.snapshot(), &Default::default())
            .unwrap();
        let analysis = snapshot.file(root).unwrap();
        assert_eq!(
            analysis.result().diagnostics().is_empty(),
            valid,
            "{:?}",
            analysis.result().diagnostics()
        );
        let signature = analysis
            .call_signature_at(text.rfind("value()").unwrap())
            .unwrap();
        assert_eq!(matches!(signature.result, TypeId::NativeObject(_)), valid);
    }

    for (text, code) in [
        (
            "async fn value() -> i32 { 1 } fn bad() -> i32 { value().await }",
            "KG_AWAIT_OUTSIDE_ASYNC",
        ),
        (
            "async fn value() -> i32 { 1 } async fn bad(items: Vec<i32>) { items.retain(|x| value().await == x); }",
            "KG_AWAIT_OUTSIDE_ASYNC",
        ),
        (
            "async fn bad() -> i32 { 1.await }",
            "KG_TYPE_INVALID_AWAIT_OPERAND",
        ),
        (
            "async fn value() -> i32 { 1 } fn bad(scope: TaskScope) { scope.spawn(value()); }",
            "KG_TYPE_ARGUMENT_TYPE_MISMATCH",
        ),
        (
            "fn bad(scope: TaskScope) { scope.spawn(|| {}); }",
            "KG_TYPE_ARGUMENT_TYPE_MISMATCH",
        ),
        (
            "trait Bad { async fn value() -> i32; }",
            "KG_SYNTAX_UNSUPPORTED",
        ),
    ] {
        sources
            .set("async-types.kgr", text.into(), SourceLayer::Base)
            .unwrap();
        let snapshot = database
            .snapshot(sources.snapshot(), &Default::default())
            .unwrap();
        let analysis = snapshot.file(root).unwrap();
        assert!(
            analysis
                .result()
                .diagnostics()
                .iter()
                .any(|diagnostic| diagnostic.kind.code() == code),
            "{text}: {:?}",
            analysis.result().diagnostics()
        );
    }
    let mut host = HostTypeDeclaration::new("host.State");
    host.ownership = HostTypeOwnership::HostRoot;
    database.set_host_declarations(
        HostDeclarations::new(HostInterface {
            paths: vec![],
            types: vec![host],
            functions: vec![],
        })
        .unwrap(),
    );
    for text in [
        "async fn bad(value:host::State) {}",
        "async fn bad(value:(i32,host::State)) {}",
        "fn bad() { val f=async |value:host::State| {}; }",
        "fn bad() { val f:fn(host::State)->Future<()> = async |value| {}; }",
    ] {
        sources
            .set("async-types.kgr", text.into(), SourceLayer::Base)
            .unwrap();
        let snapshot = database
            .snapshot(sources.snapshot(), &Default::default())
            .unwrap();
        let analysis = snapshot.file(root).unwrap();
        assert!(
            analysis
                .result()
                .diagnostics()
                .iter()
                .any(|diagnostic| matches!(
                    diagnostic.kind,
                    DiagnosticKind::InvalidAsyncCapture { .. }
                )),
            "{text}: {:?}",
            analysis.result().diagnostics()
        );
    }
}

#[test]
fn native_generic_scalar_calls_keep_their_exact_declared_types() {
    let text = "use demo::native::{echo, choose}; fn narrow() { echo(1i8); choose(1i8, 2i8, 3i8); } fn wide() { echo(1u64); choose(1u64, 2u64, 3u64); }";
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
    let facts = authoring_facts.facts();
    let mut seen = 0;
    for function in &facts.lowered.module.functions {
        let expected = TypeId::Builtin(if function.name == "narrow" {
            BuiltinType::I8
        } else {
            BuiltinType::U64
        });
        let block = facts.lowered.module.block(function.body.unwrap());
        for statement in &block.statements {
            let StmtKind::Expr(expression) = facts.lowered.module.stmt(*statement).kind else {
                panic!("call statement");
            };
            let call = facts.typed.type_table.call_resolution(expression).unwrap();
            let CallTarget::SourceFunction(id) = &call.target else {
                panic!("checked native declaration");
            };
            let imported = facts.imported_functions.target(id).unwrap();
            assert_eq!(imported.declaration.module, fixture::module().identity);
            assert!(matches!(
                imported.signature.implementation,
                FunctionImplementation::Native(NativeBinding::Entry(_))
            ));
            assert_eq!(
                call.type_arguments.as_slice(),
                std::slice::from_ref(&expected)
            );
            let applied = call.signature.as_ref().unwrap();
            assert_eq!(applied.return_type, expected);
            assert!(applied.params.iter().all(|ty| *ty == expected));
            assert_eq!(
                facts.typed.type_table.expr_type(expression),
                Some(expected.clone())
            );
            assert_eq!(
                analysis
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
fn required_script_and_native_methods_keep_distinct_signature_implementations() {
    let mut sources = SourceDatabase::default();
    let root = sources
        .set(
            "implementations.kgr",
            "use demo::native::NativeRead; trait Local { fn required(self)->i32; fn defaulted(self)->i32 { 42 } }".into(),
            SourceLayer::Base,
        )
        .unwrap();
    let snapshot = fixture::database()
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    let authoring_facts = snapshot
        .file(root)
        .unwrap()
        .to_unverified(&Default::default())
        .unwrap();
    let facts = authoring_facts.facts();
    for (name, expected) in [
        ("required", FunctionImplementation::Required),
        ("defaulted", FunctionImplementation::Script),
    ] {
        assert_eq!(
            facts
                .typed
                .functions
                .iter()
                .find(|function| function.name == name)
                .unwrap()
                .implementation,
            expected
        );
    }
    let iterator = facts
        .aggregates
        .traits()
        .find(|item| item.declaration.name == "NativeRead")
        .unwrap();
    let file = snapshot.file(iterator.declaration.location.file).unwrap();
    let authoring = file.to_unverified(&Default::default()).unwrap();
    for name in ["read", "fixed"] {
        let expected =
            FunctionImplementation::Native(NativeBinding::Default(NativeDefaultApplication {
                declaration: fixture::module().definition(
                    kagari_common::identity::DefinitionKind::Function,
                    &format!("default_{name}"),
                ),
                arguments: vec![kagari_types::ty::Ty::SelfType(iterator.id.clone())],
            }));
        let method = iterator
            .methods
            .iter()
            .find(|method| method.name == name)
            .unwrap();
        let function = authoring
            .facts()
            .typed
            .functions
            .iter()
            .find(|function| {
                authoring
                    .facts()
                    .declarations
                    .definition(ResolvedName::Function(function.id))
                    == Some(&method.id)
            })
            .unwrap();
        assert_eq!(function.implementation, expected);
    }
}

#[test]
fn native_generic_calls_use_source_signatures_and_ordinary_inference() {
    let text = r#"
use demo::native::{choose as bound, echo};
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
    let facts = authoring_facts.facts();
    let mut seen = Vec::new();
    for (id, expression) in facts.lowered.module.body.expressions() {
        let ExprKind::Call { callee, .. } = &expression.kind else {
            continue;
        };
        let name = match &facts.lowered.module.expr(*callee).kind {
            ExprKind::Name { name, .. } | ExprKind::Field { name, .. } => name.as_str(),
            _ => continue,
        };
        let expected = match name {
            "identity" => FunctionImplementation::Script,
            "bound" | "echo" => {
                FunctionImplementation::Native(NativeBinding::Entry(fixture::module().definition(
                    kagari_common::identity::DefinitionKind::Function,
                    if name == "bound" { "choose" } else { "echo" },
                )))
            }
            _ => continue,
        };
        let call = facts.typed.type_table.call_resolution(id).unwrap();
        let signature = match &call.target {
            CallTarget::Function(target) if name == "identity" => facts
                .typed
                .functions
                .iter()
                .find(|function| function.id == *target)
                .unwrap(),
            CallTarget::SourceFunction(target) if name != "identity" => {
                let imported = facts.imported_functions.target(target).unwrap();
                assert_ne!(imported.site.location.file, root);
                assert_eq!(imported.declaration.module, fixture::module().identity);
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
            facts.typed.type_table.expr_type(id),
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
    let host = HostFunctionDeclaration::new(
        "demo.echo",
        vec![HostParameter {
            name: "input".into(),
            ty: HostValueType::I32,
            passing: HostPassingStyle::Owned,
        }],
        HostValueType::I32,
    );
    let mut database = test_analysis();
    database.set_host_declarations(
        HostDeclarations::new(HostInterface {
            paths: Vec::new(),
            types: Vec::new(),
            functions: vec![host.clone()],
        })
        .unwrap(),
    );
    let snapshot = database
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    let file = snapshot.file(root).unwrap();
    assert!(!file.result().diagnostics().is_empty());
    let script = file
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
        .call_signature_at(text.find("demo::echo").unwrap())
        .unwrap();
    assert_eq!(signature.declaration, DeclarationId::Definition(host.id));
    assert_eq!(
        signature.parameters,
        [("input".into(), TypeId::Builtin(BuiltinType::I32))]
    );
    assert_eq!(signature.result, TypeId::Builtin(BuiltinType::I32));
    let method = file
        .call_signature_at(text.find("verify(1)").unwrap())
        .unwrap();
    assert_eq!(
        method.parameters,
        [("flag".into(), TypeId::Builtin(BuiltinType::Bool))]
    );
    assert_eq!(method.result, TypeId::Builtin(BuiltinType::I32));
    let authoring_facts = file.to_unverified(&Default::default()).unwrap();
    let facts = authoring_facts.facts();
    let mut checked = 0;
    for (site, expression) in facts.lowered.module.body.expressions() {
        let ExprKind::Call { callee, .. } = &expression.kind else {
            continue;
        };
        let Some(call) = facts.typed.type_table.call_resolution(site) else {
            continue;
        };
        let Some(applied) = &call.signature else {
            continue;
        };
        let queried = file
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

#[test]
fn native_generic_permissions_do_not_bypass_bounds_or_script_export_rules() {
    for (text, native) in [
        (
            "use demo::native::choose as bound; fn main() { bound(1.0, 2.0, 3.0); }",
            true,
        ),
        (
            "#[native(fake)] pub fn bound<T>(value:T)->T { value } fn main() { bound(1); }",
            false,
        ),
    ] {
        let mut sources = SourceDatabase::default();
        let root = sources
            .set("native-authority.kgr", text.into(), SourceLayer::Base)
            .unwrap();
        let snapshot = fixture::database()
            .snapshot(sources.snapshot(), &Default::default())
            .unwrap();
        let analysis = snapshot.file(root).unwrap();
        let diagnostics = analysis.result().diagnostics();
        if native {
            assert!(
                diagnostics.iter().any(|diagnostic| matches!(
                    diagnostic.kind,
                    DiagnosticKind::GenericBoundNotSatisfied { .. }
                )),
                "{diagnostics:?}"
            );
            assert!(!diagnostics.iter().any(|diagnostic| matches!(
                diagnostic.kind,
                DiagnosticKind::PublicGenericFunction { .. }
            )));
        } else {
            assert!(diagnostics.iter().any(|diagnostic| matches!(
                diagnostic.kind,
                DiagnosticKind::UnknownAttribute { .. }
            )));
            assert!(diagnostics.iter().any(|diagnostic| matches!(
                diagnostic.kind,
                DiagnosticKind::PublicGenericFunction { .. }
            )));
            assert!(
                analysis
                    .result()
                    .facts()
                    .typed
                    .functions
                    .iter()
                    .all(|function| function.implementation == FunctionImplementation::Script)
            );
        }
    }
}

#[test]
fn inherent_method_selection_checks_receiver_owner_before_same_named_members() {
    let text = "struct First {} struct Second {} impl First { fn read(self)->i32 { 1 } } impl Second { fn read(self)->bool { true } } fn main()->i32 { First {}.read() }";
    let mut sources = SourceDatabase::default();
    let root = sources
        .set("method-owners.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let snapshot = test_analysis()
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    let analysis = snapshot.file(root).unwrap();
    assert!(
        analysis.result().diagnostics().is_empty(),
        "{:?}",
        analysis.result().diagnostics()
    );
    let authoring_facts = analysis.to_unverified(&Default::default()).unwrap();
    let facts = authoring_facts.facts();
    let (id, _) = facts
        .lowered
        .module
        .body
        .expressions()
        .find(|(_, expr)| matches!(expr.kind, ExprKind::Call { .. }))
        .unwrap();
    let CallTarget::Function(target) = facts.typed.type_table.call_resolution(id).unwrap().target
    else {
        panic!("local method")
    };
    let function = facts
        .typed
        .functions
        .iter()
        .find(|function| function.id == target)
        .unwrap();
    assert_eq!(function.implementation, FunctionImplementation::Script);
    assert_eq!(function.return_type, TypeId::Builtin(BuiltinType::I32));
}

#[test]
fn applied_signatures_preserve_parameter_contracts_through_coercion_and_divergence() {
    let text = r#"
use std::collections::List;
fn read(values: List<i32>) -> i32 { 42 }
fn consume(value: i32) -> i32 { value }
fn stop() -> ! { loop {} }
fn run(callback: fn(i32) -> bool) {
    read([1, 2]);
    callback(1);
    consume(stop());
}
"#;
    let mut sources = SourceDatabase::default();
    let root = sources
        .set("applied-contracts.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let snapshot = test_analysis()
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    let analysis = snapshot.file(root).unwrap();
    assert!(
        analysis.result().diagnostics().is_empty(),
        "{:?}",
        analysis.result().diagnostics()
    );
    let authoring_facts = analysis.to_unverified(&Default::default()).unwrap();
    let facts = authoring_facts.facts();
    let mut seen = Vec::new();
    for (site, expression) in facts.lowered.module.body.expressions() {
        let ExprKind::Call { callee, args, .. } = &expression.kind else {
            continue;
        };
        let ExprKind::Name { name, .. } = &facts.lowered.module.expr(*callee).kind else {
            continue;
        };
        if !matches!(name.as_str(), "read" | "consume" | "callback") {
            continue;
        }
        let call = facts.typed.type_table.call_resolution(site).unwrap();
        let signature = call.signature.unwrap();
        assert_eq!(signature.params.len(), 1);
        if name == "read" {
            assert!(matches!(&signature.params[0], TypeId::Trait(interface)
                if interface.arguments == [TypeId::Builtin(BuiltinType::I32)]));
            let coercion = facts.typed.type_table.interface_coercion(args[0]).unwrap();
            assert_eq!(
                signature.params[0],
                TypeId::Trait(coercion.interface_type.clone())
            );
        } else {
            assert_eq!(signature.params, [TypeId::Builtin(BuiltinType::I32)]);
        }
        if name == "consume" {
            assert!(
                facts
                    .typed
                    .type_table
                    .expr_type(args[0])
                    .unwrap()
                    .is_never()
            );
        }
        assert_eq!(
            signature.return_type,
            TypeId::Builtin(if name == "callback" {
                BuiltinType::Bool
            } else {
                BuiltinType::I32
            })
        );
        seen.push(name.as_str());
    }
    seen.sort_unstable();
    assert_eq!(seen, ["callback", "consume", "read"]);
}

#[test]
fn lexical_values_preserve_associated_native_and_script_type_owners() {
    for (owner, call, output) in [
        ("Vec", "Vec::new()", "Vec<i32>"),
        ("String", "String::from(\"value\")", "String"),
        ("Item", "Item::make()", "Item"),
    ] {
        let text = format!(
            "struct Item {{}} impl Item {{ pub fn make() -> Item {{ Item {{}} }} }} fn make({owner}: i32) -> {output} {{ {call} }}"
        );
        let analysis = crate::analyze_source(
            &SourceFile::new("shadow-owners.kgr", text),
            foundation_catalog::shared(),
        )
        .expect("installed declaration analysis");
        assert!(
            analysis.diagnostics().is_empty(),
            "{owner}: {:?}",
            analysis.diagnostics()
        );
        assert!(analysis.clone().into_codegen().is_ok(), "{owner}");
        let facts = analysis.facts();
        let (id, _) = facts
            .lowered
            .module
            .body
            .expressions()
            .find(|(_, expression)| matches!(expression.kind, ExprKind::Call { .. }))
            .unwrap();
        assert!(
            facts.typed.type_table.call_resolution(id).is_some(),
            "{owner}"
        );
    }
}
