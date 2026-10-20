use super::{adopt_artifact, schema::NominalSchema};
use crate::{
    context::{ExecutionContext, JitPolicy},
    engine::KagariEngine,
    program::PreparedProgram,
};
use kagari_abi::representation::ValueType;
use kagari_bytecode::{artifact::KbcArtifact, program::verify_program};
use kagari_codegen_cranelift::CraneliftBackend;
use kagari_common::identity::{PackageId, map::DefinitionContext, metadata::DefinitionMetadata};
use kagari_contract::{representation::semantic_representation, types::PublicItem};
use kagari_hir::declarations::DeclarationId;
use kagari_runtime::{
    host::{HostCallContext, HostObjectId, HostSchemaEpoch},
    module::entry::ProgramEntry,
    native::{
        adapter::NativeAdapter,
        declarations::{FunctionDecl, MethodDecl},
        types::Type,
    },
    value::Value,
};
use kagari_source::{source::SourceFile, source_database::SourceLayer};
use kagari_types::{
    callable::CallableImplementation,
    declaration::{
        FnDecl, Param,
        host::{HostCallPolicy, HostFunctionEffects, HostPassingStyle},
    },
    scalar::BuiltinType,
    ty::Ty,
};
use kagari_vm::reentry::reenter;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicI32, Ordering},
};

#[test]
fn offline_host_type_navigation_is_available_from_signature_query() {
    let engine = NominalSchema::new().engine();
    for (import, ty) in [
        ("use left::Item;", "Item"),
        ("use left as api;", "api::Item"),
    ] {
        let text = format!("// 中文 😀\r\n{import} fn accept(value: {ty}) -> {ty} {{ value }}");
        let type_start = text.find("value: ").unwrap() + "value: ".len();
        let annotation = type_start + ty.len() - "Item".len();
        let file = engine
            .set_source("mem://host-signature", text, SourceLayer::Base)
            .unwrap();
        let signatures = engine
            .signatures(engine.source_snapshot(), &Default::default())
            .unwrap();
        let signature = signatures.file(file).unwrap();
        let declaration = signature.definition_at(annotation).unwrap();
        assert_eq!(declaration.name, "Item");
        let DeclarationId::Definition(id) = declaration.id else {
            panic!("common external declaration")
        };
        let schema = signature.declarations().external_type(id).unwrap();
        assert_eq!(schema.name, "Item");
        assert_eq!(
            signature
                .declarations()
                .definitions()
                .resolve(id)
                .unwrap()
                .module()
                .package,
            PackageId("left".into())
        );
        assert!(signature.definition_at(annotation - 1).is_none());
        if type_start != annotation {
            assert!(signature.definition_at(type_start).is_none());
        }
        assert!(signature.diagnostics().is_empty());
        let full = engine
            .analyze(engine.source_snapshot(), &Default::default())
            .unwrap();
        assert_eq!(
            full.file(file).unwrap().definition_at(annotation),
            Some(declaration)
        );
    }
}

#[test]
fn declared_methods_link_by_identity_and_evaluate_receiver_then_arguments_once() {
    let mut schema = NominalSchema::new();
    let trace = Arc::new(Mutex::new(Vec::new()));
    let total = Arc::new(AtomicI32::new(40));
    schema.make(trace.clone(), "receiver");
    let rhs = schema
        .builder
        .define_function(FunctionDecl::new("rhs").returns(Type::i32()).host_policy(
            HostCallPolicy {
                passing: vec![],
                effects: Default::default(),
            },
        ))
        .unwrap();
    let calls = trace.clone();
    schema
        .builder
        .bind(rhs, move |_: &HostCallContext<'_>, _: &[Value]| {
            calls.lock().unwrap().push("argument");
            Ok(Value::I32(2))
        })
        .unwrap();
    let method_policy = HostCallPolicy {
        passing: vec![HostPassingStyle::UniqueBorrow, HostPassingStyle::Owned],
        effects: HostFunctionEffects {
            may_mutate_host_state: true,
            may_call_host_services: true,
            may_trap: true,
            ..Default::default()
        },
    };
    let declaration = MethodDecl::instance("add")
        .parameter("amount", Type::i32())
        .returns(Type::i32())
        .host_policy(method_policy.clone());
    let mut missing = schema.builder.clone();
    missing
        .implement(schema.item.clone(), |group| {
            group.inherent_impl(|methods| methods.define_method(declaration.clone()))
        })
        .unwrap();
    assert!(
        missing.finish().is_err(),
        "a declared member needs its binding"
    );
    let calls = trace.clone();
    let state = total.clone();
    let item = schema.item.clone();
    let catalog = schema.builder.declarations().clone();
    let method = schema
        .builder
        .implement(schema.item.clone(), |group| {
            group.inherent_impl(|methods| {
                let method = methods.define_method(declaration)?;
                let mut wrong_policy = method_policy;
                wrong_policy.passing[0] = HostPassingStyle::Owned;
                let wrong = FnDecl {
                    name: "add".into(),
                    host_policy: Some(wrong_policy),
                    method_policy: Default::default(),
                    implementation: CallableImplementation::Native(method.id()),
                    generic_params: vec![],
                    bounds: vec![],
                    params: vec![
                        Param {
                            name: "self".into(),
                            ty: Ty::Host(item.id()),
                            mutable: false,
                        },
                        Param {
                            name: "amount".into(),
                            ty: Ty::Builtin(BuiltinType::I32),
                            mutable: false,
                        },
                    ],
                    return_type: Ty::Builtin(BuiltinType::I32),
                };
                let wrong =
                    (|_: &HostCallContext<'_>, _: &[Value]| panic!("invalid member binding"))
                        .prepare(&wrong, &catalog)?;
                assert!(methods.bind(method.clone(), wrong).is_err());
                methods.bind(
                    method.clone(),
                    move |context: &HostCallContext<'_>, args: &[Value]| {
                        calls.lock().unwrap().push("method");
                        let root = context.runtime().execution_root().unwrap();
                        let nested = root
                            .bytecode
                            .functions
                            .iter()
                            .find(|function| function.name == "nested")
                            .unwrap()
                            .id;
                        assert!(
                            reenter(context, &root, nested, &args[..1]).is_err(),
                            "the receiver's exclusive lease prevents recursive access"
                        );
                        let Value::I32(amount) = args[1] else {
                            panic!("checked parameter")
                        };
                        state.fetch_add(amount, Ordering::SeqCst);
                        context.runtime().collect_garbage().unwrap();
                        Ok(Value::I32(state.load(Ordering::SeqCst)))
                    },
                )?;
                Ok(method)
            })
        })
        .unwrap();
    let engine = schema.engine();
    let artifact = engine.compile_to_artifact(SourceFile::new("method.kgr",
        "use left as api; fn main() -> i32 { api::make().add(api::rhs()) } pub fn nested(value: api::Item) -> i32 { value.add(0) }"), Default::default()).unwrap();
    assert!(trace.lock().unwrap().is_empty());
    let method_path = method.definitions().resolve(method.id()).unwrap().to_path();
    let contract = artifact
        .records()
        .program
        .modules
        .iter()
        .flat_map(|module| &module.native_declarations)
        .find(|entry| {
            artifact
                .definitions()
                .resolve(entry.declaration)
                .unwrap()
                .to_path()
                == method_path
        })
        .unwrap();
    assert_eq!(
        contract.function.host_policy.as_ref().unwrap().passing[0],
        HostPassingStyle::UniqueBorrow
    );
    for (encoded, jit) in [(false, false), (true, false), (true, true)] {
        trace.lock().unwrap().clear();
        total.store(40, Ordering::SeqCst);
        let artifact = round_trip(&artifact, encoded);
        let context = context(jit);
        let program =
            PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default())
                .unwrap();
        assert!(
            KagariEngine::default()
                .runtime(context.clone())
                .load_program(&program, Default::default())
                .is_err()
        );
        assert!(trace.lock().unwrap().is_empty());
        let mut runtime = engine.runtime(context.clone());
        let ty = runtime
            .runtime()
            .types()
            .get_by_name("left::Item")
            .unwrap()
            .id;
        runtime
            .runtime_mut()
            .register_host_root(HostObjectId(7), ty, HostSchemaEpoch::new(0))
            .unwrap();
        let loaded = runtime.load_program(&program, Default::default()).unwrap();
        let mut backend = jit.then(|| CraneliftBackend::for_host().unwrap());
        for expected in [42, 44] {
            let result = if let Some(backend) = &mut backend {
                let prepared = runtime
                    .prepare_native(
                        &program,
                        &loaded,
                        ProgramEntry::Main,
                        backend,
                        &context.cancellation,
                    )
                    .unwrap();
                runtime.execute_prepared(&loaded, ProgramEntry::Main, &[], &context, &prepared)
            } else {
                runtime.execute(&loaded, ProgramEntry::Main, &[], &context)
            }
            .unwrap();
            assert_eq!(
                result.return_value.value(runtime.runtime().gc()).unwrap(),
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
    let mut schema = NominalSchema::new();
    let trace = Arc::new(Mutex::new(Vec::new()));
    schema.make(trace.clone(), "make");
    let take = schema
        .builder
        .define_function(
            FunctionDecl::new("take")
                .parameter("value", schema.item.apply([]).unwrap())
                .returns(Type::i32())
                .host_policy(HostCallPolicy {
                    passing: vec![HostPassingStyle::SharedBorrow],
                    effects: HostFunctionEffects {
                        may_allocate: true,
                        ..Default::default()
                    },
                }),
        )
        .unwrap();
    let calls = trace.clone();
    schema
        .builder
        .bind(
            take,
            move |context: &HostCallContext<'_>, args: &[Value]| {
                calls.lock().unwrap().push("take");
                assert!(matches!(args[0], Value::HostRoot(_)));
                context.runtime().collect_garbage().unwrap();
                Ok(Value::I32(42))
            },
        )
        .unwrap();
    let definitions = DefinitionContext::new().unwrap();
    let expected = schema.required_types(&definitions);
    let engine = schema.engine();
    let artifact = engine.compile_to_artifact(SourceFile::new("nominal.kgr",
        "use left::Item; use left as api; pub fn pass(value: Item) -> api::Item { value } fn id<T>(value: T) -> T { value } fn main() -> i32 { val value: Item = api::make(); api::take(pass(id(value))) }"), Default::default()).unwrap();
    let artifact = adopt_artifact(&definitions, artifact);
    let module = &artifact.records().program.modules[artifact.records().program.root.index()];
    assert_eq!(module.external_types.len(), expected.len());
    for schema in &expected {
        assert!(module.external_types.contains(schema));
    }
    let PublicItem::Function(pass) = &module.public_items[0] else {
        panic!("public pass")
    };
    assert_eq!(pass.return_type, Ty::Host(expected[0].declaration));
    assert_eq!(
        semantic_representation(&pass.return_type),
        ValueType::HostHandle
    );
    let mut invalid = artifact.records().program.clone();
    invalid.modules[invalid.root.index()].external_types.clear();
    assert!(
        KbcArtifact::from_program(
            invalid,
            Default::default(),
            &definitions,
            &Default::default()
        )
        .is_err()
    );
    for (encoded, jit) in [(false, false), (true, false), (true, true)] {
        trace.lock().unwrap().clear();
        let context = context(jit);
        let mut runtime = engine.runtime(context.clone());
        let ty = runtime
            .runtime()
            .types()
            .get_by_name("left::Item")
            .unwrap()
            .id;
        runtime
            .runtime_mut()
            .register_host_root(HostObjectId(7), ty, HostSchemaEpoch::new(0))
            .unwrap();
        let program = PreparedProgram::from_artifact(
            round_trip(&artifact, encoded),
            &Default::default(),
            &Default::default(),
        )
        .unwrap();
        let loaded = runtime.load_program(&program, Default::default()).unwrap();
        assert!(trace.lock().unwrap().is_empty());
        let report = if jit {
            let mut backend = CraneliftBackend::for_host().unwrap();
            let prepared = runtime
                .prepare_native(
                    &program,
                    &loaded,
                    ProgramEntry::Main,
                    &mut backend,
                    &context.cancellation,
                )
                .unwrap();
            runtime.execute_prepared(&loaded, ProgramEntry::Main, &[], &context, &prepared)
        } else {
            runtime.execute(&loaded, ProgramEntry::Main, &[], &context)
        }
        .unwrap();
        assert_eq!(
            report.return_value.value(runtime.runtime().gc()).unwrap(),
            Value::I32(42)
        );
        assert_eq!(*trace.lock().unwrap(), ["make", "take"]);
        drop(report);
        assert_eq!(runtime.runtime().gc().active_roots(), 0);
    }
}

#[test]
fn annotation_only_host_dependencies_are_verified_and_linked() {
    let schema = NominalSchema::new();
    let definitions = DefinitionContext::new().unwrap();
    let expected = schema.required_types(&definitions);
    let engine = schema.engine();
    let artifact = engine.compile_to_artifact(SourceFile::new("annotation.kgr",
        "use left; pub fn pass(value: left::Item) -> left::Item { value } fn main() -> i32 { 7 }"), Default::default()).unwrap();
    let artifact = adopt_artifact(&definitions, artifact);
    let module = &artifact.records().program.modules[artifact.records().program.root.index()];
    assert!(module.native_imports.is_empty());
    assert_eq!(module.external_types.len(), expected.len());
    for schema in &expected {
        assert!(module.external_types.contains(schema));
    }
    let program =
        PreparedProgram::from_artifact(artifact.clone(), &Default::default(), &Default::default())
            .unwrap();
    assert!(
        KagariEngine::default()
            .runtime(Default::default())
            .load_program(&program, Default::default())
            .is_err()
    );
    let mut runtime = engine.runtime(Default::default());
    let loaded = runtime.load_program(&program, Default::default()).unwrap();
    assert_eq!(
        runtime
            .execute(&loaded, ProgramEntry::Main, &[], &Default::default())
            .unwrap()
            .return_value
            .value(runtime.runtime().gc())
            .unwrap(),
        Value::I32(7)
    );
    let mut invalid = artifact.records().program.clone();
    invalid.modules[invalid.root.index()].external_types.clear();
    assert!(verify_program(&invalid, artifact.definitions(), &Default::default()).is_err());
}

fn context(jit: bool) -> ExecutionContext {
    ExecutionContext {
        jit_policy: if jit {
            JitPolicy::Enabled
        } else {
            JitPolicy::Disabled
        },
        ..Default::default()
    }
}

fn round_trip(
    artifact: &DefinitionMetadata<KbcArtifact>,
    encoded: bool,
) -> DefinitionMetadata<KbcArtifact> {
    if encoded {
        KbcArtifact::from_bytes(&artifact.records().to_bytes(artifact.definitions()).unwrap())
            .unwrap()
    } else {
        artifact.clone()
    }
}
