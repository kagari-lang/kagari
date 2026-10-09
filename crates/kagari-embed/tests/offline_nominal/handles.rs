use super::*;
use kagari_bytecode::{artifact::KbcArtifact, program::verify_program};
use kagari_contract::representation::semantic_representation;
use kagari_embed::{context::JitPolicy, program::PreparedProgram};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicI32, Ordering},
};

#[test]
fn offline_host_type_navigation_is_available_from_signature_query() {
    let engine = KagariEngine::default();
    engine.set_host_interface(interface()).unwrap();
    let text = "// 中文 😀\r\nuse left::Item; fn accept(value: Item) -> Item { value }";
    let file = engine
        .set_source("mem://host-signature", text.into(), SourceLayer::Base)
        .unwrap();
    let signatures = engine
        .signatures(engine.source_snapshot(), &Default::default())
        .unwrap();
    let signature = signatures.file(file).unwrap();
    let annotation = text.find("value: Item").unwrap() + "value: ".len();
    assert_eq!(
        signature.host_type_at(annotation).unwrap().symbol,
        "left.Item"
    );
    assert!(signature.definition_at(annotation).is_none());
    assert!(signature.host_type_at(annotation - 1).is_none());
    assert!(signature.diagnostics().is_empty());

    let full = engine
        .analyze(engine.source_snapshot(), &Default::default())
        .unwrap();
    assert_eq!(
        full.file(file).unwrap().host_type_at(annotation),
        signature.host_type_at(annotation)
    );
}

#[test]
fn declared_methods_link_by_identity_and_evaluate_receiver_then_arguments_once() {
    use kagari_types::host_interface::type_declaration::HostMethodDeclaration;
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

    let method_id = method.id.clone();
    interface.types[0].methods.push(method);
    let rhs = HostFunctionDeclaration::new("left.rhs", vec![], HostValueType::I32);
    interface.functions.push(rhs.clone());
    let engine = KagariEngine::default();
    engine.set_host_interface(interface.clone()).unwrap();

    let artifact = engine
        .compile_to_artifact(
            SourceFile::new(
                "method.kgr",
                "use left as api; fn main() -> i32 { api::make().add(api::rhs()) }",
            ),
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
            KbcArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap()
        } else {
            artifact.clone()
        };
        let context = ExecutionContext {
            jit_policy: if jit {
                JitPolicy::Enabled
            } else {
                JitPolicy::Disabled
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
        let trace = Arc::new(Mutex::new(Vec::new()));
        let calls = trace.clone();
        runtime
            .register_host_function(HostFunction::new(
                interface.functions[0].clone(),
                move |cx, _| {
                    calls.lock().unwrap().push("receiver");
                    Ok(cx.runtime().gc().alloc_host_root(root).unwrap())
                },
            ))
            .unwrap();
        let calls = trace.clone();
        runtime
            .register_host_function(HostFunction::new(rhs.clone(), move |_, _| {
                calls.lock().unwrap().push("argument");
                Ok(Value::I32(2))
            }))
            .unwrap();
        assert!(
            runtime
                .load_program(
                    &PreparedProgram::from_artifact(
                        artifact.clone(),
                        &Default::default(),
                        &Default::default()
                    )
                    .unwrap(),
                    Default::default()
                )
                .is_err()
        );
        assert!(trace.lock().unwrap().is_empty());
        let mut wrong = interface.types[0].method_contract(&method_id).unwrap();
        wrong.params[0].passing = HostPassingStyle::Owned;
        assert!(
            runtime
                .register_host_function(HostFunction::new(wrong, |_, _| panic!(
                    "invalid member binding"
                )))
                .is_err()
        );
        let total = Arc::new(AtomicI32::new(40));
        let state = total.clone();
        let calls = trace.clone();
        runtime
            .register_host_function(
                HostFunction::method(&interface.types[0], &method_id, move |context, args| {
                    calls.lock().unwrap().push("method");
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
                    state.fetch_add(amount, Ordering::SeqCst);
                    context.runtime().collect_garbage().unwrap();
                    Ok(Value::I32(state.load(Ordering::SeqCst)))
                })
                .unwrap(),
            )
            .unwrap();
        let loaded_program =
            PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default())
                .unwrap();
        let loaded = runtime
            .load_program(&loaded_program, Default::default())
            .unwrap();
        let mut backend =
            jit.then(|| kagari_codegen_cranelift::CraneliftBackend::for_host().unwrap());
        for expected in [42, 44] {
            let result = if let Some(backend) = &mut backend {
                let prepared = runtime
                    .prepare_native(
                        &loaded_program,
                        &loaded,
                        "main",
                        backend,
                        &context.cancellation,
                    )
                    .unwrap();
                runtime.execute_prepared(&loaded, "main", &[], &context, &prepared)
            } else {
                runtime.execute(&loaded, "main", &[], &context)
            }
            .unwrap();
            assert_eq!(
                result
                    .return_value
                    .value(runtime.runtime().gc())
                    .expect("retained execution result"),
                Value::I32(expected)
            );
        }
        assert_eq!(
            *trace.lock().unwrap(),
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

    let artifact = engine.compile_to_artifact(
        SourceFile::new("nominal.kgr", "use left::Item; use left as api; pub fn pass(value: Item) -> api::Item { value } fn id<T>(value: T) -> T { value } fn main() -> i32 { val value: Item = api::make(); api::take(pass(id(value))) }"),
         ArtifactOptions::default(),
    ).unwrap();
    let module = &artifact.program.modules[artifact.program.root.index()];
    assert_eq!(module.host_interface.types, interface.types[..2]);
    let kagari_contract::types::PublicItem::Function(pass) = &module.public_items[0] else {
        panic!("public pass")
    };
    assert_eq!(
        pass.return_type,
        kagari_types::ty::Ty::Host(interface.types[0].id.clone())
    );
    assert_eq!(
        semantic_representation(&pass.return_type),
        kagari_abi::representation::ValueType::HostHandle
    );
    let mut invalid = artifact.program.clone();
    invalid.modules[artifact.program.root.index()]
        .host_interface
        .types
        .clear();
    assert!(KbcArtifact::from_program(invalid, Default::default()).is_err());
    for (encoded, jit) in [(false, false), (true, false), (true, true)] {
        let artifact = if encoded {
            KbcArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap()
        } else {
            artifact.clone()
        };
        let context = ExecutionContext {
            jit_policy: if jit {
                JitPolicy::Enabled
            } else {
                JitPolicy::Disabled
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
        let trace = Arc::new(Mutex::new(Vec::new()));
        let calls = trace.clone();
        runtime
            .register_host_function(HostFunction::new(
                interface.functions[0].clone(),
                move |cx, _| {
                    calls.lock().unwrap().push("make");
                    Ok(cx.runtime().gc().alloc_host_root(root).unwrap())
                },
            ))
            .unwrap();
        let calls = trace.clone();
        runtime
            .register_host_function(HostFunction::new(
                interface.functions[1].clone(),
                move |context, args| {
                    calls.lock().unwrap().push("take");
                    assert!(matches!(args[0], Value::HostRoot(_)));
                    context.runtime().collect_garbage().unwrap();
                    Ok(Value::I32(42))
                },
            ))
            .unwrap();
        let loaded_program =
            PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default())
                .unwrap();
        let loaded = runtime
            .load_program(&loaded_program, Default::default())
            .unwrap();
        assert!(trace.lock().unwrap().is_empty());
        let report = if jit {
            let mut backend = kagari_codegen_cranelift::CraneliftBackend::for_host().unwrap();
            let prepared = runtime
                .prepare_native(
                    &loaded_program,
                    &loaded,
                    "main",
                    &mut backend,
                    &context.cancellation,
                )
                .unwrap();
            runtime.execute_prepared(&loaded, "main", &[], &context, &prepared)
        } else {
            runtime.execute(&loaded, "main", &[], &context)
        }
        .unwrap();
        assert_eq!(
            report
                .return_value
                .value(runtime.runtime().gc())
                .expect("retained execution result"),
            Value::I32(42)
        );
        assert_eq!(*trace.lock().unwrap(), ["make", "take"]);
        drop(report);
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
        )
        .unwrap();
    let module = &artifact.program.modules[artifact.program.root.index()];
    assert!(module.host_interface.functions.is_empty());
    assert_eq!(module.host_interface.types.len(), 2);
    let mut runtime = engine.runtime(Default::default());
    assert!(
        runtime
            .load_program(
                &PreparedProgram::from_artifact(
                    artifact.clone(),
                    &Default::default(),
                    &Default::default()
                )
                .unwrap(),
                Default::default()
            )
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
    let loaded_program =
        PreparedProgram::from_artifact(artifact.clone(), &Default::default(), &Default::default())
            .unwrap();
    let loaded = runtime
        .load_program(&loaded_program, Default::default())
        .unwrap();
    assert_eq!(
        runtime
            .execute(&loaded, "main", &[], &Default::default())
            .unwrap()
            .return_value
            .value(runtime.runtime().gc())
            .expect("retained execution result"),
        Value::I32(7)
    );
    let mut invalid = artifact.program;
    invalid.modules[invalid.root.index()]
        .host_interface
        .types
        .clear();
    assert!(verify_program(&invalid).is_err());
}
