use super::*;
use crate::{
    metadata::{AbiFingerprint, TypeKind, TypeRegistration},
    reload::ReloadValidationError,
};
use kagari_bytecode::{
    artifact::{
        ArtifactBuildOptions, ArtifactCompatibility, ArtifactValidationError,
        DependencyFingerprint, KbcArtifact,
    },
    instruction::{BytecodeInstruction, ConstantOperand},
    module::{BytecodeFunction, BytecodeModule, FunctionMetadata, FunctionRecord},
    program::{BytecodeProgram, ModuleRef},
};

use {
    kagari_abi::representation::ValueType,
    kagari_contract::{
        callable::CallableImplementation,
        ids::FunctionRef,
        scalar::BuiltinType,
        types::{FnDecl, PublicItem, Ty},
    },
};

#[test]
fn corrupted_collection_root_quarantines_the_runtime() {
    let mut runtime = Runtime::default();
    let loaded = runtime
        .load_program(
            "gc-invariant",
            BytecodeProgram {
                root: ModuleRef::new(0),
                modules: vec![BytecodeModule::default()],
            },
        )
        .unwrap();
    let mut foreign_runtime = Runtime::default();
    let owner = crate::layout_fixtures::allocation_owner(&mut foreign_runtime);
    let foreign = foreign_runtime
        .alloc_array(&owner, Ty::Builtin(BuiltinType::I32), Vec::new())
        .unwrap();
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
                root: ModuleRef::new(0),
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
        public_items: vec![PublicItem::Function(FnDecl {
            method_policy: Default::default(),
            implementation: CallableImplementation::Script,
            name: "main".to_owned(),
            generic_params: Vec::new(),
            bounds: Vec::new(),
            params: Vec::new(),
            return_type: Ty::Builtin(return_type),
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
        return_type: ValueType::Unit,
        ..FunctionMetadata::default()
    };
    BytecodeModule {
        types: vec![ValueType::Unit],
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
                root: ModuleRef::new(0),
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
    root.dependencies = vec![ModuleRef::new(0)];
    KbcArtifact::from_program(
        BytecodeProgram {
            root: ModuleRef::new(1),
            modules: vec![dependency, root],
        },
        ArtifactBuildOptions {
            ..Default::default()
        },
    )
    .unwrap()
}

fn compatibility_for_artifact(artifact: &KbcArtifact) -> ArtifactCompatibility {
    ArtifactCompatibility {
        module_identity: Some(artifact.header.module_identity.clone()),
        dependency_fingerprints: Some(artifact.verification.loader.dependency_fingerprints.clone()),

        ..ArtifactCompatibility::default()
    }
}

#[test]
fn reload_publishes_valid_candidate_after_validation() {
    let mut runtime = Runtime::default();
    let loaded = runtime
        .load_program(
            "reloadable",
            BytecodeProgram {
                root: ModuleRef::new(0),
                modules: vec![module_with_public_function(BuiltinType::I32)],
            },
        )
        .expect("module should load");

    let reloaded = runtime
        .stage_reload_program(
            &loaded,
            "reloadable",
            BytecodeProgram {
                root: ModuleRef::new(0),
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
            BytecodeProgram {
                root: ModuleRef::new(0),
                modules: vec![module_with_public_function(BuiltinType::I32)],
            },
        )
        .expect("module should load");
    let before_count = runtime.modules().loaded_count();

    let error = runtime
        .stage_reload_program(
            &loaded,
            "reloadable",
            BytecodeProgram {
                root: ModuleRef::new(0),
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
            BytecodeProgram {
                root: ModuleRef::new(0),
                modules: vec![module_with_public_function(BuiltinType::I32)],
            },
        )
        .expect("module should load");
    let second = runtime
        .stage_reload_program(
            &first,
            "reloadable",
            BytecodeProgram {
                root: ModuleRef::new(0),
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
            BytecodeProgram {
                root: ModuleRef::new(0),
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
        ReloadValidationError::Artifact(ArtifactValidationError::DependencyFingerprintMismatch)
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
        BytecodeProgram {
            root: ModuleRef::new(0),
            modules: vec![module_with_public_function_and_constant(
                BuiltinType::I32,
                1,
            )],
        },
        ArtifactBuildOptions::default(),
    )
    .unwrap();
    let dependency_v2 = KbcArtifact::from_program(
        BytecodeProgram {
            root: ModuleRef::new(0),
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
            BytecodeProgram {
                root: ModuleRef::new(0),
                modules: vec![module_with_public_function(BuiltinType::I32)],
            },
        )
        .expect("consumer should load");
    let mut consumer_snapshot = ReloadDependencySnapshot::from_bytecode(
        &consumer.to_unverified(&Default::default()).unwrap(),
    );
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
            BytecodeProgram {
                root: ModuleRef::new(0),
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
            ReloadDependencySnapshot::from_bytecode(
                &loaded.to_unverified(&Default::default()).unwrap(),
            ),
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
            BytecodeProgram {
                root: ModuleRef::new(0),
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
            BytecodeProgram {
                root: ModuleRef::new(0),
                modules: vec![module_with_public_function(BuiltinType::I32)],
            },
        )
        .expect("consumer should load");
    let mut consumer_snapshot = ReloadDependencySnapshot::from_bytecode(
        &consumer.to_unverified(&Default::default()).unwrap(),
    );
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
        ReloadValidationError::Artifact(ArtifactValidationError::ContentHashMismatch)
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
    let mut runtime = Runtime::default();
    let owner = crate::layout_fixtures::allocation_owner(&mut runtime);
    let array = runtime
        .alloc_array(
            &owner,
            Ty::Builtin(BuiltinType::I32),
            vec![value::Value::I32(1)],
        )
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
fn exposes_runtime_type_registry() {
    let runtime = Runtime::new(RuntimeConfig {
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
}
