use super::*;
use kagari_abi::{
    budget::LogicalBudgetCharge,
    callable::CallableImplementation,
    ids::FunctionRef,
    representation::ValueType,
    scalar::BuiltinType,
    types::{AbiType, FunctionAbi, PublicAbiItem},
};
use kagari_bytecode::{
    ArtifactBuildOptions, ArtifactCompatibility, BytecodeFunction, BytecodeInstruction,
    BytecodeModule, BytecodeProgram, ConstantOperand, DependencyFingerprint, FunctionMetadata,
    KbcArtifact,
};

#[test]
fn corrupted_collection_root_quarantines_the_runtime() {
    let mut runtime = Runtime::default();
    let loaded = runtime
        .load_program(
            "gc-invariant",
            BytecodeProgram {
                root: kagari_bytecode::ModuleRef::new(0),
                modules: vec![BytecodeModule::default()],
            },
        )
        .unwrap();
    let foreign = Runtime::default().alloc_array(Vec::new()).unwrap();
    runtime.module_instance_mut(&loaded).unwrap().module_slots = vec![value::Value::Array(foreign)];

    let error = runtime.collect_garbage().unwrap_err();
    assert_eq!(error.kind(), RuntimeErrorKind::EngineFault);
    assert!(runtime.is_quarantined());
    assert_eq!(
        runtime.collect_garbage().unwrap_err().kind(),
        RuntimeErrorKind::EngineFault
    );
}

#[test]
fn retained_module_state_borrow_quarantines_on_reentry_without_panicking() {
    let mut runtime = Runtime::default();
    let loaded = runtime
        .load_program(
            "borrowed-module",
            BytecodeProgram {
                root: kagari_bytecode::ModuleRef::new(0),
                modules: vec![BytecodeModule::default()],
            },
        )
        .unwrap();
    let held = runtime.module_instance_mut(&loaded).unwrap();
    let error = runtime.module_instance_mut(&loaded).unwrap_err();
    assert_eq!(error.kind(), RuntimeErrorKind::EngineFault);
    assert!(runtime.is_quarantined());
    drop(held);
}

fn module_with_public_function(return_type: BuiltinType) -> BytecodeModule {
    BytecodeModule {
        public_items: vec![PublicAbiItem::Function(FunctionAbi {
            method_policy: Default::default(),
            implementation: CallableImplementation::Script,
            name: "main".to_owned(),
            generic_params: Vec::new(),
            bounds: Vec::new(),
            params: Vec::new(),
            return_type: AbiType::Builtin(return_type),
        })],
        ..BytecodeModule::default()
    }
}

fn module_with_public_function_and_constant(
    return_type: BuiltinType,
    value: i32,
) -> BytecodeModule {
    let mut module = module_with_public_function(return_type);
    module.constants.push(ConstantOperand::I32(value));
    module
}

fn module_with_executable_function() -> BytecodeModule {
    let metadata = FunctionMetadata {
        instruction_budgets: vec![LogicalBudgetCharge::Step; 1],
        return_type: ValueType::Unit,
        ..FunctionMetadata::default()
    };
    BytecodeModule {
        types: vec![ValueType::Unit],
        function_table: vec![kagari_bytecode::FunctionRecord {
            id: FunctionRef::new(0),
            identity: None,
            name: "main".to_owned(),
            params: metadata.params.clone(),
            return_type: metadata.return_type,
            effects: metadata.effects,
        }],
        functions: vec![BytecodeFunction {
            id: FunctionRef::new(0),
            name: "main".to_owned(),
            metadata,
            instructions: vec![BytecodeInstruction::Return(None)],
            ..BytecodeFunction::default()
        }],
        ..BytecodeModule::default()
    }
}

#[test]
fn load_rejects_missing_function_terminator_before_publication() {
    let mut module = module_with_executable_function();
    module.functions[0].instructions.clear();
    let mut runtime = Runtime::default();
    let error = runtime
        .load_program(
            "missing-return",
            BytecodeProgram {
                root: kagari_bytecode::ModuleRef::new(0),
                modules: vec![module],
            },
        )
        .unwrap_err();
    assert_eq!(error.kind(), RuntimeErrorKind::ModuleValidation);
    assert!(runtime.modules().latest("missing-return").is_none());
}

fn artifact_with_loader_fingerprints() -> KbcArtifact {
    let dependency = BytecodeModule {
        identity: kagari_common::identity::ModuleIdentity::single_file("pkg/dependency"),
        ..Default::default()
    };
    let mut root = module_with_public_function(BuiltinType::I32);
    root.dependencies = vec![kagari_bytecode::ModuleRef::new(0)];
    KbcArtifact::from_program(
        BytecodeProgram {
            root: kagari_bytecode::ModuleRef::new(1),
            modules: vec![dependency, root],
        },
        ArtifactBuildOptions {
            security_profile: Some("dev".into()),
            ..Default::default()
        },
    )
    .unwrap()
}
fn compatibility_for_artifact(artifact: &KbcArtifact) -> ArtifactCompatibility {
    ArtifactCompatibility {
        module_identity: Some(artifact.header.module_identity.clone()),
        dependency_fingerprints: Some(artifact.verification.loader.dependency_fingerprints.clone()),
        security_profile: artifact.verification.loader.security_profile.clone(),
        ..ArtifactCompatibility::default()
    }
}

fn debug_security(capabilities: CapabilitySet) -> SecurityContext {
    SecurityContext {
        profile: LanguageProfile {
            allow_debugger: true,
            ..LanguageProfile::default()
        },
        capabilities,
    }
}

#[test]
fn load_module_reports_module_resource_limit() {
    let mut runtime = Runtime::new(RuntimeConfig {
        resources: ResourcePolicy {
            max_modules: Some(1),
            ..ResourcePolicy::default()
        },
        ..RuntimeConfig::default()
    });

    runtime
        .load_program(
            "first",
            kagari_bytecode::BytecodeProgram {
                root: kagari_bytecode::ModuleRef::new(0),
                modules: vec![BytecodeModule::default()],
            },
        )
        .unwrap();
    let error = runtime
        .load_program(
            "second",
            kagari_bytecode::BytecodeProgram {
                root: kagari_bytecode::ModuleRef::new(0),
                modules: vec![BytecodeModule::default()],
            },
        )
        .unwrap_err();

    assert_eq!(error.kind(), RuntimeErrorKind::ResourceLimitExceeded);
    assert_eq!(runtime.resources().counters().loaded_modules, 1);
}

#[test]
fn reload_publishes_valid_candidate_after_validation() {
    let mut runtime = Runtime::default();
    let loaded = runtime
        .load_program(
            "reloadable",
            kagari_bytecode::BytecodeProgram {
                root: kagari_bytecode::ModuleRef::new(0),
                modules: vec![module_with_public_function(BuiltinType::I32)],
            },
        )
        .expect("module should load");

    let reloaded = runtime
        .stage_reload_program(
            &loaded,
            "reloadable",
            kagari_bytecode::BytecodeProgram {
                root: kagari_bytecode::ModuleRef::new(0),
                modules: vec![module_with_public_function(BuiltinType::I32)],
            },
        )
        .and_then(|candidate| runtime.publish_staged_reload(candidate))
        .expect("compatible module should reload");

    assert_eq!(reloaded.id, loaded.id);
    assert_eq!(reloaded.epoch.0, loaded.epoch.0 + 1);
    assert_eq!(
        runtime.modules().latest("reloadable").unwrap().epoch,
        reloaded.epoch
    );
}

#[test]
fn reload_rejects_public_abi_changes_before_publication() {
    let mut runtime = Runtime::default();
    let loaded = runtime
        .load_program(
            "reloadable",
            kagari_bytecode::BytecodeProgram {
                root: kagari_bytecode::ModuleRef::new(0),
                modules: vec![module_with_public_function(BuiltinType::I32)],
            },
        )
        .expect("module should load");
    let before_count = runtime.modules().loaded_count();

    let error = runtime
        .stage_reload_program(
            &loaded,
            "reloadable",
            kagari_bytecode::BytecodeProgram {
                root: kagari_bytecode::ModuleRef::new(0),
                modules: vec![module_with_public_function(BuiltinType::String)],
            },
        )
        .and_then(|candidate| runtime.publish_staged_reload(candidate))
        .expect_err("public ABI change should reject reload");

    assert_eq!(error.code(), "KG_RELOAD_PUBLIC_ABI_FINGERPRINT_MISMATCH");
    assert!(matches!(
        error,
        ReloadValidationError::PublicAbiFingerprintMismatch
    ));
    assert_eq!(runtime.modules().loaded_count(), before_count);
    assert_eq!(
        runtime.modules().latest("reloadable").unwrap().epoch,
        loaded.epoch
    );
}

#[test]
fn reload_rejects_stale_active_epoch_before_publication() {
    let mut runtime = Runtime::default();
    let first = runtime
        .load_program(
            "reloadable",
            kagari_bytecode::BytecodeProgram {
                root: kagari_bytecode::ModuleRef::new(0),
                modules: vec![module_with_public_function(BuiltinType::I32)],
            },
        )
        .expect("module should load");
    let second = runtime
        .stage_reload_program(
            &first,
            "reloadable",
            kagari_bytecode::BytecodeProgram {
                root: kagari_bytecode::ModuleRef::new(0),
                modules: vec![module_with_public_function(BuiltinType::I32)],
            },
        )
        .and_then(|candidate| runtime.publish_staged_reload(candidate))
        .expect("compatible module should reload");
    let before_count = runtime.modules().loaded_count();

    let error = runtime
        .stage_reload_program(
            &first,
            "reloadable",
            kagari_bytecode::BytecodeProgram {
                root: kagari_bytecode::ModuleRef::new(0),
                modules: vec![module_with_public_function(BuiltinType::I32)],
            },
        )
        .and_then(|candidate| runtime.publish_staged_reload(candidate))
        .expect_err("stale active epoch should reject reload");

    assert_eq!(error.code(), "KG_RELOAD_MODULE_NOT_ACTIVE");
    assert!(matches!(
        error,
        ReloadValidationError::ModuleNotActive {
            expected,
            active: Some(active),
            ..
        } if expected == first.epoch && active == second.epoch
    ));
    assert_eq!(runtime.modules().loaded_count(), before_count);
    assert_eq!(
        runtime.modules().latest("reloadable").unwrap().epoch,
        second.epoch
    );
}

#[test]
fn reload_resource_failure_preserves_active_epoch() {
    let mut runtime = Runtime::new(RuntimeConfig {
        resources: ResourcePolicy {
            max_modules: Some(1),
            ..ResourcePolicy::default()
        },
        ..RuntimeConfig::default()
    });
    let loaded = runtime
        .load_program(
            "reloadable",
            kagari_bytecode::BytecodeProgram {
                root: kagari_bytecode::ModuleRef::new(0),
                modules: vec![module_with_public_function(BuiltinType::I32)],
            },
        )
        .expect("module should load");

    let error = runtime
        .stage_reload_program(
            &loaded,
            "reloadable",
            kagari_bytecode::BytecodeProgram {
                root: kagari_bytecode::ModuleRef::new(0),
                modules: vec![module_with_public_function(BuiltinType::I32)],
            },
        )
        .and_then(|candidate| runtime.publish_staged_reload(candidate))
        .expect_err("resource limit should reject reload before publication");

    assert_eq!(error.code(), "KG_RUNTIME_RESOURCE_LIMIT_EXCEEDED");
    assert!(matches!(
        error,
        ReloadValidationError::Runtime(ref error)
            if error.kind() == RuntimeErrorKind::ResourceLimitExceeded
    ));
    assert_eq!(runtime.modules().loaded_count(), 1);
    assert_eq!(
        runtime.modules().latest("reloadable").unwrap().epoch,
        loaded.epoch
    );
}

#[test]
fn reload_artifact_validates_loader_compatibility_before_publication() {
    let artifact = artifact_with_loader_fingerprints();
    let mut runtime = Runtime::default();
    let loaded = runtime
        .load_program("reloadable", artifact.program.clone())
        .expect("module should load");
    let before_count = runtime.modules().loaded_count();

    let error = runtime
        .stage_reload_artifact(
            &loaded,
            "reloadable",
            artifact,
            &ArtifactCompatibility {
                dependency_fingerprints: Some(Vec::new()),
                ..Default::default()
            },
        )
        .and_then(|candidate| runtime.publish_staged_reload(candidate))
        .expect_err("loader compatibility mismatch should reject reload");

    assert_eq!(error.code(), "KG_ARTIFACT_DEPENDENCY_FINGERPRINT_MISMATCH");
    assert!(matches!(
        error,
        ReloadValidationError::Artifact(
            kagari_bytecode::ArtifactValidationError::DependencyFingerprintMismatch
        )
    ));
    assert_eq!(runtime.modules().loaded_count(), before_count);
    assert_eq!(
        runtime.modules().latest("reloadable").unwrap().epoch,
        loaded.epoch
    );
}

#[test]
fn reload_artifact_publishes_after_loader_and_reload_validation() {
    let artifact = artifact_with_loader_fingerprints();
    let compatibility = compatibility_for_artifact(&artifact);
    let mut runtime = Runtime::default();
    let loaded = runtime
        .load_program("reloadable", artifact.program.clone())
        .expect("module should load");

    let reloaded = runtime
        .stage_reload_artifact(&loaded, "reloadable", artifact, &compatibility)
        .and_then(|candidate| runtime.publish_staged_reload(candidate))
        .expect("compatible artifact should reload");

    assert_eq!(reloaded.id, loaded.id);
    assert_eq!(reloaded.epoch.0, loaded.epoch.0 + 1);
    assert_eq!(
        runtime.modules().latest("reloadable").unwrap().epoch,
        reloaded.epoch
    );
}

#[test]
fn staged_reload_failure_and_stale_publication_preserve_the_active_entry() {
    let artifact = artifact_with_loader_fingerprints();
    let mut runtime = Runtime::default();
    let baseline = runtime
        .load_program("reloadable", artifact.program.clone())
        .unwrap();
    let stage = |runtime: &mut Runtime| {
        let prepared = runtime
            .prepare_reload(
                &baseline,
                "reloadable".into(),
                VerifiedProgram::new(artifact.program.clone()).unwrap(),
            )
            .unwrap();
        runtime.stage_prepared_reload(prepared).unwrap()
    };
    let failed = stage(&mut runtime);
    let failed_key = failed.program.module().key();
    assert_eq!(
        runtime.modules.latest("reloadable").unwrap().key(),
        baseline.key()
    );
    drop(failed);
    assert!(runtime.modules.loaded(failed_key).is_none());
    assert_eq!(
        runtime.resources.counters().loaded_modules,
        baseline.members().count()
    );

    let candidate = stage(&mut runtime);
    let stale = stage(&mut runtime);
    let stale_key = stale.program.module().key();
    assert_eq!(
        runtime.modules.latest("reloadable").unwrap().key(),
        baseline.key()
    );
    let current = runtime.publish_staged_reload(candidate).unwrap();
    assert!(matches!(
        runtime.publish_staged_reload(stale),
        Err(ReloadValidationError::ModuleNotActive { .. })
    ));
    assert!(runtime.modules.loaded(stale_key).is_none());
    assert_eq!(
        runtime.modules.latest("reloadable").unwrap().key(),
        current.key()
    );
    assert_eq!(
        runtime.resources.counters().loaded_modules,
        runtime.modules.loaded_count()
    );
    assert!(runtime.module_instance_snapshot(&current).is_some());
    runtime.validate_loaded_module(&baseline).unwrap();
}

#[test]
fn prepared_reload_is_inert_and_rejects_a_stale_publication() {
    let artifact = artifact_with_loader_fingerprints();
    let mut runtime = Runtime::default();
    let baseline = runtime
        .load_program("reloadable", artifact.program.clone())
        .unwrap();
    let before_count = runtime.modules().loaded_count();
    let before_resources = runtime.resources().counters();
    let prepare = |runtime: &Runtime| {
        runtime
            .prepare_reload(
                &baseline,
                "reloadable".into(),
                VerifiedProgram::new(artifact.program.clone()).unwrap(),
            )
            .unwrap()
    };
    let candidate = prepare(&runtime);
    let stale = prepare(&runtime);
    assert_eq!(runtime.modules().loaded_count(), before_count);
    assert_eq!(runtime.resources().counters(), before_resources);
    assert_eq!(
        runtime.modules().latest("reloadable").unwrap().key(),
        baseline.key()
    );
    let current = runtime
        .stage_prepared_reload(candidate)
        .and_then(|candidate| runtime.publish_staged_reload(candidate))
        .unwrap();
    let published_resources = runtime.resources().counters();
    let published_count = runtime.modules().loaded_count();
    assert!(matches!(
        runtime
            .stage_prepared_reload(stale)
            .and_then(|candidate| runtime.publish_staged_reload(candidate)),
        Err(ReloadValidationError::ModuleNotActive { .. })
    ));
    assert_eq!(
        runtime.modules().latest("reloadable").unwrap().key(),
        current.key()
    );
    assert_eq!(runtime.modules().loaded_count(), published_count);
    assert_eq!(runtime.resources().counters(), published_resources);
    runtime.validate_loaded_module(&baseline).unwrap();
}

#[test]
fn reload_invalidates_interpreter_caches_with_stale_dependency_fingerprints() {
    let dependency_v1 = KbcArtifact::from_program(
        kagari_bytecode::BytecodeProgram {
            root: kagari_bytecode::ModuleRef::new(0),
            modules: vec![module_with_public_function_and_constant(
                BuiltinType::I32,
                1,
            )],
        },
        ArtifactBuildOptions::default(),
    )
    .unwrap();
    let dependency_v2 = KbcArtifact::from_program(
        kagari_bytecode::BytecodeProgram {
            root: kagari_bytecode::ModuleRef::new(0),
            modules: vec![module_with_public_function_and_constant(
                BuiltinType::I32,
                2,
            )],
        },
        ArtifactBuildOptions::default(),
    )
    .unwrap();
    let dependency_v1_snapshot = ReloadDependencySnapshot::from_artifact(&dependency_v1);
    let dependency_v2_compatibility = compatibility_for_artifact(&dependency_v2);
    let mut runtime = Runtime::default();
    let dependency = runtime
        .load_program("dependency", dependency_v1.program.clone())
        .expect("dependency should load");
    let consumer = runtime
        .load_program(
            "consumer",
            kagari_bytecode::BytecodeProgram {
                root: kagari_bytecode::ModuleRef::new(0),
                modules: vec![module_with_public_function(BuiltinType::I32)],
            },
        )
        .expect("consumer should load");
    let mut consumer_snapshot = ReloadDependencySnapshot::from_bytecode(&consumer.bytecode);
    consumer_snapshot
        .dependency_fingerprints
        .push(DependencyFingerprint {
            module_id: dependency_v1.header.module_identity.clone(),
            fingerprint: dependency_v1_snapshot.module_fingerprint,
        });

    let interpreter_cache = runtime
        .register_interpreter_cache(consumer.key(), None, consumer_snapshot.clone())
        .expect("interpreter cache should register");
    let function_cache = runtime
        .register_interpreter_cache(consumer.key(), Some(FunctionRef::new(0)), consumer_snapshot)
        .expect("function cache should register");

    assert!(runtime.interpreter_cache(interpreter_cache).is_some());
    assert!(runtime.interpreter_cache(function_cache).is_some());
    assert_eq!(
        runtime
            .modules()
            .retention_counts(consumer.key())
            .compiled_artifacts,
        0
    );

    runtime
        .stage_reload_artifact(
            &dependency,
            "dependency",
            dependency_v2,
            &dependency_v2_compatibility,
        )
        .and_then(|candidate| runtime.publish_staged_reload(candidate))
        .expect("compatible dependency implementation should reload");

    assert!(runtime.interpreter_cache(interpreter_cache).is_none());
    assert!(runtime.interpreter_cache(function_cache).is_none());
    assert!(runtime.interpreter_caches.get(interpreter_cache).is_none());
    assert!(runtime.interpreter_caches.get(function_cache).is_none());
    assert_eq!(
        runtime
            .modules()
            .retention_counts(consumer.key())
            .compiled_artifacts,
        0
    );
}

#[test]
fn reload_invalidates_interpreter_cache_for_new_epoch_even_when_public_abi_is_stable() {
    let mut runtime = Runtime::default();
    let loaded = runtime
        .load_program(
            "reloadable",
            kagari_bytecode::BytecodeProgram {
                root: kagari_bytecode::ModuleRef::new(0),
                modules: vec![module_with_public_function_and_constant(
                    BuiltinType::I32,
                    1,
                )],
            },
        )
        .expect("module should load");
    let artifact = runtime
        .register_interpreter_cache(
            loaded.key(),
            None,
            ReloadDependencySnapshot::from_bytecode(&loaded.bytecode),
        )
        .expect("function cache should register");

    assert!(runtime.interpreter_cache(artifact).is_some());
    assert_eq!(
        runtime
            .modules()
            .retention_counts(loaded.key())
            .compiled_artifacts,
        0
    );

    let reloaded = runtime
        .stage_reload_program(
            &loaded,
            "reloadable",
            kagari_bytecode::BytecodeProgram {
                root: kagari_bytecode::ModuleRef::new(0),
                modules: vec![module_with_public_function_and_constant(
                    BuiltinType::I32,
                    2,
                )],
            },
        )
        .and_then(|candidate| runtime.publish_staged_reload(candidate))
        .expect("implementation-only reload should publish a new epoch");

    assert_eq!(reloaded.id, loaded.id);
    assert_eq!(reloaded.epoch.0, loaded.epoch.0 + 1);
    assert!(runtime.interpreter_cache(artifact).is_none());
    assert!(runtime.interpreter_caches.get(artifact).is_none());
    assert_eq!(
        runtime
            .modules()
            .retention_counts(loaded.key())
            .compiled_artifacts,
        0
    );
}

#[test]
fn failed_reload_does_not_invalidate_interpreter_caches() {
    let dependency_v1 = artifact_with_loader_fingerprints();
    let dependency_v1_snapshot = ReloadDependencySnapshot::from_artifact(&dependency_v1);
    let mut dependency_v2 = dependency_v1.clone();
    dependency_v2.program.modules[dependency_v2.program.root.index()]
        .constants
        .push(ConstantOperand::I32(2));
    let mut runtime = Runtime::default();
    let dependency = runtime
        .load_program("dependency", dependency_v1.program.clone())
        .expect("dependency should load");
    let consumer = runtime
        .load_program(
            "consumer",
            kagari_bytecode::BytecodeProgram {
                root: kagari_bytecode::ModuleRef::new(0),
                modules: vec![module_with_public_function(BuiltinType::I32)],
            },
        )
        .expect("consumer should load");
    let mut consumer_snapshot = ReloadDependencySnapshot::from_bytecode(&consumer.bytecode);
    consumer_snapshot
        .dependency_fingerprints
        .push(DependencyFingerprint {
            module_id: dependency_v1.header.module_identity.clone(),
            fingerprint: dependency_v1_snapshot.module_fingerprint,
        });

    let artifact = runtime
        .register_interpreter_cache(consumer.key(), None, consumer_snapshot)
        .expect("interpreter cache should register");

    let error = runtime
        .stage_reload_artifact(
            &dependency,
            "dependency",
            dependency_v2,
            &ArtifactCompatibility::default(),
        )
        .and_then(|candidate| runtime.publish_staged_reload(candidate))
        .expect_err("loader compatibility mismatch should reject reload");

    assert!(matches!(
        error,
        ReloadValidationError::Artifact(
            kagari_bytecode::ArtifactValidationError::ContentHashMismatch
        )
    ));
    assert!(runtime.interpreter_cache(artifact).is_some());
    assert!(runtime.interpreter_caches.get(artifact).is_some());
    assert_eq!(
        runtime
            .modules()
            .retention_counts(consumer.key())
            .compiled_artifacts,
        0
    );
}

#[test]
fn heap_mutations_update_runtime_resource_counters() {
    let runtime = Runtime::default();
    let array = runtime
        .gc()
        .alloc_array(vec![value::Value::I32(1)])
        .unwrap();
    runtime
        .gc()
        .array_push(array, value::Value::I32(2))
        .unwrap();

    let counters = runtime.resources().counters();

    assert_eq!(counters.current_heap_units, 3);
    assert_eq!(counters.peak_heap_units, 3);
}

#[test]
fn builtin_map_and_set_allocations_update_resource_counters() {
    let runtime = Runtime::default();
    let map = runtime
        .alloc_map(vec![
            (value::Value::Str("hp".to_owned()), value::Value::I32(100)),
            (value::Value::Str("mp".to_owned()), value::Value::I32(20)),
        ])
        .unwrap();
    let set = runtime
        .alloc_set(vec![
            value::Value::Str("ready".to_owned()),
            value::Value::Str("visible".to_owned()),
        ])
        .unwrap();

    assert_eq!(runtime.gc().map_len(map), Some(2));
    assert_eq!(runtime.gc().set_len(set), Some(2));

    let counters = runtime.resources().counters();
    assert_eq!(counters.allocation_units, 6);
    assert_eq!(counters.current_heap_units, 6);
    assert_eq!(counters.peak_heap_units, 6);
}

#[test]
fn exposes_runtime_type_registry_and_security_context() {
    let runtime = Runtime::new(RuntimeConfig {
        security: SecurityContext {
            capabilities: CapabilitySet {
                reflection_metadata: true,
                reflection_read: true,
                ..CapabilitySet::default()
            },
            profile: LanguageProfile {
                allow_reflection: true,
                ..LanguageProfile::default()
            },
        },
        ..RuntimeConfig::default()
    });
    let type_id = runtime
        .types()
        .register(TypeRegistration {
            abi_fingerprint: AbiFingerprint(99),
            ..TypeRegistration::new("Player", TypeKind::Struct)
        })
        .unwrap();

    assert_eq!(runtime.types().id_by_name("Player"), Some(type_id));
    assert!(runtime.security().allows_reflection_read());
}

#[test]
fn security_context_requires_profile_and_capability_for_runtime_boundaries() {
    let default_runtime = Runtime::default();
    assert_eq!(
        default_runtime
            .validate_reflection_metadata_boundary()
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::CapabilityDenied
    );
    assert_eq!(
        default_runtime
            .validate_reflection_read_boundary()
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::CapabilityDenied
    );
    assert_eq!(
        default_runtime
            .validate_reflection_write_boundary()
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::CapabilityDenied
    );
    assert_eq!(
        default_runtime
            .validate_dynamic_invocation_boundary()
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::CapabilityDenied
    );
    assert_eq!(
        default_runtime
            .validate_downcast_boundary()
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::CapabilityDenied
    );
    assert_eq!(
        default_runtime
            .validate_host_function_boundary("host.missing")
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::CapabilityDenied
    );
    assert_eq!(
        default_runtime
            .validate_path_mutation_boundary()
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::CapabilityDenied
    );
    assert_eq!(
        default_runtime
            .validate_module_loading_boundary()
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::CapabilityDenied
    );
    assert_eq!(
        default_runtime.validate_jit_boundary().unwrap_err().kind(),
        RuntimeErrorKind::CapabilityDenied
    );

    let enabled = Runtime::new(RuntimeConfig {
        security: SecurityContext {
            profile: LanguageProfile {
                allow_host_calls: true,
                allow_path_mutation: true,
                allow_module_loading: true,
                allow_jit: true,
                allow_reflection: true,
                allow_reflection_write: true,
                ..LanguageProfile::default()
            },
            capabilities: CapabilitySet {
                host_calls: true,
                path_mutation: true,
                module_loading: true,
                jit: true,
                reflection_metadata: true,
                reflection_read: true,
                reflection_write: true,
                dynamic_invocation: true,
                downcast: true,
                ..CapabilitySet::default()
            },
        },
        host_exposure: HostExposurePolicy {
            allow_host_functions: true,
            ..HostExposurePolicy::default()
        },
        ..RuntimeConfig::default()
    });

    assert!(
        enabled
            .validate_host_function_boundary("host.missing")
            .is_ok()
    );
    assert!(enabled.validate_reflection_metadata_boundary().is_ok());
    assert!(enabled.validate_reflection_read_boundary().is_ok());
    assert!(enabled.validate_reflection_write_boundary().is_ok());
    assert!(enabled.validate_dynamic_invocation_boundary().is_ok());
    assert!(enabled.validate_downcast_boundary().is_ok());
    assert!(enabled.validate_path_mutation_boundary().is_ok());
    assert!(enabled.validate_module_loading_boundary().is_ok());
    assert!(enabled.validate_jit_boundary().is_ok());
}

#[test]
fn security_restricted_profiles_disable_runtime_boundaries_independently() {
    let capability_only = Runtime::new(RuntimeConfig {
        security: SecurityContext {
            profile: LanguageProfile {
                allow_interface_values: false,
                ..LanguageProfile::default()
            },
            capabilities: CapabilitySet {
                host_calls: true,
                path_mutation: true,
                module_loading: true,
                jit: true,
                reflection_metadata: true,
                reflection_read: true,
                reflection_write: true,
                dynamic_invocation: true,
                downcast: true,
                debug_attach: true,
                debug_breakpoints: true,
                debug_pause: true,
                debug_stack_inspection: true,
                debug_value_inspection: true,
                debug_watch_evaluation: true,
                ..CapabilitySet::default()
            },
        },
        host_exposure: HostExposurePolicy {
            allow_host_functions: true,
            ..HostExposurePolicy::default()
        },
        ..RuntimeConfig::default()
    });

    let denied = [
        capability_only.validate_host_function_boundary("host.open"),
        capability_only.validate_path_mutation_boundary(),
        capability_only.validate_module_loading_boundary(),
        capability_only.validate_jit_boundary(),
        capability_only.validate_reflection_metadata_boundary(),
        capability_only.validate_reflection_read_boundary(),
        capability_only.validate_reflection_write_boundary(),
        capability_only.validate_dynamic_invocation_boundary(),
        capability_only.validate_downcast_boundary(),
        capability_only.validate_debug_attach_boundary(),
        capability_only.validate_debug_breakpoint_boundary(),
        capability_only.validate_debug_pause_boundary(),
        capability_only.validate_debug_stack_inspection_boundary(),
        capability_only.validate_debug_value_inspection_boundary(),
        capability_only.validate_debug_watch_evaluation_boundary(),
    ];

    for result in denied {
        assert_eq!(
            result
                .expect_err("restricted profile should deny boundary")
                .kind(),
            RuntimeErrorKind::CapabilityDenied
        );
    }
}

#[test]
fn downcast_gate_is_independent_from_reflection_gates() {
    let downcast_only = Runtime::new(RuntimeConfig {
        security: SecurityContext {
            capabilities: CapabilitySet {
                downcast: true,
                ..CapabilitySet::default()
            },
            ..SecurityContext::default()
        },
        ..RuntimeConfig::default()
    });

    assert!(downcast_only.validate_downcast_boundary().is_ok());
    assert_eq!(
        downcast_only
            .validate_reflection_metadata_boundary()
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::CapabilityDenied
    );

    let metadata_only = Runtime::new(RuntimeConfig {
        security: SecurityContext {
            profile: LanguageProfile {
                allow_reflection: true,
                ..LanguageProfile::default()
            },
            capabilities: CapabilitySet {
                reflection_metadata: true,
                ..CapabilitySet::default()
            },
        },
        ..RuntimeConfig::default()
    });

    assert!(
        metadata_only
            .validate_reflection_metadata_boundary()
            .is_ok()
    );
    assert_eq!(
        metadata_only
            .validate_downcast_boundary()
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::CapabilityDenied
    );
}

#[test]
fn debug_visibility_respects_host_value_policy() {
    let host_value = value::Value::HostRoot(HostRootHandle::new(
        Default::default(),
        HostObjectId(1),
        TypeId::new(0),
        HostSchemaEpoch::new(0),
        AbiFingerprint(1),
    ));
    let runtime_without_host_debug = Runtime::new(RuntimeConfig {
        security: debug_security(CapabilitySet {
            debug_value_inspection: true,
            ..CapabilitySet::default()
        }),
        ..RuntimeConfig::default()
    });

    assert_eq!(
        runtime_without_host_debug
            .validate_debug_value_visible(&host_value)
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::CapabilityDenied
    );

    let runtime_with_host_debug = Runtime::new(RuntimeConfig {
        security: debug_security(CapabilitySet {
            debug_value_inspection: true,
            debug_host_value_inspection: true,
            ..CapabilitySet::default()
        }),
        debug_visibility: DebugVisibilityPolicy {
            allow_host_value_inspection: true,
            ..DebugVisibilityPolicy::default()
        },
        ..RuntimeConfig::default()
    });

    assert!(
        runtime_with_host_debug
            .validate_debug_value_visible(&host_value)
            .is_ok()
    );
}
