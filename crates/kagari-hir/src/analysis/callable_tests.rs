use crate::{
    LanguageFeatureProfile,
    analysis::AnalysisDatabase,
    declarations::DeclarationId,
    hir::ExprKind,
    host::HostDeclarations,
    native::NativeBinding,
    typeck::{CallTarget, FunctionImplementation},
    types::TypeId,
};
use kagari_abi::{
    callable::EngineNativeBinding,
    scalar::BuiltinType,
    standard::{RuntimePrimitive, bindings::NativeDefaultMethod, surface::StandardEnum},
};
use kagari_common::{
    DiagnosticKind, SourceFile,
    host_interface::{
        HostFunctionDeclaration, HostInterface, HostParameter, HostPassingStyle, HostValueType,
    },
    integer::IntegerMethod,
    source_database::{SourceDatabase, SourceLayer},
};

#[test]
fn primitive_methods_and_associated_functions_keep_their_declared_scalar_owner() {
    let text = r#"
fn narrow() { (1i8).wrapping_add(2i8); i8::from_str_radix("7f", 16u32); }
fn wide() { (1u64).wrapping_add(2u64); u64::from_str_radix("ff", 16u32); }
"#;
    let mut sources = SourceDatabase::default();
    let root = sources
        .set("numeric-bindings.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let snapshot = AnalysisDatabase::default()
        .snapshot(sources.snapshot(), Default::default(), &Default::default())
        .unwrap();
    let analysis = snapshot.file(root).unwrap();
    assert!(
        analysis.result().diagnostics().is_empty(),
        "{:?}",
        analysis.result().diagnostics()
    );
    let facts = analysis.result().facts();
    let mut seen = 0;
    for function in &facts.lowered.module.functions {
        let expected = if function.name == "narrow" {
            BuiltinType::I8
        } else {
            BuiltinType::U64
        };
        let block = facts.lowered.module.block(function.body.unwrap());
        for statement in &block.statements {
            let crate::hir::StmtKind::Expr(expression) = facts.lowered.module.stmt(*statement).kind
            else {
                panic!("call statement")
            };
            let call = facts.typed.type_table.call_resolution(expression).unwrap();
            let CallTarget::SourceFunction(id) = &call.target else {
                panic!("checked source impl member")
            };
            let imported = facts.imported_functions.target(id).unwrap();
            assert_eq!(imported.declaration.module.path, ["numeric"]);
            assert!(call.type_arguments.is_empty());
            match imported.signature.implementation {
                FunctionImplementation::Native(NativeBinding::Entry(
                    EngineNativeBinding::Integer(IntegerMethod::WrappingAdd),
                )) => {
                    assert_eq!(imported.signature.params[0].ty, TypeId::Builtin(expected));
                    assert_eq!(
                        facts.typed.type_table.expr_type(expression),
                        Some(TypeId::Builtin(expected))
                    );
                }
                FunctionImplementation::Native(NativeBinding::Entry(
                    EngineNativeBinding::ParseRadix,
                )) => {
                    let TypeId::StandardEnum { args, .. } = &imported.signature.return_type else {
                        panic!("radix result")
                    };
                    assert_eq!(args[0], TypeId::Builtin(expected));
                }
                other => panic!("unexpected primitive implementation: {other:?}"),
            }
            let signature = analysis
                .call_signature_at(facts.lowered.source_map.expr_span(expression).start)
                .unwrap();
            assert_eq!(signature.declaration, imported.site.id);
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
            "trait Local { fn required(self)->i32; fn defaulted(self)->i32 { 42 } }".into(),
            SourceLayer::Base,
        )
        .unwrap();
    let snapshot = AnalysisDatabase::default()
        .snapshot(sources.snapshot(), Default::default(), &Default::default())
        .unwrap();
    let facts = snapshot.file(root).unwrap().result().facts();
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
        .find(|item| item.declaration.name == "Iterator")
        .unwrap();
    let file = snapshot.file(iterator.declaration.location.file).unwrap();
    for (name, expected) in [
        ("next", FunctionImplementation::Required),
        (
            "map",
            FunctionImplementation::Native(NativeBinding::Entry(
                EngineNativeBinding::TraitDefault(NativeDefaultMethod::Map),
            )),
        ),
    ] {
        let method = iterator
            .methods
            .iter()
            .find(|method| method.name == name)
            .unwrap();
        let function = file
            .result()
            .facts()
            .typed
            .functions
            .iter()
            .find(|function| {
                file.result()
                    .facts()
                    .declarations
                    .definition(crate::resolver::ResolvedName::Function(function.id))
                    == Some(&method.id)
            })
            .unwrap();
        assert_eq!(function.implementation, expected);
    }
}

#[test]
fn native_generic_calls_use_source_signatures_and_ordinary_inference() {
    let text = r#"
use std::math::clamp as bound;
fn identity<T>(value: T) -> T { value }
fn main() -> i32 {
    val value = bound(identity(42), 0, 100);
    val optional: Option<i32> = Some(value);
    optional.unwrap_or(0)
}
"#;
    let mut sources = SourceDatabase::default();
    let root = sources
        .set("callables.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let snapshot = AnalysisDatabase::default()
        .snapshot(sources.snapshot(), Default::default(), &Default::default())
        .unwrap();
    let analysis = snapshot.file(root).unwrap();
    assert!(
        analysis.result().diagnostics().is_empty(),
        "{:?}",
        analysis.result().diagnostics()
    );
    let facts = analysis.result().facts();
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
            "bound" => FunctionImplementation::Native(NativeBinding::Entry(
                EngineNativeBinding::Intrinsic(RuntimePrimitive::MathClamp),
            )),
            "unwrap_or" => FunctionImplementation::Native(NativeBinding::Entry(
                EngineNativeBinding::Intrinsic(RuntimePrimitive::OptionUnwrapOr),
            )),
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
                assert_eq!(imported.declaration.module.package.0, "kagari-std");
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
            "unwrap_or" => vec![
                TypeId::StandardEnum {
                    kind: StandardEnum::Option,
                    args: vec![scalar.clone()],
                },
                scalar,
            ],
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
    assert_eq!(seen, ["bound", "identity", "unwrap_or"]);
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
    let mut database = AnalysisDatabase::default();
    database.set_host_declarations(
        HostDeclarations::new(HostInterface {
            paths: Vec::new(),
            types: Vec::new(),
            functions: vec![host.clone()],
        })
        .unwrap(),
    );
    let snapshot = database
        .snapshot(
            sources.snapshot(),
            LanguageFeatureProfile {
                allow_host_calls: true,
                ..Default::default()
            },
            &Default::default(),
        )
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
    let facts = file.result().facts();
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
            "use std::math::clamp as bound; fn main() { bound(true, false, true); }",
            true,
        ),
        (
            "#[intrinsic(MathClamp)] pub fn bound<T>(value:T)->T { value } fn main() { bound(1); }",
            false,
        ),
    ] {
        let mut sources = SourceDatabase::default();
        let root = sources
            .set("kagari://std/math.kgr", text.into(), SourceLayer::Base)
            .unwrap();
        let snapshot = AnalysisDatabase::default()
            .snapshot(sources.snapshot(), Default::default(), &Default::default())
            .unwrap();
        let analysis = snapshot.file(root).unwrap();
        let diagnostics = analysis.result().diagnostics();
        if native {
            assert!(
                diagnostics.iter().any(|diagnostic| matches!(
                    diagnostic.kind,
                    DiagnosticKind::StandardConstraintNotSatisfied { .. }
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
    let snapshot = AnalysisDatabase::default()
        .snapshot(sources.snapshot(), Default::default(), &Default::default())
        .unwrap();
    let analysis = snapshot.file(root).unwrap();
    assert!(
        analysis.result().diagnostics().is_empty(),
        "{:?}",
        analysis.result().diagnostics()
    );
    let facts = analysis.result().facts();
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
use std::array::List;
use std::debug::panic;
fn read(values: List<i32>) -> i32 { 42 }
fn consume(value: i32) -> i32 { value }
fn stop() -> ! { panic("stop") }
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
    let snapshot = AnalysisDatabase::default()
        .snapshot(sources.snapshot(), Default::default(), &Default::default())
        .unwrap();
    let analysis = snapshot.file(root).unwrap();
    assert!(
        analysis.result().diagnostics().is_empty(),
        "{:?}",
        analysis.result().diagnostics()
    );
    let facts = analysis.result().facts();
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
fn lexical_values_shadow_associated_native_and_script_owners() {
    for (owner, call) in [
        ("ArrayList", "ArrayList::new()"),
        ("String", "String::from(\"value\")"),
        ("Item", "Item::make()"),
    ] {
        let text = format!(
            "struct Item {{}} impl Item {{ pub fn make() -> Item {{ Item {{}} }} }} fn bad({owner}: i32) {{ {call}; }}"
        );
        let analysis = crate::analyze_source(
            &SourceFile::new("shadow-owners.kgr", text),
            Default::default(),
        );
        assert!(analysis.clone().into_codegen().is_err(), "{owner}");
        let facts = analysis.facts();
        let (id, _) = facts
            .lowered
            .module
            .body
            .expressions()
            .find(|(_, expression)| matches!(expression.kind, ExprKind::Call { .. }))
            .unwrap();
        assert!(
            facts.typed.type_table.call_resolution(id).is_none(),
            "{owner}"
        );
    }
}
