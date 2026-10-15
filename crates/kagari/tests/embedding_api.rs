use kagari::{
    BytecodeArtifact,
    context::ExecutionContext,
    engine::{KagariEngine, source::ArtifactOptions},
    error::{EmbeddingError, RuntimeFailureKind},
    program::{PreparedProgram, ProgramPreparationError},
    runtime::{KagariRuntime, LoadOptions, ReloadOptions},
};
use kagari_abi::representation::ValueType;
use kagari_bytecode::{
    artifact::{ArtifactBuildOptions, KbcArtifact},
    instruction::{
        BytecodeInstruction, CallTarget, ConstantId, ConstantOperand, NativeImportId, PathId,
        Register,
    },
    module::{
        BytecodeFunction, BytecodeModule, FunctionMetadata, FunctionRecord, PathRecord,
        RootSlotLayout,
    },
    program::{BytecodeProgram, ModuleRef},
};
use kagari_common::identity::{
    DefinitionKind, ModuleIdentity, map::DefinitionContext, metadata::DefinitionMetadata,
};
use kagari_contract::{
    exports::ProgramExports,
    ids::FunctionRef,
    native_import::NativeImport,
    types::{ConcreteFunctionIdentity, PublicItem},
};
use kagari_runtime::value_semantics::script_equal;
use kagari_runtime::{
    host::{
        HostCallContext, HostError, HostObjectId, HostPathAdapter, HostPathDescriptor,
        HostPathDescriptorId, HostSchemaEpoch, PreparedHostPathWrite,
    },
    module::entry::ProgramEntry,
    native::{builder::ModuleBuilder, declarations::FunctionDecl, module::NativeModule},
    value::Value,
};
use kagari_source::source::SourceFile;
use kagari_types::declaration::logging;

use kagari_types::{
    callable::{CallableImplementation, Signature},
    declaration::{
        FieldDef, TypeDeclaration,
        host::{
            HostCallPolicy, HostFieldPolicy, HostFunctionEffects, HostReflectionPolicy,
            HostTypeOwnership, HostTypePolicy, PathAccess,
        },
        path::{HostPathDeclaration, HostPathSegmentDeclaration},
    },
    scalar::BuiltinType,
    ty::Ty,
    visibility::Visibility,
};

fn compile_artifact(
    engine: &KagariEngine,
    name: &str,
    source: &str,
) -> DefinitionMetadata<BytecodeArtifact> {
    engine
        .compile_to_artifact(SourceFile::new(name, source), ArtifactOptions::default())
        .expect("source should compile")
}

fn host_call_context() -> ExecutionContext {
    ExecutionContext {
        ..ExecutionContext::default()
    }
}

fn embedding_host_path_module(path_access: PathAccess) -> NativeModule {
    let catalog = kagari_stdlib::declarations::StandardDeclarations::default()
        .catalog()
        .unwrap();
    let mut builder = ModuleBuilder::new("game", &catalog).unwrap();
    let mut player = builder.define_type("Player").unwrap();
    player
        .external(HostTypePolicy {
            ownership: HostTypeOwnership::HostRoot,
            reflection: HostReflectionPolicy::Hidden,
            path_access: PathAccess::ReadWrite,
        })
        .unwrap();
    player
        .field(
            DefinitionMetadata::checked(
                DefinitionContext::new().unwrap().snapshot(),
                FieldDef {
                    name: "hp".into(),
                    ty: Ty::Builtin(BuiltinType::I32),
                    visibility: Visibility::Public,
                    mutable: true,
                    host_policy: Some(HostFieldPolicy {
                        readable: true,
                        path_access: PathAccess::ReadWrite,
                    }),
                },
                &Default::default(),
            )
            .unwrap(),
        )
        .unwrap();
    let player = player.finish().unwrap();
    let hp = player
        .definitions()
        .lookup_child(player.id(), DefinitionKind::Field, "hp", 0)
        .unwrap();
    let other_access = if path_access == PathAccess::ReadWrite {
        PathAccess::ReadOnly
    } else {
        PathAccess::ReadWrite
    };
    for access in [path_access, other_access] {
        let path = builder
            .define_path(
                DefinitionMetadata::checked(
                    player.definitions().clone(),
                    HostPathDeclaration {
                        root: player.id(),
                        segments: vec![HostPathSegmentDeclaration::Field(hp)],
                        access,
                        schema_epoch: 0,
                    },
                    &Default::default(),
                )
                .unwrap(),
            )
            .unwrap();
        let mut adapter = HostPathAdapter::new().with_read(|_, _| Ok(Value::I32(10)));
        if access == PathAccess::ReadWrite {
            adapter = adapter.with_prepare_write(|_, _, record| {
                if matches!(record.new_value, Value::I32(_)) {
                    Ok(PreparedHostPathWrite::new(|| {}))
                } else {
                    Err(HostError::new("hp expects i32"))
                }
            });
        }
        builder.bind_path(&path, adapter).unwrap();
    }
    let make = builder
        .define_function(
            FunctionDecl::new("player")
                .returns(player.apply([]).unwrap())
                .host_policy(HostCallPolicy {
                    passing: vec![],
                    effects: HostFunctionEffects {
                        may_allocate: true,
                        ..Default::default()
                    },
                }),
        )
        .unwrap();
    builder
        .bind(make, |cx: &HostCallContext<'_>, _: &[Value]| {
            let root = cx.runtime().host().root(HostObjectId(1)).unwrap();
            Ok(cx.runtime().gc().alloc_host_root(root).unwrap())
        })
        .unwrap();
    builder.finish().unwrap()
}

fn register_embedding_host_path_runtime(runtime: &mut KagariRuntime) -> HostPathDescriptorId {
    let player = runtime
        .runtime()
        .types()
        .get_by_name("game::Player")
        .unwrap()
        .id;
    runtime
        .runtime_mut()
        .register_host_root(HostObjectId(1), player, HostSchemaEpoch::new(0))
        .unwrap();
    let descriptor = HostPathDescriptorId::new(0);
    assert!(
        runtime
            .runtime()
            .host()
            .path_descriptor(descriptor)
            .is_some()
    );
    descriptor
}

fn host_path_artifact(
    contract: &HostPathDescriptor,
    source_name: &str,
    path_debug_name: &str,
    instructions: Vec<BytecodeInstruction>,
    registers: Vec<ValueType>,
    return_type: ValueType,
    constants: Vec<ConstantOperand>,
) -> DefinitionMetadata<BytecodeArtifact> {
    let definitions = DefinitionContext::new().unwrap();
    let provider = embedding_host_path_module(contract.access)
        .to_declaration()
        .import_into(&definitions, &Default::default())
        .unwrap();
    let module = provider.records();
    let make = module
        .native_declarations(provider.definitions())
        .unwrap()
        .remove(0);
    let CallableImplementation::Native(binding) = make.function.implementation else {
        panic!("native fixture entry")
    };
    let import = NativeImport {
        receiver_access: None,
        result_adapter: None,
        generic: None,
        instance: ConcreteFunctionIdentity {
            declaration: make.declaration,
            arguments: vec![],
        },
        binding,
        signature: Signature {
            params: vec![],
            result: make.function.return_type.clone(),
        },
        requirements: vec![],
        callables: vec![],
        host: None,
    };
    let path = module.paths[0].clone();
    let schema = TypeDeclaration {
        declaration: path.root,
        definition: module.types[0].clone(),
    };
    assert_eq!(
        path.contract(std::slice::from_ref(&schema), provider.definitions())
            .unwrap()
            .fingerprint(provider.definitions())
            .unwrap(),
        contract.abi_fingerprint.0
    );
    let identity = ModuleIdentity::single_file(source_name);
    let root = definitions.intern_root(&identity).unwrap();
    let main = definitions
        .intern_child(root, DefinitionKind::Function, "main", 0)
        .unwrap();
    let instance = ConcreteFunctionIdentity {
        declaration: main,
        arguments: vec![],
    };
    let mut metadata = FunctionMetadata {
        return_type,
        roots: RootSlotLayout::from_types(&[], &registers),
        registers,
        ..FunctionMetadata::default()
    };
    metadata.semantic.result = Some(match return_type {
        ValueType::Unit => Ty::Builtin(BuiltinType::Unit),
        ValueType::I32 => Ty::Builtin(BuiltinType::I32),
        _ => panic!("host path fixture returns only unit or i32"),
    });
    for (index, ty) in metadata.registers.iter().enumerate() {
        if *ty == ValueType::HostHandle {
            metadata
                .semantic
                .registers
                .insert(index, Ty::Host(schema.declaration));
        }
    }
    let artifact = KbcArtifact::from_program(
        BytecodeProgram {
            exports: ProgramExports {
                entry: Some(main),
                ..Default::default()
            },
            root: ModuleRef::new(0),
            modules: vec![
                BytecodeModule {
                    dependencies: vec![ModuleRef::new(1)],
                    external_types: vec![schema.clone()],
                    path_declarations: vec![path],
                    native_imports: if instructions.iter().any(|instruction| {
                        matches!(
                            instruction,
                            BytecodeInstruction::Call {
                                callee: CallTarget::Native(_),
                                ..
                            }
                        )
                    }) {
                        vec![import]
                    } else {
                        vec![]
                    },
                    identity,
                    source_name: source_name.to_owned(),
                    module_slots: vec![],
                    constants,
                    types: vec![ValueType::Unit, ValueType::HostHandle, ValueType::I32],
                    paths: vec![PathRecord {
                        contract_fingerprint: contract.abi_fingerprint.0,
                        id: PathId::new(0),
                        root_ty: ValueType::HostHandle,
                        result_ty: ValueType::I32,
                        read_only: contract.access == PathAccess::ReadOnly,
                        debug_name: path_debug_name.to_owned(),
                    }],
                    function_table: vec![FunctionRecord {
                        id: FunctionRef::new(0),
                        identity: Some(instance.clone()),
                        name: "main".to_owned(),
                        params: metadata.params.clone(),
                        return_type: metadata.return_type,
                        effects: metadata.effects,
                    }],
                    functions: vec![BytecodeFunction {
                        id: FunctionRef::new(0),
                        identity: Some(instance.clone()),
                        name: "main".to_owned(),
                        parameter_count: 0,
                        register_count: metadata.registers.len() as u16,
                        local_count: 0,
                        metadata,
                        instructions,
                    }],
                    ..BytecodeModule::default()
                },
                BytecodeModule {
                    identity: module.identity.clone(),
                    source_name: "native:game".into(),
                    external_types: vec![schema],
                    native_declarations: vec![make.clone()],
                    public_items: vec![
                        PublicItem::Type(module.types[0].clone()),
                        PublicItem::Function(make.function),
                    ],
                    ..Default::default()
                },
            ],
        },
        ArtifactBuildOptions::default(),
        &definitions,
    )
    .unwrap();
    DefinitionMetadata::checked(definitions.snapshot(), artifact, &Default::default()).unwrap()
}

fn logging_module(
    entry: impl for<'call> Fn(&HostCallContext<'call>, &[Value]) -> Result<Value, HostError>
    + Send
    + Sync
    + 'static,
) -> NativeModule {
    let mut builder = ModuleBuilder::from_declaration(
        logging::declaration().unwrap(),
        &kagari_stdlib::declarations::StandardDeclarations::default()
            .catalog()
            .unwrap(),
        Default::default(),
    )
    .unwrap();
    let log = builder.declared_function("log").unwrap();
    builder.bind(log, entry).unwrap();
    builder.finish().unwrap()
}

#[test]
fn compiles_loads_executes_and_reloads_through_embedding_api() {
    let engine = KagariEngine::default();
    let context = ExecutionContext::default();
    let first = compile_artifact(&engine, "game/main.kgr", "fn main() -> i32 { 1 }");
    let second = compile_artifact(&engine, "game/main.kgr", "fn main() -> i32 { 2 }");

    let mut runtime = engine.runtime(context.clone());
    let loaded = runtime
        .load_program(
            &PreparedProgram::from_artifact(first, &Default::default(), &Default::default())
                .unwrap(),
            LoadOptions {
                module_name: Some("game.main".to_owned()),
            },
        )
        .expect("module should load");
    let report = runtime
        .execute(&loaded, ProgramEntry::Main, &[], &context)
        .expect("entry should execute");
    assert_eq!(
        report
            .return_value
            .value(runtime.runtime().gc())
            .expect("retained execution result"),
        Value::I32(1)
    );

    let reloaded = runtime
        .reload_program(
            &loaded,
            &PreparedProgram::from_artifact(second, &Default::default(), &Default::default())
                .unwrap(),
            ReloadOptions {
                module_name: Some("game.main".to_owned()),
            },
        )
        .expect("compatible module should reload");
    assert_eq!(reloaded.id, loaded.id);
    assert_eq!(reloaded.epoch.0, loaded.epoch.0 + 1);

    let report = runtime
        .execute(&reloaded, ProgramEntry::Main, &[], &context)
        .expect("reloaded entry should execute");
    assert_eq!(
        report
            .return_value
            .value(runtime.runtime().gc())
            .expect("retained execution result"),
        Value::I32(2)
    );
}

#[test]
fn compile_failures_return_structured_diagnostics() {
    let engine = KagariEngine::default();
    let error = engine
        .compile_source(SourceFile::new("bad.kgr", "fn main( -> i32 { 1 }"))
        .expect_err("parse failure should be structured");

    let EmbeddingError::Diagnostics { diagnostics } = error else {
        panic!("expected diagnostic error");
    };
    assert!(!diagnostics.is_empty());
    assert!(
        diagnostics
            .iter()
            .all(|diagnostic| !diagnostic.code.is_empty())
    );
    assert!(
        diagnostics
            .iter()
            .all(|diagnostic| !diagnostic.message.is_empty())
    );
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code.starts_with("KG_PARSE_"))
    );
}

#[test]
fn analysis_failures_return_structured_diagnostics() {
    let engine = KagariEngine::default();
    let error = engine
        .compile_source(SourceFile::new(
            "bad_type.kgr",
            "fn main() -> Missing { 1 }",
        ))
        .expect_err("analysis failure should be structured");

    let EmbeddingError::Diagnostics { diagnostics } = error else {
        panic!("expected diagnostic error");
    };
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "KG_TYPE_UNKNOWN_TYPE")
    );
}

#[test]
fn each_execute_uses_its_cancellation_without_changing_runtime_defaults() {
    let engine = KagariEngine::default();
    let mut runtime = engine.runtime(ExecutionContext::default());
    let artifact = compile_artifact(&engine, "scoped.kgr", "fn main() -> i32 { 42 }");
    let loaded = runtime
        .load_program(
            &PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default())
                .unwrap(),
            LoadOptions::default(),
        )
        .unwrap();
    let context = ExecutionContext::default();

    for _ in 0..2 {
        assert_eq!(
            runtime
                .execute(&loaded, ProgramEntry::Main, &[], &context)
                .unwrap()
                .return_value
                .value(runtime.runtime().gc())
                .expect("retained execution result"),
            Value::I32(42)
        );
    }

    context.cancellation.cancel();
    assert_eq!(
        runtime
            .execute(&loaded, ProgramEntry::Main, &[], &context)
            .unwrap_err()
            .code(),
        "KG_RUNTIME_CANCELLED"
    );
    assert_eq!(
        runtime
            .execute(
                &loaded,
                ProgramEntry::Main,
                &[],
                &ExecutionContext::default()
            )
            .unwrap()
            .return_value
            .value(runtime.runtime().gc())
            .expect("retained execution result"),
        Value::I32(42)
    );
    assert!(!runtime.runtime().is_quarantined());
}

#[test]
fn failed_reload_validation_does_not_publish_new_epoch() {
    let engine = KagariEngine::default();
    let context = ExecutionContext::default();
    let first = compile_artifact(&engine, "reload.kgr", "fn main() -> i32 { 1 }");
    let candidate = compile_artifact(&engine, "reload.kgr", "fn main() -> i32 { 2 }");
    let (definitions, mut candidate) = candidate.into_parts();
    candidate.header.runtime_abi_version = "wrong-runtime-abi".to_owned();
    let candidate =
        DefinitionMetadata::checked(definitions, candidate, &Default::default()).unwrap();

    let mut runtime = engine.runtime(context.clone());
    let loaded = runtime
        .load_program(
            &PreparedProgram::from_artifact(first, &Default::default(), &Default::default())
                .unwrap(),
            LoadOptions {
                module_name: Some("reload".to_owned()),
            },
        )
        .expect("module should load");
    let before_count = runtime.runtime().modules().loaded_count();

    let error =
        PreparedProgram::from_artifact(candidate, &Default::default(), &context.cancellation)
            .expect_err("invalid artifact should not be prepared for reload");
    let ProgramPreparationError::Artifact(error) = error else {
        panic!("expected artifact validation error");
    };
    assert_eq!(error.code(), "KG_ARTIFACT_RUNTIME_ABI_MISMATCH");
    assert_eq!(runtime.runtime().modules().loaded_count(), before_count);
    assert_eq!(
        runtime.runtime().modules().latest("reload").unwrap().epoch,
        loaded.epoch
    );
    assert_eq!(
        runtime
            .execute(&loaded, ProgramEntry::Main, &[], &context)
            .unwrap()
            .return_value
            .value(runtime.runtime().gc())
            .expect("retained execution result"),
        Value::I32(1)
    );
}

#[test]
fn reload_rejects_typed_path_fingerprint_changes_without_publishing_epoch() {
    let engine = KagariEngine::with_native_modules(
        Default::default(),
        vec![embedding_host_path_module(PathAccess::ReadWrite)],
    )
    .unwrap();
    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    register_embedding_host_path_runtime(&mut runtime);
    let contract = runtime
        .runtime()
        .host()
        .path_descriptor(HostPathDescriptorId::new(0))
        .unwrap()
        .clone();
    let first = host_path_artifact(
        &contract,
        "reload_paths.kgr",
        "game.Player.hp",
        vec![BytecodeInstruction::Return(None)],
        vec![],
        ValueType::Unit,
        vec![],
    );
    let changed = runtime
        .runtime()
        .host()
        .path_descriptor(HostPathDescriptorId::new(1))
        .unwrap()
        .clone();
    assert_ne!(changed.abi_fingerprint, contract.abi_fingerprint);
    let candidate = host_path_artifact(
        &changed,
        "reload_paths.kgr",
        "game.Player.hp.readonly",
        vec![BytecodeInstruction::Return(None)],
        vec![],
        ValueType::Unit,
        vec![],
    );

    let loaded = runtime
        .load_program(
            &PreparedProgram::from_artifact(first, &Default::default(), &Default::default())
                .unwrap(),
            LoadOptions {
                module_name: Some("reload_paths".to_owned()),
            },
        )
        .expect("module should load");
    let before_count = runtime.runtime().modules().loaded_count();

    let error = runtime
        .reload_program(
            &loaded,
            &PreparedProgram::from_artifact(candidate, &Default::default(), &Default::default())
                .unwrap(),
            ReloadOptions {
                module_name: Some("reload_paths".to_owned()),
            },
        )
        .expect_err("changed typed path fingerprints should reject reload");

    let EmbeddingError::ReloadValidation { code, message } = error else {
        panic!("expected reload validation error");
    };
    assert_eq!(code, "KG_RELOAD_PATH_FINGERPRINT_MISMATCH");
    assert!(message.contains("typed path fingerprints"));
    assert_eq!(runtime.runtime().modules().loaded_count(), before_count);
    assert_eq!(
        runtime
            .runtime()
            .modules()
            .latest("reload_paths")
            .unwrap()
            .epoch,
        loaded.epoch
    );
}

#[test]
fn execute_entry_accepts_args_boundary_and_rejects_unimplemented_arguments() {
    let engine = KagariEngine::default();
    let context = ExecutionContext::default();
    let artifact = compile_artifact(&engine, "args.kgr", "fn main() -> i32 { 1 }");
    let mut runtime = engine.runtime(context.clone());
    let loaded = runtime
        .load_program(
            &PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default())
                .unwrap(),
            LoadOptions {
                module_name: Some("args".to_owned()),
            },
        )
        .expect("module should load");

    let error = runtime
        .execute(&loaded, ProgramEntry::Main, &[Value::I32(1)], &context)
        .expect_err("argument passing is not implemented yet");

    assert_eq!(error.code(), "KG_RUNTIME_UNSUPPORTED_EXECUTION");
    assert!(matches!(
        error,
        EmbeddingError::Runtime {
            kind: RuntimeFailureKind::UnsupportedExecution,
            ..
        }
    ));
}

#[test]
fn installed_host_path_mutation_uses_declared_access() {
    let engine = KagariEngine::with_native_modules(
        Default::default(),
        vec![embedding_host_path_module(PathAccess::ReadWrite)],
    )
    .unwrap();
    let context = host_call_context();
    let mut runtime = engine.runtime(context.clone());
    register_embedding_host_path_runtime(&mut runtime);
    let contract = runtime
        .runtime()
        .host()
        .path_descriptor(HostPathDescriptorId::new(0))
        .unwrap()
        .clone();
    let artifact = host_path_artifact(
        &contract,
        "set_path.kgr",
        "game.Player.hp",
        vec![
            BytecodeInstruction::Call {
                dst: Some(Register::new(0)),
                callee: CallTarget::Native(NativeImportId::new(0)),
                args: vec![],
            },
            BytecodeInstruction::LoadConst {
                dst: Register::new(1),
                constant: ConstantId::new(0),
            },
            BytecodeInstruction::SetPath {
                root_or_view: Register::new(0),
                path: PathId::new(0),
                dynamic_args: vec![],
                value: Register::new(1),
            },
            BytecodeInstruction::Return(None),
        ],
        vec![ValueType::HostHandle, ValueType::I32],
        ValueType::Unit,
        vec![ConstantOperand::I32(5)],
    );
    let loaded = runtime
        .load_program(
            &PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default())
                .unwrap(),
            LoadOptions {
                module_name: Some("set_path".to_owned()),
            },
        )
        .expect("module should load");

    assert_eq!(
        runtime
            .execute(&loaded, ProgramEntry::Main, &[], &context)
            .unwrap()
            .return_value
            .value(runtime.runtime().gc())
            .expect("retained execution result"),
        Value::Unit
    );
}

#[test]
fn installed_host_path_reads_need_no_permission_flags() {
    let engine = KagariEngine::with_native_modules(
        Default::default(),
        vec![embedding_host_path_module(PathAccess::ReadOnly)],
    )
    .unwrap();
    let context = host_call_context();
    let mut runtime = engine.runtime(context.clone());
    register_embedding_host_path_runtime(&mut runtime);
    let contract = runtime
        .runtime()
        .host()
        .path_descriptor(HostPathDescriptorId::new(0))
        .unwrap()
        .clone();
    let artifact = host_path_artifact(
        &contract,
        "read_secure_path.kgr",
        "game.Player.secure_hp",
        vec![
            BytecodeInstruction::Call {
                dst: Some(Register::new(0)),
                callee: CallTarget::Native(NativeImportId::new(0)),
                args: vec![],
            },
            BytecodeInstruction::ReadPath {
                dst: Register::new(1),
                root_or_view: Register::new(0),
                path: PathId::new(0),
                dynamic_args: vec![],
            },
            BytecodeInstruction::Return(Some(Register::new(1))),
        ],
        vec![ValueType::HostHandle, ValueType::I32],
        ValueType::I32,
        vec![],
    );
    let loaded = runtime
        .load_program(
            &PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default())
                .unwrap(),
            LoadOptions {
                module_name: Some("read_secure_path".to_owned()),
            },
        )
        .expect("module should load");

    assert_eq!(
        runtime
            .execute(&loaded, ProgramEntry::Main, &[], &context)
            .unwrap()
            .return_value
            .value(runtime.runtime().gc())
            .expect("retained execution result"),
        Value::I32(10)
    );
}

#[test]
fn installed_host_and_reflection_helpers_are_available() {
    let engine = KagariEngine::default();
    let print_artifact = compile_artifact(&engine, "print.kgr", r#"fn main() { print("x"); }"#);
    let type_of_artifact = engine
        .compile_to_artifact(
            SourceFile::new("type_of.kgr", r#"fn main() -> String { type_of(7) }"#),
            ArtifactOptions::default(),
        )
        .expect("reflection source should compile with reflection profile");
    let mut runtime = engine.runtime(ExecutionContext::default());
    logging_module(|_, _| Ok(Value::Unit))
        .install(runtime.runtime_mut())
        .unwrap();
    let print_module = runtime
        .load_program(
            &PreparedProgram::from_artifact(
                print_artifact,
                &Default::default(),
                &Default::default(),
            )
            .unwrap(),
            LoadOptions {
                module_name: Some("print".to_owned()),
            },
        )
        .expect("print module should load");
    let type_of_module = runtime
        .load_program(
            &PreparedProgram::from_artifact(
                type_of_artifact,
                &Default::default(),
                &Default::default(),
            )
            .unwrap(),
            LoadOptions {
                module_name: Some("type_of".to_owned()),
            },
        )
        .expect("type_of module should load");

    assert_eq!(
        runtime
            .execute(
                &print_module,
                ProgramEntry::Main,
                &[],
                &ExecutionContext::default()
            )
            .unwrap()
            .return_value
            .value(runtime.runtime().gc())
            .expect("retained execution result"),
        Value::Unit
    );
    assert!(
        script_equal(
            runtime.runtime().gc(),
            &(runtime
                .execute(
                    &type_of_module,
                    ProgramEntry::Main,
                    &[],
                    &ExecutionContext::default()
                )
                .unwrap()
                .return_value
                .value(runtime.runtime().gc())
                .expect("retained execution result")),
            &(runtime.runtime().gc().alloc_string("i32".into()).unwrap())
        )
        .unwrap()
    );
}
