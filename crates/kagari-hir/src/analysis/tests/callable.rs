use crate::tests::standard::standard_modules;
use crate::{
    hir::expr::ExprKind,
    resolver::resolved::ResolvedName,
    semantic::signatures::FunctionImplementation,
    tests::{native as fixture, test_analysis},
    typeck::table::CallTarget,
    types::TypeId,
};
use kagari_source::{
    diagnostic::DiagnosticKind,
    source::SourceFile,
    source_database::{SourceDatabase, SourceLayer},
};
use kagari_types::scalar::BuiltinType;

#[test]
fn call_type_arguments_reject_top_level_associated_bindings() {
    for text in [
        "fn identity<T>(value: T) -> T { value } fn main() -> i32 { identity::<i32, Item = DoesNotExist>(42) }",
        "fn main() -> i32 { fn identity<T>(value: T) -> T { value } identity::<Item = i32, i32>(42) }",
        "struct Relay {} impl Relay { fn identity<T>(self, value: T) -> T { value } } fn main() -> i32 { Relay {}.identity::<i32, Item = i32>(42) }",
    ] {
        let source = SourceFile::new("named-call-arguments.kgr", text);
        let analysis = crate::analyze_source(&source, standard_modules())
            .expect("installed declaration analysis");
        let diagnostic = analysis
            .diagnostics()
            .iter()
            .find(|diagnostic| {
                matches!(
                    diagnostic.kind,
                    DiagnosticKind::UnsupportedSyntax {
                        feature: "associated type bindings in call arguments"
                    }
                )
            })
            .unwrap_or_else(|| panic!("{text}: {:?}", analysis.diagnostics()));
        let span = diagnostic.span.expect("written binding site");
        assert_eq!(&text[span.start..span.end], "Item");
        assert!(analysis.into_codegen().is_err(), "{text}");
    }
}

/* Temporarily paused: stdlib restoration (NC01-NC06).
#[test]
fn call_type_arguments_keep_nested_associated_bindings() {
    let source = SourceFile::new(
        "nested-call-arguments.kgr",
        r#"
fn identity<T>(value: T) -> T { value }
fn preserve(iter: Iterator<Item = i32>) -> Iterator<Item = i32> {
    identity::<Iterator<Item = i32>>(iter)
}
fn sum<I: Iterator<Item = i32>>(iter: I) -> i32 { iter.sum() }
fn main() -> i32 { sum([20, 22].iter()) }
"#,
    );
    let analysis = crate::analyze_source(&source, standard_modules())
        .expect("installed declaration analysis");
    assert!(
        analysis.diagnostics().is_empty(),
        "{:?}",
        analysis.diagnostics()
    );
    assert!(analysis.into_codegen().is_ok());
}
*/

/* Temporarily paused: stdlib restoration (NC01-NC06).
#[test]
fn async_callable_typing_contract() {
    let context = DefinitionContext::new().unwrap();
    let text = r#"
async fn identity<T>(value: T) -> T { return value; }
async fn nested() -> Future<i32> { identity(1) }
async fn task_result<T>(task: Task<T>) -> T { task.await }
async fn nested_task(task: Task<Future<i32>>) -> Future<i32> { task.await }
async fn empty<T>() -> Vec<T> { Vec::from([]) }
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
fn block<T>(value: T) -> Future<T> { async { return value; } }
fn block_unit() -> Future<()> { async {} }
fn block_inferred() -> Future<i32> { val pending = async { return 42; }; pending }
fn block_context() -> Future<Vec<i64>> { async { Vec::from([]) } }
fn block_nested() -> Future<Future<i32>> { async { identity(42) } }
fn block_propagate(value: Result<i32, i32>) -> Future<Result<i32, i32>> {
    async { Ok(identity(value?).await) }
}
fn block_option(value: Option<i32>) -> Future<Option<i32>> { async { Some(value?) } }
fn block_capture() -> Future<i32> {
    var x = 1;
    val make = || async { x += 1; val result = loop { break x; }; result };
    make()
}
async fn block_boundaries() -> i32 {
    val pending = async { val f = || { return 40; }; return f() + 2; };
    pending.await
}
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
        .signatures
        .functions()
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
            "fn bad() { loop { val pending = async { break; }; } }",
            "KG_TYPE_BREAK_OUTSIDE_LOOP",
        ),
        (
            "fn bad() { loop { val pending = async { continue; }; } }",
            "KG_TYPE_CONTINUE_OUTSIDE_LOOP",
        ),
        (
            "fn bad() { val pending = async { val inner = || async { 1 }.await; }; }",
            "KG_AWAIT_OUTSIDE_ASYNC",
        ),
        (
            "fn bad() -> i32 { async { 42 }.await }",
            "KG_AWAIT_OUTSIDE_ASYNC",
        ),
        (
            "fn bad() -> Future<i32> { async { return true; } }",
            "KG_TYPE_RETURN_TYPE_MISMATCH",
        ),
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
    let mut host = ModuleDecl::new(ModuleIdentity {
        package: PackageId("host".into()),
        path: vec![],
    });
    host.types.push(TypeDef {
        name: "State".into(),
        kind: TypeDefKind::Host(HostTypePolicy {
            ownership: HostTypeOwnership::HostRoot,
            reflection: HostReflectionPolicy::Hidden,
            path_access: PathAccess::None,
        }),
        generic_params: vec![],
        bounds: vec![],
        fields: vec![],
        variants: vec![],
    });
    host.collect_definitions(&context).unwrap();
    let mut modules = standard_modules();
    modules.push(Arc::new(
        DefinitionMetadata::checked(context.snapshot(), host, &Default::default()).unwrap(),
    ));
    database.set_native_modules(modules);
    for text in [
        "async fn bad(value:host::State) {}",
        "async fn bad(value:(i32,host::State)) {}",
        "fn bad() { val f=async |value:host::State| {}; }",
        "fn bad() { val f:fn(host::State)->Future<()> = async |value| {}; }",
        "fn bad(value:host::State) { val f=async { value; }; }",
    ] {
        sources
            .set("async-types.kgr", text.into(), SourceLayer::Base)
            .unwrap();
        let snapshot = database
            .snapshot(sources.snapshot(), &Default::default())
            .unwrap();
        let analysis = snapshot.file(root).unwrap();
        assert!(
            analysis.result().diagnostics().iter().any(|diagnostic| {
                if text.starts_with("fn bad(value:") {
                    matches!(
                        diagnostic.kind,
                        DiagnosticKind::InvalidClosureCapture { .. }
                    )
                } else {
                    matches!(diagnostic.kind, DiagnosticKind::InvalidAsyncCapture { .. })
                }
            }),
            "{text}: {:?}",
            analysis.result().diagnostics()
        );
    }
}
*/

#[test]
fn required_script_and_native_methods_keep_distinct_signature_implementations() {
    let mut sources = SourceDatabase::default();
    let root = sources
        .set(
            "implementations.kgr",
            "use demo::bindings::NativeRead; trait Local { fn required(self)->i32; fn defaulted(self)->i32 { 42 } }".into(),
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
    let facts = authoring_facts.records().facts();
    for (name, expected) in [
        ("required", FunctionImplementation::Required),
        ("defaulted", FunctionImplementation::Script),
    ] {
        assert_eq!(
            facts
                .typed
                .signatures
                .functions()
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
    assert!(iterator.declaration.source_location().is_none());
    let owner = authoring_facts
        .definitions()
        .resolve(iterator.id)
        .unwrap()
        .module()
        .clone();
    let file = snapshot
        .files()
        .find(|candidate| candidate.source().module_identity() == &owner)
        .unwrap();
    let authoring = file.to_unverified(&Default::default()).unwrap();
    for name in ["read", "fixed"] {
        let expected = FunctionImplementation::Script;
        let method = iterator
            .methods
            .iter()
            .find(|method| method.name == name)
            .unwrap();
        let function = authoring
            .records()
            .facts()
            .typed
            .signatures
            .functions()
            .iter()
            .find(|function| {
                authoring
                    .records()
                    .facts()
                    .declarations
                    .definition(ResolvedName::Function(function.id))
                    == Some(&method.id)
            })
            .unwrap();
        assert_eq!(function.implementation, expected);
        let lowered = &authoring.records().facts().lowered;
        assert!(
            lowered.module.functions[function.id.index()]
                .implementation
                .script_body()
                .is_some()
        );
        assert!(
            authoring
                .records()
                .facts()
                .typed
                .type_table()
                .call_resolution(
                    lowered
                        .module
                        .block(
                            lowered.module.functions[function.id.index()]
                                .implementation
                                .script_body()
                                .unwrap()
                        )
                        .tail_expr
                        .unwrap()
                )
                .is_some()
        );
    }
}

#[test]
fn native_generic_bounds_and_authority_do_not_depend_on_script_visibility() {
    for (text, native) in [
        (
            "use demo::bindings::choose as bound; fn main() { bound(1.0, 2.0, 3.0); }",
            true,
        ),
        (
            "#[uninstalled(fake)] pub fn bound<T>(value:T)->T { value } fn main() { bound(1); }",
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
        } else {
            let unknown = diagnostics
                .iter()
                .filter(|diagnostic| {
                    matches!(
                        &diagnostic.kind,
                        DiagnosticKind::UnknownAttribute { name } if name == "uninstalled"
                    )
                })
                .collect::<Vec<_>>();
            let [diagnostic] = unknown.as_slice() else {
                panic!("one unknown binding attribute expected: {diagnostics:?}");
            };
            let span = diagnostic.span.expect("authored attribute range");
            assert_eq!(&text[span.start..span.end], "#[uninstalled(fake)]");
            assert!(
                analysis
                    .result()
                    .facts()
                    .typed
                    .signatures
                    .functions()
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
    let facts = authoring_facts.records().facts();
    let (id, _) = facts
        .lowered
        .module
        .nodes
        .expressions()
        .find(|(_, expr)| matches!(expr.kind, ExprKind::Call { .. }))
        .unwrap();
    let CallTarget::Function(target) = facts.typed.type_table.call_resolution(id).unwrap().target
    else {
        panic!("local method")
    };
    let function = facts
        .typed
        .signatures
        .functions()
        .iter()
        .find(|function| function.id == target)
        .unwrap();
    assert_eq!(function.implementation, FunctionImplementation::Script);
    assert_eq!(function.return_type, TypeId::Builtin(BuiltinType::I32));
}

/* Temporarily paused: stdlib restoration (NC01-NC06).
#[test]
fn applied_signatures_preserve_parameter_contracts_through_coercion_and_divergence() {
    let text = r#"
use std::collections::List;
fn read(values: List<i32>) -> i32 { 42 }
fn consume(value: i32) -> i32 { value }
fn stop() -> ! { loop {} }
fn run(callback: fn(i32) -> bool) {
    read(Vec::from([1, 2]));
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
    let facts = authoring_facts.records().facts();
    let mut seen = Vec::new();
    for (site, expression) in facts.lowered.module.nodes.expressions() {
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
*/

/* Temporarily paused: stdlib restoration (NC01-NC06).
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
            standard_modules(),
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
            .nodes
            .expressions()
            .find(|(_, expression)| matches!(expression.kind, ExprKind::Call { .. }))
            .unwrap();
        assert!(
            facts.typed.type_table.call_resolution(id).is_some(),
            "{owner}"
        );
    }
}
*/
