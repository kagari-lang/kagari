use kagari_common::SourceFile;
use kagari_embed::{
    ArtifactOptions, BytecodeArtifact, CompileOptions, EmbeddingError, ExecutionContext,
    HostExposurePolicy, KagariEngine, KagariRuntime, LoadOptions, ReloadOptions,
    RuntimeFailureKind,
};
use kagari_ir::{
    bytecode::{
        ArtifactBuildOptions, BytecodeFunction, BytecodeInstruction, BytecodeModule, CallTarget,
        ConstantOperand, FunctionMetadata, FunctionRecord, FunctionRef, KbcArtifact, PathId,
        PathRecord, Register,
    },
    module::ValueType,
};
use kagari_runtime::{
    AbiFingerprint, CapabilitySet, HostObjectId, HostPathAdapter, HostPathDescriptorId,
    HostPathDescriptorRegistration, HostPathSegmentRegistration, HostReflectionPolicy,
    HostSchemaEpoch, HostTypeOwnership, HostTypeRegistration, LanguageProfile, PathAccess,
    ResourcePolicy, TypeKind, TypeRegistration,
    host::{HostError, HostFunction},
    value::Value,
};

fn compile_artifact(
    engine: &KagariEngine,
    name: &str,
    source: &str,
) -> kagari_embed::BytecodeArtifact {
    engine
        .compile_to_artifact(
            SourceFile::new(name, source),
            CompileOptions::default(),
            ArtifactOptions::default(),
        )
        .expect("source should compile")
}

fn host_call_context() -> ExecutionContext {
    ExecutionContext {
        language_profile: LanguageProfile {
            allow_host_calls: true,
            ..LanguageProfile::default()
        },
        capabilities: CapabilitySet {
            host_calls: true,
            ..CapabilitySet::default()
        },
        host_policy: HostExposurePolicy {
            allowed_host_functions: vec!["host.player".to_owned()],
            allowed_host_types: vec!["game.Player".to_owned()],
            allow_host_path_reads: true,
            ..HostExposurePolicy::default()
        },
        ..ExecutionContext::default()
    }
}

fn register_embedding_host_path_runtime(
    runtime: &mut KagariRuntime,
    path_access: PathAccess,
    capability_requirements: CapabilitySet,
) -> HostPathDescriptorId {
    let i32_id = runtime
        .runtime_mut()
        .types()
        .register(TypeRegistration {
            abi_fingerprint: AbiFingerprint(101),
            ..TypeRegistration::new("i32", TypeKind::Primitive)
        })
        .unwrap();
    let host_type = HostTypeRegistration::new(player_type_declaration(), "game.Player");
    let player_id = runtime.register_host_type(host_type).unwrap();
    let root = runtime
        .runtime_mut()
        .register_host_root(HostObjectId(1), player_id, HostSchemaEpoch::new(0))
        .unwrap();
    runtime
        .register_host_function(HostFunction::new(
            kagari_common::host_interface::HostFunctionDeclaration::new(
                "host.player",
                vec![],
                kagari_common::host_interface::HostValueType::opaque("game.Player"),
            ),
            move |_, _| Ok(Value::HostRoot(root)),
        ))
        .unwrap();

    let hp_declaration = runtime
        .runtime()
        .host()
        .host_type(player_id)
        .unwrap()
        .declaration
        .fields
        .iter()
        .find(|field| field.name == "hp")
        .unwrap()
        .id
        .clone();
    let descriptor_id = runtime
        .runtime_mut()
        .register_host_path_descriptor(HostPathDescriptorRegistration {
            root_type: player_id,
            result_type: i32_id,
            segments: vec![HostPathSegmentRegistration::Field {
                declaration: hp_declaration,
            }],
            access: path_access,
            schema_epoch: HostSchemaEpoch::new(0),
            capability_requirements,
        })
        .unwrap();
    assert_eq!(descriptor_id.index(), 0);

    runtime
        .runtime_mut()
        .register_host_path_adapter(
            descriptor_id,
            HostPathAdapter::new()
                .with_read(|_, _| Ok(Value::I32(10)))
                .with_prepare_write(|_, _, record| {
                    if matches!(record.new_value, Value::I32(_)) {
                        Ok(kagari_runtime::host::PreparedHostPathWrite::new(|| {}))
                    } else {
                        Err(HostError::new("hp expects i32"))
                    }
                }),
        )
        .unwrap();
    descriptor_id
}

fn host_path_artifact(
    contract: &kagari_runtime::HostPathDescriptor,
    source_name: &str,
    path_debug_name: &str,
    instructions: Vec<BytecodeInstruction>,
    registers: Vec<ValueType>,
    return_type: ValueType,
) -> BytecodeArtifact {
    let constants = instructions
        .iter()
        .filter_map(|instruction| match instruction {
            BytecodeInstruction::LoadConst { constant, .. } => Some(constant.clone()),
            _ => None,
        })
        .collect();
    let metadata = FunctionMetadata {
        return_type,
        roots: kagari_ir::bytecode::RootSlotLayout::from_types(&[], &registers),
        registers,
        ..FunctionMetadata::default()
    };
    KbcArtifact::from_program(
        kagari_ir::bytecode::BytecodeProgram {
            root: kagari_ir::bytecode::ModuleRef::new(0),
            modules: vec![BytecodeModule {
                host_interface: kagari_common::host_interface::HostInterface {
                    paths: vec![],
                    types: if instructions.iter().any(|instruction| {
                        matches!(
                            instruction,
                            BytecodeInstruction::Call {
                                callee: CallTarget::HostFunction(_),
                                ..
                            }
                        )
                    }) {
                        vec![player_type_declaration()]
                    } else {
                        Vec::new()
                    },
                    functions: if instructions.iter().any(|instruction| {
                        matches!(
                            instruction,
                            BytecodeInstruction::Call {
                                callee: CallTarget::HostFunction(_),
                                ..
                            }
                        )
                    }) {
                        vec![kagari_common::host_interface::HostFunctionDeclaration::new(
                            "host.player",
                            vec![],
                            kagari_common::host_interface::HostValueType::opaque("game.Player"),
                        )]
                    } else {
                        vec![]
                    },
                },
                identity: kagari_common::identity::ModuleIdentity::single_file(source_name),
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
                    identity: None,
                    name: "main".to_owned(),
                    params: metadata.params.clone(),
                    return_type: metadata.return_type,
                    effects: metadata.effects,
                }],
                functions: vec![BytecodeFunction {
                    id: FunctionRef::new(0),
                    identity: None,
                    name: "main".to_owned(),
                    parameter_count: 0,
                    register_count: metadata.registers.len() as u16,
                    local_count: 0,
                    metadata,
                    instructions,
                }],
                ..BytecodeModule::default()
            }],
        },
        ArtifactBuildOptions::default(),
    )
    .unwrap()
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
            first,
            LoadOptions {
                module_name: Some("game.main".to_owned()),
                ..LoadOptions::default()
            },
        )
        .expect("module should load");
    let report = runtime
        .execute(&loaded, "main", &[], &context)
        .expect("entry should execute");
    assert_eq!(report.return_value, Value::I32(1));

    let reloaded = runtime
        .reload_program(
            &loaded,
            second,
            ReloadOptions {
                module_name: Some("game.main".to_owned()),
                ..ReloadOptions::default()
            },
        )
        .expect("compatible module should reload");
    assert_eq!(reloaded.id, loaded.id);
    assert_eq!(reloaded.epoch.0, loaded.epoch.0 + 1);

    let report = runtime
        .execute(&reloaded, "main", &[], &context)
        .expect("reloaded entry should execute");
    assert_eq!(report.return_value, Value::I32(2));
}

#[test]
fn compile_failures_return_structured_diagnostics() {
    let engine = KagariEngine::default();
    let error = engine
        .compile_source(
            SourceFile::new("bad.kgr", "fn main( -> i32 { 1 }"),
            CompileOptions::default(),
        )
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
        .compile_source(
            SourceFile::new("bad_type.kgr", "fn main() -> Missing { 1 }"),
            CompileOptions::default(),
        )
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
fn execution_context_resource_limits_surface_as_runtime_failures() {
    let engine = KagariEngine::default();
    let context = ExecutionContext {
        resources: ResourcePolicy {
            max_instruction_steps: Some(1),
            ..ResourcePolicy::default()
        },
        ..ExecutionContext::default()
    };
    let artifact = compile_artifact(&engine, "limited.kgr", "fn main() -> i32 { 1 }");
    let mut runtime = engine.runtime(context.clone());
    let loaded = runtime
        .load_program(
            artifact,
            LoadOptions {
                module_name: Some("limited".to_owned()),
                ..LoadOptions::default()
            },
        )
        .expect("module should load");

    let error = runtime
        .execute(&loaded, "main", &[], &context)
        .expect_err("execution should hit context resource limit");

    assert_eq!(error.code(), "KG_RUNTIME_RESOURCE_LIMIT_EXCEEDED");
    assert!(matches!(
        error,
        EmbeddingError::Runtime {
            kind: RuntimeFailureKind::ResourceLimitExceeded,
            ..
        }
    ));
}

#[test]
fn each_execute_applies_its_context_budget_and_cancellation_without_changing_runtime_defaults() {
    let engine = KagariEngine::default();
    let mut runtime = engine.runtime(ExecutionContext::default());
    let artifact = compile_artifact(&engine, "scoped.kgr", "fn main() -> i32 { 42 }");
    let loaded = runtime
        .load_program(artifact, LoadOptions::default())
        .unwrap();
    let mut context = ExecutionContext::default();
    context.resources.max_instruction_steps = Some(0);
    assert_eq!(
        runtime
            .execute(&loaded, "main", &[], &context)
            .unwrap_err()
            .code(),
        "KG_RUNTIME_RESOURCE_LIMIT_EXCEEDED"
    );
    context.resources.max_instruction_steps = Some(2);
    for _ in 0..2 {
        assert_eq!(
            runtime
                .execute(&loaded, "main", &[], &context)
                .unwrap()
                .return_value,
            Value::I32(42)
        );
    }
    assert_eq!(
        runtime.runtime().resources().counters().instruction_steps,
        4
    );
    assert_eq!(
        runtime.runtime().resources().policy().max_instruction_steps,
        None
    );
    context.cancellation.cancel();
    assert_eq!(
        runtime
            .execute(&loaded, "main", &[], &context)
            .unwrap_err()
            .code(),
        "KG_RUNTIME_CANCELLED"
    );
    assert_eq!(
        runtime
            .execute(&loaded, "main", &[], &ExecutionContext::default())
            .unwrap()
            .return_value,
        Value::I32(42)
    );
    assert!(!runtime.runtime().is_quarantined());
}

#[test]
fn failed_reload_validation_does_not_publish_new_epoch() {
    let engine = KagariEngine::default();
    let context = ExecutionContext::default();
    let first = compile_artifact(&engine, "reload.kgr", "fn main() -> i32 { 1 }");
    let mut candidate = compile_artifact(&engine, "reload.kgr", "fn main() -> i32 { 2 }");
    candidate.header.runtime_abi_version = "wrong-runtime-abi".to_owned();

    let mut runtime = engine.runtime(context.clone());
    let loaded = runtime
        .load_program(
            first,
            LoadOptions {
                module_name: Some("reload".to_owned()),
                ..LoadOptions::default()
            },
        )
        .expect("module should load");
    let before_count = runtime.runtime().modules().loaded_count();

    let error = runtime
        .reload_program(
            &loaded,
            candidate,
            ReloadOptions {
                module_name: Some("reload".to_owned()),
                ..ReloadOptions::default()
            },
        )
        .expect_err("invalid artifact should not reload");

    assert_eq!(error.code(), "KG_ARTIFACT_RUNTIME_ABI_MISMATCH");
    assert!(matches!(error, EmbeddingError::ReloadValidation { .. }));
    assert_eq!(runtime.runtime().modules().loaded_count(), before_count);
    assert_eq!(
        runtime.runtime().modules().latest("reload").unwrap().epoch,
        loaded.epoch
    );
}

#[test]
fn reload_rejects_typed_path_fingerprint_changes_without_publishing_epoch() {
    let engine = KagariEngine::default();
    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    register_embedding_host_path_runtime(
        &mut runtime,
        PathAccess::ReadWrite,
        CapabilitySet::default(),
    );
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
    );
    let mut changed = contract.clone();
    changed.abi_fingerprint.0 ^= 1;
    let candidate = host_path_artifact(
        &changed,
        "reload_paths.kgr",
        "game.Player.mp",
        vec![BytecodeInstruction::Return(None)],
        vec![],
        ValueType::Unit,
    );

    let loaded = runtime
        .load_program(
            first,
            LoadOptions {
                module_name: Some("reload_paths".to_owned()),
                ..LoadOptions::default()
            },
        )
        .expect("module should load");
    let before_count = runtime.runtime().modules().loaded_count();

    let error = runtime
        .reload_program(
            &loaded,
            candidate,
            ReloadOptions {
                module_name: Some("reload_paths".to_owned()),
                ..ReloadOptions::default()
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
            artifact,
            LoadOptions {
                module_name: Some("args".to_owned()),
                ..LoadOptions::default()
            },
        )
        .expect("module should load");

    let error = runtime
        .execute(&loaded, "main", &[Value::I32(1)], &context)
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
fn execution_context_denies_host_path_mutation_with_structured_error() {
    let engine = KagariEngine::default();
    let context = host_call_context();
    let mut runtime = engine.runtime(context.clone());
    register_embedding_host_path_runtime(
        &mut runtime,
        PathAccess::ReadWrite,
        CapabilitySet::default(),
    );
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
                callee: CallTarget::HostFunction(kagari_ir::bytecode::HostImportId::new(0)),
                args: vec![],
            },
            BytecodeInstruction::LoadConst {
                dst: Register::new(1),
                constant: ConstantOperand::I32(5),
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
    );
    let loaded = runtime
        .load_program(
            artifact,
            LoadOptions {
                module_name: Some("set_path".to_owned()),
                ..LoadOptions::default()
            },
        )
        .expect("module should load");

    let error = runtime
        .execute(&loaded, "main", &[], &context)
        .expect_err("context should deny host path mutation");

    assert_eq!(error.code(), "KG_RUNTIME_CAPABILITY_DENIED");
    assert!(matches!(
        error,
        EmbeddingError::Runtime {
            kind: RuntimeFailureKind::CapabilityDenied,
            ..
        }
    ));
}

#[test]
fn host_path_capability_denials_surface_as_structured_runtime_errors() {
    let engine = KagariEngine::default();
    let context = host_call_context();
    let mut runtime = engine.runtime(context.clone());
    register_embedding_host_path_runtime(
        &mut runtime,
        PathAccess::ReadOnly,
        CapabilitySet {
            reflection_read: true,
            ..CapabilitySet::default()
        },
    );
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
                callee: CallTarget::HostFunction(kagari_ir::bytecode::HostImportId::new(0)),
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
    );
    let loaded = runtime
        .load_program(
            artifact,
            LoadOptions {
                module_name: Some("read_secure_path".to_owned()),
                ..LoadOptions::default()
            },
        )
        .expect("module should load");

    let error = runtime
        .execute(&loaded, "main", &[], &context)
        .expect_err("missing capability should surface through embedding runtime errors");

    assert_eq!(error.code(), "KG_RUNTIME_CAPABILITY_DENIED");
    assert!(matches!(
        error,
        EmbeddingError::Runtime {
            kind: RuntimeFailureKind::CapabilityDenied,
            ..
        }
    ));
}

#[test]
fn execution_context_denies_host_and_reflection_helpers() {
    let engine = KagariEngine::default();
    let print_artifact = compile_artifact(&engine, "print.kgr", r#"fn main() { print("x"); }"#);
    let type_of_artifact = engine
        .compile_to_artifact(
            SourceFile::new("type_of.kgr", r#"fn main() -> String { type_of(7) }"#),
            CompileOptions {
                language_profile: LanguageProfile {
                    allow_reflection: true,
                    ..LanguageProfile::default()
                },
            },
            ArtifactOptions::default(),
        )
        .expect("reflection source should compile with reflection profile");
    let mut runtime = engine.runtime(ExecutionContext::default());
    runtime
        .register_host_function(HostFunction::new(
            kagari_common::host_interface::standard_log(),
            |_, _| unreachable!("denied callback must not run"),
        ))
        .unwrap();
    let print_module = runtime
        .load_program(
            print_artifact,
            LoadOptions {
                module_name: Some("print".to_owned()),
                ..LoadOptions::default()
            },
        )
        .expect("print module should load");
    let type_of_module = runtime
        .load_program(
            type_of_artifact,
            LoadOptions {
                module_name: Some("type_of".to_owned()),
                ..LoadOptions::default()
            },
        )
        .expect("type_of module should load");

    let host_denied = ExecutionContext {
        host_policy: HostExposurePolicy {
            allow_host_functions: false,
            ..HostExposurePolicy::default()
        },
        ..ExecutionContext::default()
    };
    let error = runtime
        .execute(&print_module, "main", &[], &host_denied)
        .expect_err("host function exposure should be denied");
    assert!(matches!(
        error,
        EmbeddingError::Runtime {
            kind: RuntimeFailureKind::CapabilityDenied,
            ..
        }
    ));

    let error = runtime
        .execute(&type_of_module, "main", &[], &ExecutionContext::default())
        .expect_err("reflection is denied by default context");
    assert!(matches!(
        error,
        EmbeddingError::Runtime {
            kind: RuntimeFailureKind::CapabilityDenied,
            ..
        }
    ));

    let reflection_allowed = ExecutionContext {
        language_profile: LanguageProfile {
            allow_reflection: true,
            ..LanguageProfile::default()
        },
        capabilities: CapabilitySet {
            reflection_metadata: true,
            ..CapabilitySet::default()
        },
        ..ExecutionContext::default()
    };
    let report = runtime
        .execute(&type_of_module, "main", &[], &reflection_allowed)
        .expect("reflection metadata should execute when profile and capability allow it");
    assert_eq!(report.return_value, Value::Str("i32".to_owned()));
}

fn player_type_declaration() -> kagari_common::host_interface::HostTypeDeclaration {
    let mut declaration = kagari_common::host_interface::HostTypeDeclaration::new("game.Player");
    declaration.ownership = HostTypeOwnership::HostRoot;
    declaration.path_access = PathAccess::ReadWrite;
    let mut hp = kagari_common::host_interface::HostFieldDeclaration::new(
        &declaration.id,
        "hp",
        kagari_common::host_interface::HostValueType::I32,
    );
    hp.writable = true;
    hp.path_access = PathAccess::ReadWrite;
    declaration.fields.push(hp);
    declaration.reflection = HostReflectionPolicy::Hidden;
    declaration
}
