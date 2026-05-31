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
        PathRecord, Register, RuntimeHelper,
    },
    module::ValueType,
};
use kagari_runtime::{
    AbiFingerprint, CapabilitySet, FieldMetadataId, HostObjectId, HostPathAdapter,
    HostPathDescriptorId, HostPathDescriptorRegistration, HostPathSegment, HostReflectionPolicy,
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
    let mut host_type = HostTypeRegistration::new("game.Player", "game.Player");
    host_type.ownership = HostTypeOwnership::HostRoot;
    host_type.path_access = PathAccess::ReadWrite;
    host_type.reflection = HostReflectionPolicy::Hidden;
    host_type.abi_fingerprint = AbiFingerprint(102);
    let player_id = runtime.register_host_type(host_type).unwrap();
    let root = runtime
        .runtime_mut()
        .register_host_root(HostObjectId(1), player_id, HostSchemaEpoch::new(0))
        .unwrap();
    runtime
        .register_host_function(HostFunction::new(
            "host.player",
            vec![],
            "Player",
            move |_| Ok(Value::HostRoot(root)),
        ))
        .unwrap();

    let descriptor_id = runtime
        .runtime_mut()
        .register_host_path_descriptor(HostPathDescriptorRegistration {
            root_type: player_id,
            result_type: i32_id,
            segments: vec![HostPathSegment::Field {
                name: "hp".to_owned(),
                field_id: FieldMetadataId::new(0),
                owner_type: player_id,
                result_type: i32_id,
                access: path_access,
                abi_fingerprint: AbiFingerprint(103),
            }],
            access: path_access,
            schema_epoch: HostSchemaEpoch::new(0),
            abi_fingerprint: AbiFingerprint(104),
            capability_requirements,
        })
        .unwrap();
    assert_eq!(descriptor_id.index(), 0);

    runtime
        .runtime_mut()
        .register_host_path_adapter(
            descriptor_id,
            HostPathAdapter::new()
                .with_read(|_| Ok(Value::I32(10)))
                .with_write(|_, value| {
                    if matches!(value, Value::I32(_)) {
                        Ok(())
                    } else {
                        Err(HostError::new("hp expects i32"))
                    }
                }),
        )
        .unwrap();
    descriptor_id
}

fn host_path_artifact(
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
        registers,
        ..FunctionMetadata::default()
    };
    KbcArtifact::from_module(
        BytecodeModule {
            module_init: None,
            module_slots: vec![],
            constants,
            types: vec![ValueType::Unit, ValueType::HeapObject, ValueType::I32],
            paths: vec![PathRecord {
                id: PathId::new(0),
                root_ty: ValueType::HeapObject,
                result_ty: ValueType::I32,
                read_only: false,
                debug_name: path_debug_name.to_owned(),
            }],
            function_table: vec![FunctionRecord {
                id: FunctionRef::new(0),
                name: "main".to_owned(),
                params: metadata.params.clone(),
                return_type: metadata.return_type,
                effects: metadata.effects,
            }],
            functions: vec![BytecodeFunction {
                id: FunctionRef::new(0),
                name: "main".to_owned(),
                parameter_count: 0,
                register_count: metadata.registers.len() as u16,
                local_count: 0,
                metadata,
                instructions,
            }],
            ..BytecodeModule::default()
        },
        ArtifactBuildOptions {
            module_identity: kagari_ir::bytecode::ArtifactModuleIdentity::single_file(source_name),
            ..ArtifactBuildOptions::default()
        },
    )
}

#[test]
fn compiles_loads_executes_and_reloads_through_embedding_api() {
    let engine = KagariEngine::default();
    let context = ExecutionContext::default();
    let first = compile_artifact(&engine, "game/main.kgr", "fn main() -> i32 { 1 }");
    let second = compile_artifact(&engine, "game/main.kgr", "fn main() -> i32 { 2 }");

    let mut runtime = engine.runtime(context);
    let loaded = runtime
        .load_module(
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
        .reload_module(
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
            .any(|diagnostic| diagnostic.code.contains("UnknownType"))
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
    let mut runtime = engine.runtime(context);
    let loaded = runtime
        .load_module(
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

    assert!(matches!(
        error,
        EmbeddingError::Runtime {
            kind: RuntimeFailureKind::ResourceLimitExceeded,
            ..
        }
    ));
}

#[test]
fn failed_reload_validation_does_not_publish_new_epoch() {
    let engine = KagariEngine::default();
    let context = ExecutionContext::default();
    let first = compile_artifact(&engine, "reload.kgr", "fn main() -> i32 { 1 }");
    let mut candidate = compile_artifact(&engine, "reload.kgr", "fn main() -> i32 { 2 }");
    candidate.header.runtime_abi_version = "wrong-runtime-abi".to_owned();

    let mut runtime = engine.runtime(context);
    let loaded = runtime
        .load_module(
            first,
            LoadOptions {
                module_name: Some("reload".to_owned()),
                ..LoadOptions::default()
            },
        )
        .expect("module should load");
    let before_count = runtime.runtime().modules().loaded_count();

    let error = runtime
        .reload_module(
            &loaded,
            candidate,
            ReloadOptions {
                module_name: Some("reload".to_owned()),
                ..ReloadOptions::default()
            },
        )
        .expect_err("invalid artifact should not reload");

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
    let first = host_path_artifact(
        "reload_paths.kgr",
        "game.Player.hp",
        vec![BytecodeInstruction::Return(None)],
        vec![],
        ValueType::Unit,
    );
    let candidate = host_path_artifact(
        "reload_paths.kgr",
        "game.Player.mp",
        vec![BytecodeInstruction::Return(None)],
        vec![],
        ValueType::Unit,
    );

    let mut runtime = engine.runtime(context);
    let loaded = runtime
        .load_module(
            first,
            LoadOptions {
                module_name: Some("reload_paths".to_owned()),
                ..LoadOptions::default()
            },
        )
        .expect("module should load");
    let before_count = runtime.runtime().modules().loaded_count();

    let error = runtime
        .reload_module(
            &loaded,
            candidate,
            ReloadOptions {
                module_name: Some("reload_paths".to_owned()),
                ..ReloadOptions::default()
            },
        )
        .expect_err("changed typed path fingerprints should reject reload");

    let EmbeddingError::ReloadValidation { message } = error else {
        panic!("expected reload validation error");
    };
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
    let mut runtime = engine.runtime(context);
    let loaded = runtime
        .load_module(
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
    let context = ExecutionContext::default();
    let artifact = host_path_artifact(
        "set_path.kgr",
        "game.Player.hp",
        vec![
            BytecodeInstruction::Call {
                dst: Some(Register::new(0)),
                callee: CallTarget::RuntimeHelper(RuntimeHelper::HostFunction(
                    "host.player".to_owned(),
                )),
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
        vec![ValueType::HeapObject, ValueType::I32],
        ValueType::Unit,
    );
    let mut runtime = engine.runtime(context);
    register_embedding_host_path_runtime(
        &mut runtime,
        PathAccess::ReadWrite,
        CapabilitySet::default(),
    );
    let loaded = runtime
        .load_module(
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
    let context = ExecutionContext::default();
    let artifact = host_path_artifact(
        "read_secure_path.kgr",
        "game.Player.secure_hp",
        vec![
            BytecodeInstruction::Call {
                dst: Some(Register::new(0)),
                callee: CallTarget::RuntimeHelper(RuntimeHelper::HostFunction(
                    "host.player".to_owned(),
                )),
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
        vec![ValueType::HeapObject, ValueType::I32],
        ValueType::I32,
    );
    let mut runtime = engine.runtime(context);
    register_embedding_host_path_runtime(
        &mut runtime,
        PathAccess::ReadOnly,
        CapabilitySet {
            reflection_read: true,
            ..CapabilitySet::default()
        },
    );
    let loaded = runtime
        .load_module(
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
    let type_of_artifact = compile_artifact(
        &engine,
        "type_of.kgr",
        r#"fn main() -> String { type_of(7) }"#,
    );
    let mut runtime = engine.runtime(ExecutionContext::default());
    let print_module = runtime
        .load_module(
            print_artifact,
            LoadOptions {
                module_name: Some("print".to_owned()),
                ..LoadOptions::default()
            },
        )
        .expect("print module should load");
    let type_of_module = runtime
        .load_module(
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
            reflection_read: true,
            ..CapabilitySet::default()
        },
        ..ExecutionContext::default()
    };
    let report = runtime
        .execute(&type_of_module, "main", &[], &reflection_allowed)
        .expect("reflection read should execute when profile and capability allow it");
    assert_eq!(report.return_value, Value::Str("i32".to_owned()));
}
