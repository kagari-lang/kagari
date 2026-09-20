use kagari_common::{
    SourceFile,
    host_interface::{
        HostFieldDeclaration, HostFunctionDeclaration, HostInterface, HostParameter,
        HostPassingStyle, HostTypeDeclaration, HostTypeOwnership, HostValueType,
    },
};
use kagari_embed::{
    ArtifactOptions, CompileOptions, ExecutionContext, HostExposurePolicy, KagariEngine,
};
use kagari_runtime::{
    CapabilitySet, LanguageProfile,
    host::{HostFunction, HostObjectId, HostSchemaEpoch, HostTypeRegistration},
    value::Value,
};
use std::{cell::RefCell, rc::Rc};

fn interface() -> HostInterface {
    let related = HostTypeDeclaration::new("right.Item");
    let mut item = HostTypeDeclaration::new("left.Item");
    item.ownership = HostTypeOwnership::HostRoot;
    item.path_access = kagari_common::host_interface::PathAccess::ReadOnly;
    item.fields.push(HostFieldDeclaration::new(
        &item.id,
        "related",
        HostValueType::Opaque(related.id.clone()),
    ));
    let make =
        HostFunctionDeclaration::new("left.make", vec![], HostValueType::Opaque(item.id.clone()));
    let take = HostFunctionDeclaration::new(
        "left.take",
        vec![HostParameter {
            name: "value".into(),
            ty: HostValueType::Opaque(item.id.clone()),
            passing: HostPassingStyle::SharedBorrow,
        }],
        HostValueType::I32,
    );
    HostInterface {
        types: vec![item, related, HostTypeDeclaration::new("unused.Other")],
        functions: vec![make, take],
    }
}

#[test]
fn declared_methods_link_by_identity_and_evaluate_receiver_then_arguments_once() {
    use kagari_common::host_interface::HostMethodDeclaration;
    let mut interface = interface();
    let mut method = HostMethodDeclaration::new(
        &interface.types[0].id,
        "add",
        vec![HostParameter {
            name: "amount".into(),
            ty: HostValueType::I32,
            passing: HostPassingStyle::Owned,
        }],
        HostValueType::I32,
    );
    method.receiver = HostPassingStyle::UniqueBorrow;
    method.effects.may_mutate_host_state = true;
    method.capability_requirements.fs_write = true;
    let method_id = method.id.clone();
    interface.types[0].methods.push(method);
    let rhs = HostFunctionDeclaration::new("left.rhs", vec![], HostValueType::I32);
    interface.functions.push(rhs.clone());
    let engine = KagariEngine::default();
    engine.set_host_interface(interface.clone()).unwrap();
    let profile = LanguageProfile {
        allow_host_calls: true,
        allow_jit: true,
        ..Default::default()
    };
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new(
                "method.kgr",
                "use left as api; fn main() -> i32 { api::make().add(api::rhs()) }",
            ),
            CompileOptions {
                language_profile: profile,
            },
            Default::default(),
        )
        .unwrap();
    let required = &artifact.program.modules[artifact.program.root.index()].host_interface;
    let call = required
        .functions
        .iter()
        .find(|f| f.id == method_id)
        .unwrap();
    assert_eq!(
        call,
        &interface.types[0].method_contract(&method_id).unwrap()
    );
    for (encoded, jit) in [(false, false), (true, false), (true, true)] {
        let artifact = if encoded {
            kagari_ir::bytecode::KbcArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap()
        } else {
            artifact.clone()
        };
        let context = ExecutionContext {
            language_profile: profile,
            capabilities: CapabilitySet {
                host_calls: true,
                jit: true,
                fs_write: true,
                ..Default::default()
            },
            host_policy: HostExposurePolicy {
                allowed_host_functions: vec![
                    "left.make".into(),
                    "left.rhs".into(),
                    "left.Item.add".into(),
                ],
                ..Default::default()
            },
            jit_policy: if jit {
                kagari_embed::JitPolicy::Enabled
            } else {
                kagari_embed::JitPolicy::Disabled
            },
            ..Default::default()
        };
        let mut runtime = engine.runtime(context.clone());
        let types = runtime
            .register_host_types(
                interface.types[..2]
                    .iter()
                    .cloned()
                    .map(|ty| HostTypeRegistration::new(ty, "Object"))
                    .collect(),
            )
            .unwrap();
        let root = runtime
            .runtime_mut()
            .register_host_root(HostObjectId(8), types[0], HostSchemaEpoch::new(0))
            .unwrap();
        let trace = Rc::new(RefCell::new(Vec::new()));
        let calls = trace.clone();
        runtime
            .register_host_function(HostFunction::new(
                interface.functions[0].clone(),
                move |_, _| {
                    calls.borrow_mut().push("receiver");
                    Ok(Value::HostRoot(root))
                },
            ))
            .unwrap();
        let calls = trace.clone();
        runtime
            .register_host_function(HostFunction::new(rhs.clone(), move |_, _| {
                calls.borrow_mut().push("argument");
                Ok(Value::I32(2))
            }))
            .unwrap();
        assert!(
            runtime
                .load_program(artifact.clone(), Default::default())
                .is_err()
        );
        assert!(trace.borrow().is_empty());
        let mut wrong = interface.types[0].method_contract(&method_id).unwrap();
        wrong.params[0].passing = HostPassingStyle::Owned;
        assert!(
            runtime
                .register_host_function(HostFunction::new(wrong, |_, _| panic!(
                    "invalid member binding"
                )))
                .is_err()
        );
        let total = Rc::new(std::cell::Cell::new(40));
        let state = total.clone();
        let calls = trace.clone();
        runtime
            .register_host_function(
                HostFunction::method(&interface.types[0], &method_id, move |context, args| {
                    calls.borrow_mut().push("method");
                    assert!(
                        context
                            .runtime()
                            .invoke_host("left.Item.add", args)
                            .is_err(),
                        "the receiver's exclusive lease prevents recursive access"
                    );
                    let Value::I32(amount) = args[1] else {
                        panic!("checked parameter")
                    };
                    state.set(state.get() + amount);
                    context.runtime().collect_garbage().unwrap();
                    Ok(Value::I32(state.get()))
                })
                .unwrap(),
            )
            .unwrap();
        let loaded = runtime.load_program(artifact, Default::default()).unwrap();
        let mut denied = context.clone();
        denied.jit_policy = kagari_embed::JitPolicy::Disabled;
        denied.capabilities.fs_write = false;
        assert!(runtime.execute(&loaded, "main", &[], &denied).is_err());
        assert_eq!(total.get(), 40);
        assert_eq!(*trace.borrow(), ["receiver", "argument"]);
        trace.borrow_mut().clear();
        let mut backend = jit.then(|| kagari_jit_cranelift::CraneliftBackend::for_host().unwrap());
        for expected in [42, 44] {
            let result = if let Some(backend) = &mut backend {
                runtime.execute_with_backend(&loaded, "main", &[], &context, backend)
            } else {
                runtime.execute(&loaded, "main", &[], &context)
            }
            .unwrap();
            assert_eq!(result.return_value, Value::I32(expected));
        }
        assert_eq!(
            *trace.borrow(),
            [
                "receiver", "argument", "method", "receiver", "argument", "method"
            ]
        );
        assert_eq!(runtime.runtime().gc().active_roots(), 0);
    }
}

#[test]
fn source_host_handles_link_offline_contracts_and_execute_across_backends() {
    let interface = interface();
    let engine = KagariEngine::default();
    engine
        .set_host_interface(HostInterface::from_bytes(&interface.to_bytes().unwrap()).unwrap())
        .unwrap();
    let profile = LanguageProfile {
        allow_host_calls: true,
        allow_jit: true,
        ..Default::default()
    };
    let artifact = engine.compile_to_artifact(
        SourceFile::new("nominal.kgr", "use left::Item; use left as api; pub fn pass(value: Item) -> api::Item { value } fn id<T>(value: T) -> T { value } fn main() -> i32 { val value: Item = api::make(); api::take(pass(id(value))) }"),
        CompileOptions { language_profile: profile }, ArtifactOptions::default(),
    ).unwrap();
    let module = &artifact.program.modules[artifact.program.root.index()];
    assert_eq!(module.host_interface.types, interface.types[..2]);
    let kagari_ir::module::PublicAbiItem::Function(pass) = &module.public_items[0] else {
        panic!("public pass")
    };
    assert_eq!(
        pass.return_type,
        kagari_ir::module::abi::AbiType::Host(interface.types[0].id.clone())
    );
    assert_eq!(
        pass.return_type.representation(),
        kagari_ir::module::ValueType::HostHandle
    );
    let mut invalid = artifact.program.clone();
    invalid.modules[artifact.program.root.index()]
        .host_interface
        .types
        .clear();
    assert!(kagari_ir::bytecode::KbcArtifact::from_program(invalid, Default::default()).is_err());
    for (encoded, jit) in [(false, false), (true, false), (true, true)] {
        let artifact = if encoded {
            kagari_ir::bytecode::KbcArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap()
        } else {
            artifact.clone()
        };
        let context = ExecutionContext {
            language_profile: profile,
            capabilities: CapabilitySet {
                host_calls: true,
                jit: true,
                ..Default::default()
            },
            host_policy: HostExposurePolicy {
                allowed_host_functions: vec!["left.make".into(), "left.take".into()],
                ..Default::default()
            },
            jit_policy: if jit {
                kagari_embed::JitPolicy::Enabled
            } else {
                kagari_embed::JitPolicy::Disabled
            },
            ..Default::default()
        };
        let mut runtime = engine.runtime(context.clone());
        let ids = runtime
            .register_host_types(
                interface.types[..2]
                    .iter()
                    .cloned()
                    .map(|ty| HostTypeRegistration::new(ty, "Object"))
                    .collect(),
            )
            .unwrap();
        let root = runtime
            .runtime_mut()
            .register_host_root(HostObjectId(7), ids[0], HostSchemaEpoch::new(0))
            .unwrap();
        let trace = Rc::new(RefCell::new(Vec::new()));
        let calls = trace.clone();
        runtime
            .register_host_function(HostFunction::new(
                interface.functions[0].clone(),
                move |_, _| {
                    calls.borrow_mut().push("make");
                    Ok(Value::HostRoot(root))
                },
            ))
            .unwrap();
        let calls = trace.clone();
        runtime
            .register_host_function(HostFunction::new(
                interface.functions[1].clone(),
                move |context, args| {
                    calls.borrow_mut().push("take");
                    assert!(matches!(args[0], Value::HostRoot(_)));
                    context.runtime().collect_garbage().unwrap();
                    Ok(Value::I32(42))
                },
            ))
            .unwrap();
        let loaded = runtime.load_program(artifact, Default::default()).unwrap();
        assert!(trace.borrow().is_empty());
        let report = if jit {
            let mut backend = kagari_jit_cranelift::CraneliftBackend::for_host().unwrap();
            runtime.execute_with_backend(&loaded, "main", &[], &context, &mut backend)
        } else {
            runtime.execute(&loaded, "main", &[], &context)
        }
        .unwrap();
        assert_eq!(report.return_value, Value::I32(42));
        assert_eq!(*trace.borrow(), ["make", "take"]);
        assert_eq!(runtime.runtime().gc().active_roots(), 0);
    }
}

#[test]
fn annotation_only_host_dependencies_are_verified_and_linked() {
    let engine = KagariEngine::default();
    let interface = interface();
    engine.set_host_interface(interface.clone()).unwrap();
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new(
                "annotation.kgr",
                "pub fn pass(value: left::Item) -> left::Item { value } fn main() -> i32 { 7 }",
            ),
            Default::default(),
            Default::default(),
        )
        .unwrap();
    let module = &artifact.program.modules[artifact.program.root.index()];
    assert!(module.host_interface.functions.is_empty());
    assert_eq!(module.host_interface.types.len(), 2);
    let mut runtime = engine.runtime(Default::default());
    assert!(
        runtime
            .load_program(artifact.clone(), Default::default())
            .is_err()
    );
    runtime
        .register_host_types(
            interface.types[..2]
                .iter()
                .cloned()
                .map(|ty| HostTypeRegistration::new(ty, "Object"))
                .collect(),
        )
        .unwrap();
    let loaded = runtime
        .load_program(artifact.clone(), Default::default())
        .unwrap();
    assert_eq!(
        runtime
            .execute(&loaded, "main", &[], &Default::default())
            .unwrap()
            .return_value,
        Value::I32(7)
    );
    let mut invalid = artifact.program;
    invalid.modules[invalid.root.index()]
        .host_interface
        .types
        .clear();
    assert!(kagari_ir::bytecode::verify_program(&invalid).is_err());
}
