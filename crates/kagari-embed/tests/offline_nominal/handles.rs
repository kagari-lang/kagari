use super::*;
use kagari_abi::representation::ValueType;
use kagari_bytecode::{artifact::KbcArtifact, program::verify_program};
use kagari_contract::{representation::semantic_representation, types::PublicItem};
use kagari_embed::{context::JitPolicy, program::PreparedProgram};
use kagari_types::ty::Ty;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicI32, Ordering},
};

#[test]
fn offline_host_type_navigation_is_available_from_signature_query() {
    let definitions = DefinitionContext::new().unwrap();
    let engine = KagariEngine::default();
    engine
        .set_host_interface(scoped(&definitions, interface(&definitions)))
        .unwrap();
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
    let definitions = DefinitionContext::new().unwrap();
    let mut interface = interface(&definitions);
    let mut method = HostMethodDeclaration::new(
        &definitions,
        interface.types[0].id,
        "add",
        vec![HostParameter {
            name: "amount".into(),
            ty: HostValueType::I32,
            passing: HostPassingStyle::Owned,
        }],
        HostValueType::I32,
    )
    .unwrap();
    method.receiver = HostPassingStyle::UniqueBorrow;
    method.effects.may_mutate_host_state = true;

    let method_id = method.id;
    interface.types[0].methods.push(method);
    let rhs =
        HostFunctionDeclaration::new(&definitions, "left.rhs", vec![], HostValueType::I32).unwrap();
    interface.functions.push(rhs.clone());
    let engine = KagariEngine::default();
    engine
        .set_host_interface(scoped(&definitions, interface.clone()))
        .unwrap();

    let artifact = engine
        .compile_to_artifact(
            SourceFile::new(
                "method.kgr",
                "use left as api; fn main() -> i32 { api::make().add(api::rhs()) }",
            ),
            Default::default(),
        )
        .unwrap();
    let artifact = adopt_artifact(&definitions, artifact);
    let required =
        &artifact.records().program.modules[artifact.records().program.root.index()].host_interface;
    let call = required
        .functions
        .iter()
        .find(|f| f.id == method_id)
        .unwrap();
    assert_eq!(
        call,
        &interface.types[0]
            .method_contract(&method_id, &definitions.snapshot())
            .unwrap()
    );
    for (encoded, jit) in [(false, false), (true, false), (true, true)] {
        let artifact = if encoded {
            KbcArtifact::from_bytes(&artifact.records().to_bytes(artifact.definitions()).unwrap())
                .unwrap()
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
                    .map(|ty| HostTypeRegistration::new(scoped(&definitions, ty), "Object"))
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
                scoped(&definitions, interface.functions[0].clone()),
                move |cx, _| {
                    calls.lock().unwrap().push("receiver");
                    Ok(cx.runtime().gc().alloc_host_root(root).unwrap())
                },
            ))
            .unwrap();
        let calls = trace.clone();
        runtime
            .register_host_function(HostFunction::new(
                scoped(&definitions, rhs.clone()),
                move |_, _| {
                    calls.lock().unwrap().push("argument");
                    Ok(Value::I32(2))
                },
            ))
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
        let mut wrong = interface.types[0]
            .method_contract(&method_id, &definitions.snapshot())
            .unwrap();
        wrong.params[0].passing = HostPassingStyle::Owned;
        assert!(
            runtime
                .register_host_function(HostFunction::new(
                    scoped(&definitions, wrong),
                    |_, _| panic!("invalid member binding")
                ))
                .is_err()
        );
        let total = Arc::new(AtomicI32::new(40));
        let state = total.clone();
        let calls = trace.clone();
        runtime
            .register_host_function(
                HostFunction::method(
                    &scoped(&definitions, interface.types[0].clone()),
                    method_id,
                    move |context, args| {
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
                    },
                )
                .unwrap(),
            )
            .unwrap();
        let loaded_program =
            PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default())
                .unwrap();
        let loaded = runtime
            .load_program(&loaded_program, Default::default())
            .unwrap();
        let mut backend = jit.then(|| CraneliftBackend::for_host().unwrap());
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
    let definitions = DefinitionContext::new().unwrap();
    let interface = interface(&definitions);
    let engine = KagariEngine::default();
    engine
        .set_host_interface(
            HostInterface::from_bytes(&interface.to_bytes(&definitions.snapshot()).unwrap())
                .unwrap(),
        )
        .unwrap();

    let artifact = engine.compile_to_artifact(
        SourceFile::new("nominal.kgr", "use left::Item; use left as api; pub fn pass(value: Item) -> api::Item { value } fn id<T>(value: T) -> T { value } fn main() -> i32 { val value: Item = api::make(); api::take(pass(id(value))) }"),
         ArtifactOptions::default(),
    ).unwrap();
    let artifact = adopt_artifact(&definitions, artifact);
    let module = &artifact.records().program.modules[artifact.records().program.root.index()];
    assert_eq!(module.host_interface.types, interface.types[..2]);
    let PublicItem::Function(pass) = &module.public_items[0] else {
        panic!("public pass")
    };
    assert_eq!(pass.return_type, Ty::Host(interface.types[0].id));
    assert_eq!(
        semantic_representation(&pass.return_type),
        ValueType::HostHandle
    );
    let mut invalid = artifact.records().program.clone();
    invalid.modules[artifact.records().program.root.index()]
        .host_interface
        .types
        .clear();
    assert!(KbcArtifact::from_program(invalid, Default::default(), &definitions).is_err());
    for (encoded, jit) in [(false, false), (true, false), (true, true)] {
        let artifact = if encoded {
            KbcArtifact::from_bytes(&artifact.records().to_bytes(artifact.definitions()).unwrap())
                .unwrap()
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
                    .map(|ty| HostTypeRegistration::new(scoped(&definitions, ty), "Object"))
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
                scoped(&definitions, interface.functions[0].clone()),
                move |cx, _| {
                    calls.lock().unwrap().push("make");
                    Ok(cx.runtime().gc().alloc_host_root(root).unwrap())
                },
            ))
            .unwrap();
        let calls = trace.clone();
        runtime
            .register_host_function(HostFunction::new(
                scoped(&definitions, interface.functions[1].clone()),
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
            let mut backend = CraneliftBackend::for_host().unwrap();
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
    let definitions = DefinitionContext::new().unwrap();
    let engine = KagariEngine::default();
    let interface = interface(&definitions);
    engine
        .set_host_interface(scoped(&definitions, interface.clone()))
        .unwrap();
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new(
                "annotation.kgr",
                "pub fn pass(value: left::Item) -> left::Item { value } fn main() -> i32 { 7 }",
            ),
            Default::default(),
        )
        .unwrap();
    let artifact = adopt_artifact(&definitions, artifact);
    let module = &artifact.records().program.modules[artifact.records().program.root.index()];
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
                .map(|ty| HostTypeRegistration::new(scoped(&definitions, ty), "Object"))
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
    let mut invalid = artifact.into_records().program;
    invalid.modules[invalid.root.index()]
        .host_interface
        .types
        .clear();
    assert!(verify_program(&invalid, &definitions.snapshot()).is_err());
}
