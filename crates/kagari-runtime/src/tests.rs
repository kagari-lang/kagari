use super::*;
use crate::test_support::native::program::ProgramFixture;
use crate::{
    metadata::{AbiFingerprint, TypeKind, TypeRegistration},
    reload::{ReloadValidationError, validate_reload_candidate},
};
use kagari_abi::representation::ValueType;
use kagari_bytecode::{
    artifact::{
        ArtifactBuildOptions, ArtifactCompatibility, ArtifactValidationError,
        DependencyFingerprint, KbcArtifact,
    },
    instruction::{BytecodeInstruction, ConstantOperand},
    module::{BytecodeFunction, BytecodeModule, FunctionMetadata, FunctionRecord},
    program::ModuleRef,
};
use kagari_contract::{ids::FunctionRef, types::PublicItem};
use kagari_types::{
    callable::CallableImplementation, declaration::FnDecl, scalar::BuiltinType, ty::Ty,
};

fn module_with_public_function(return_type: BuiltinType) -> BytecodeModule {
    BytecodeModule {
        public_items: vec![PublicItem::Function(FnDecl {
            host_policy: None,
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
        functions: vec![{
            let fixture_instructions = vec![BytecodeInstruction::Return(None)];
            let mut fixture_metadata: kagari_bytecode::module::FunctionMetadata = metadata;

            if fixture_metadata.origins.is_empty() {
                fixture_metadata.origins = (0..fixture_instructions.len())
                    .map(|instruction| kagari_bytecode::module::InstructionOrigin {
                        span: Default::default(),
                        line: None,
                        column: None,
                        instruction,
                    })
                    .collect();
            }
            BytecodeFunction {
                id: FunctionRef::new(0),
                name: "main".to_owned(),
                slot_count: u32::try_from(fixture_metadata.slots.len()).unwrap(),
                metadata: fixture_metadata,
                instructions: fixture_instructions,
                ..BytecodeFunction::default()
            }
        }],
        ..BytecodeModule::default()
    }
}

#[test]
fn load_rejects_missing_function_terminator_before_publication() {
    let context = DefinitionContext::new().unwrap();
    let mut module = module_with_executable_function();
    module.functions[0].instructions.clear();
    let mut runtime = Runtime::default();
    let error = runtime
        .load_program(
            "missing-return",
            ProgramFixture {
                exports: Default::default(),
                root: ModuleRef::new(0),
                modules: vec![module],
            }
            .metadata(context.snapshot(), &Default::default())
            .unwrap(),
        )
        .unwrap_err();
    assert_eq!(error.kind(), RuntimeErrorKind::ModuleValidation);
    assert_eq!(runtime.modules().loaded_count(), 0);
}

fn artifact_with_loader_fingerprints(context: &DefinitionContext) -> KbcArtifact {
    let dependency = BytecodeModule {
        identity: kagari_common::identity::ModuleIdentity::single_file("pkg/dependency"),
        ..Default::default()
    };
    let mut root = module_with_public_function(BuiltinType::I32);
    root.dependencies = vec![ModuleRef::new(0)];
    KbcArtifact::from_program(
        ProgramFixture {
            exports: Default::default(),
            root: ModuleRef::new(1),
            modules: vec![dependency, root],
        }
        .build(context),
        ArtifactBuildOptions {
            ..Default::default()
        },
        context,
        &Default::default(),
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
    let context = DefinitionContext::new().unwrap();
    let mut runtime = Runtime::default();
    let loaded_program = runtime
        .load_program(
            "reloadable",
            ProgramFixture {
                exports: Default::default(),
                root: ModuleRef::new(0),
                modules: vec![module_with_public_function(BuiltinType::I32)],
            }
            .metadata(context.snapshot(), &Default::default())
            .unwrap(),
        )
        .expect("module should load");
    let loaded = runtime.select_program(&loaded_program).unwrap();

    runtime
        .stage_reload_program(
            &loaded_program,
            None,
            ProgramFixture {
                exports: Default::default(),
                root: ModuleRef::new(0),
                modules: vec![module_with_public_function(BuiltinType::I32)],
            }
            .metadata(context.snapshot(), &Default::default())
            .unwrap(),
        )
        .and_then(|candidate| runtime.publish_staged_reload(candidate))
        .expect("compatible module should reload");

    let reloaded = runtime.select_program(&loaded_program).unwrap();

    assert_eq!(reloaded.id, loaded.id);
    assert_eq!(reloaded.epoch.0, loaded.epoch.0 + 1);
    assert_eq!(
        runtime.select_program(&loaded_program).unwrap().epoch,
        reloaded.epoch
    );
}

#[test]
fn reload_rejects_public_abi_changes_before_publication() {
    let context = DefinitionContext::new().unwrap();
    let mut runtime = Runtime::default();
    let loaded_program = runtime
        .load_program(
            "reloadable",
            ProgramFixture {
                exports: Default::default(),
                root: ModuleRef::new(0),
                modules: vec![module_with_public_function(BuiltinType::I32)],
            }
            .metadata(context.snapshot(), &Default::default())
            .unwrap(),
        )
        .expect("module should load");
    let loaded = runtime.select_program(&loaded_program).unwrap();
    let before_count = runtime.modules().loaded_count();

    let error = runtime
        .stage_reload_program(
            &loaded_program,
            None,
            ProgramFixture {
                exports: Default::default(),
                root: ModuleRef::new(0),
                modules: vec![module_with_public_function(BuiltinType::String)],
            }
            .metadata(context.snapshot(), &Default::default())
            .unwrap(),
        )
        .and_then(|candidate| runtime.publish_staged_reload(candidate))
        .expect_err("public ABI change should reject reload");

    assert_eq!(error.code(), "KG_RELOAD_DECLARATION_CONTRACT_MISMATCH");
    assert!(matches!(
        error,
        ReloadValidationError::DeclarationContractMismatch { .. }
    ));
    assert_eq!(runtime.modules().loaded_count(), before_count);
    assert_eq!(
        runtime.select_program(&loaded_program).unwrap().epoch,
        loaded.epoch
    );
}

#[test]
fn pure_reload_validation_rejects_stale_active_epoch() {
    let context = DefinitionContext::new().unwrap();
    let mut runtime = Runtime::default();
    let first_program = runtime
        .load_program(
            "reloadable",
            ProgramFixture {
                exports: Default::default(),
                root: ModuleRef::new(0),
                modules: vec![module_with_public_function(BuiltinType::I32)],
            }
            .metadata(context.snapshot(), &Default::default())
            .unwrap(),
        )
        .expect("module should load");
    let first = runtime.select_program(&first_program).unwrap();
    runtime
        .stage_reload_program(
            &first_program,
            None,
            ProgramFixture {
                exports: Default::default(),
                root: ModuleRef::new(0),
                modules: vec![module_with_public_function(BuiltinType::I32)],
            }
            .metadata(context.snapshot(), &Default::default())
            .unwrap(),
        )
        .and_then(|candidate| runtime.publish_staged_reload(candidate))
        .expect("compatible module should reload");

    let second = runtime.select_program(&first_program).unwrap();
    let before_count = runtime.modules().loaded_count();

    let error = validate_reload_candidate(
        &first,
        ProgramFixture {
            exports: Default::default(),
            root: ModuleRef::new(0),
            modules: vec![module_with_public_function(BuiltinType::I32)],
        }
        .build(&context),
        Some(&second),
        &context,
    )
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
        runtime.select_program(&first_program).unwrap().epoch,
        second.epoch
    );
}

#[test]
fn reload_artifact_validates_loader_compatibility_before_publication() {
    let context = DefinitionContext::new().unwrap();
    let artifact = artifact_with_loader_fingerprints(&context);
    let mut runtime = Runtime::default();
    let loaded_program = runtime
        .load_program(
            "reloadable",
            DefinitionMetadata::checked(
                context.snapshot(),
                artifact.program.clone(),
                &Default::default(),
            )
            .unwrap(),
        )
        .expect("module should load");
    let loaded = runtime.select_program(&loaded_program).unwrap();
    let before_count = runtime.modules().loaded_count();

    let error = runtime
        .stage_reload_artifact(
            &loaded_program,
            None,
            DefinitionMetadata::checked(context.snapshot(), artifact, &Default::default()).unwrap(),
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
        runtime.select_program(&loaded_program).unwrap().epoch,
        loaded.epoch
    );
}

#[test]
fn reload_artifact_publishes_after_loader_and_reload_validation() {
    let context = DefinitionContext::new().unwrap();
    let artifact = artifact_with_loader_fingerprints(&context);
    let compatibility = compatibility_for_artifact(&artifact);
    let mut runtime = Runtime::default();
    let loaded_program = runtime
        .load_program(
            "reloadable",
            DefinitionMetadata::checked(
                context.snapshot(),
                artifact.program.clone(),
                &Default::default(),
            )
            .unwrap(),
        )
        .expect("module should load");
    let loaded = runtime.select_program(&loaded_program).unwrap();

    runtime
        .stage_reload_artifact(
            &loaded_program,
            None,
            DefinitionMetadata::checked(context.snapshot(), artifact, &Default::default()).unwrap(),
            &compatibility,
        )
        .and_then(|candidate| runtime.publish_staged_reload(candidate))
        .expect("compatible artifact should reload");

    let reloaded = runtime.select_program(&loaded_program).unwrap();

    assert_eq!(reloaded.id, loaded.id);
    assert_eq!(reloaded.epoch.0, loaded.epoch.0 + 1);
    assert_eq!(
        runtime.select_program(&loaded_program).unwrap().epoch,
        reloaded.epoch
    );
}

#[test]
fn staged_reload_failure_and_stale_publication_preserve_the_active_entry() {
    let context = DefinitionContext::new().unwrap();
    let artifact = artifact_with_loader_fingerprints(&context);
    let mut runtime = Runtime::default();
    let baseline_program = runtime
        .load_program(
            "reloadable",
            DefinitionMetadata::checked(
                context.snapshot(),
                artifact.program.clone(),
                &Default::default(),
            )
            .unwrap(),
        )
        .unwrap();
    let baseline = runtime.select_program(&baseline_program).unwrap();
    let stage = |runtime: &mut Runtime| {
        let prepared = runtime
            .prepare_reload(
                &baseline_program,
                None,
                VerifiedProgram::new(
                    DefinitionMetadata::checked(
                        context.snapshot(),
                        artifact.program.clone(),
                        &Default::default(),
                    )
                    .unwrap(),
                    &context,
                )
                .unwrap(),
            )
            .unwrap();
        runtime.stage_prepared_reload(prepared).unwrap()
    };
    let failed = stage(&mut runtime);
    let failed_key = failed.program.module().key();
    assert_eq!(
        runtime.select_program(&baseline_program).unwrap().key(),
        baseline.key()
    );
    drop(failed);
    assert!(runtime.modules.loaded(failed_key).is_none());
    assert_eq!(runtime.modules.loaded_count(), baseline.members().count());

    let candidate = stage(&mut runtime);
    let stale = stage(&mut runtime);
    let stale_key = stale.program.module().key();
    assert_eq!(
        runtime.select_program(&baseline_program).unwrap().key(),
        baseline.key()
    );
    runtime.publish_staged_reload(candidate).unwrap();

    let current = runtime.select_program(&baseline_program).unwrap();
    assert!(matches!(
        runtime.publish_staged_reload(stale),
        Err(ReloadValidationError::ModuleNotActive { .. })
    ));
    assert!(runtime.modules.loaded(stale_key).is_none());
    assert_eq!(
        runtime.select_program(&baseline_program).unwrap().key(),
        current.key()
    );
    assert_eq!(
        runtime.modules.loaded_count(),
        baseline.members().count() + current.members().count()
    );
    assert!(runtime.module_instance_snapshot(&current).is_some());
    runtime.validate_executable_module(&baseline).unwrap();
}

#[test]
fn prepared_reload_is_inert_and_rejects_a_stale_publication() {
    let context = DefinitionContext::new().unwrap();
    let artifact = artifact_with_loader_fingerprints(&context);
    let mut runtime = Runtime::default();
    let baseline_program = runtime
        .load_program(
            "reloadable",
            DefinitionMetadata::checked(
                context.snapshot(),
                artifact.program.clone(),
                &Default::default(),
            )
            .unwrap(),
        )
        .unwrap();
    let baseline = runtime.select_program(&baseline_program).unwrap();
    let before_count = runtime.modules().loaded_count();
    let before_resources = runtime.resources().counters();
    let prepare = |runtime: &Runtime| {
        runtime
            .prepare_reload(
                &baseline_program,
                None,
                VerifiedProgram::new(
                    DefinitionMetadata::checked(
                        context.snapshot(),
                        artifact.program.clone(),
                        &Default::default(),
                    )
                    .unwrap(),
                    &context,
                )
                .unwrap(),
            )
            .unwrap()
    };
    let candidate = prepare(&runtime);
    let stale = prepare(&runtime);
    assert_eq!(runtime.modules().loaded_count(), before_count);
    assert_eq!(runtime.resources().counters(), before_resources);
    assert_eq!(
        runtime.select_program(&baseline_program).unwrap().key(),
        baseline.key()
    );
    runtime
        .stage_prepared_reload(candidate)
        .and_then(|candidate| runtime.publish_staged_reload(candidate))
        .unwrap();

    let current = runtime.select_program(&baseline_program).unwrap();
    let published_resources = runtime.resources().counters();
    let published_count = runtime.modules().loaded_count();
    assert!(matches!(
        runtime
            .stage_prepared_reload(stale)
            .and_then(|candidate| runtime.publish_staged_reload(candidate)),
        Err(ReloadValidationError::ModuleNotActive { .. })
    ));
    assert_eq!(
        runtime.select_program(&baseline_program).unwrap().key(),
        current.key()
    );
    assert_eq!(runtime.modules().loaded_count(), published_count);
    assert_eq!(runtime.resources().counters(), published_resources);
    runtime.validate_executable_module(&baseline).unwrap();
}

#[test]
fn reload_preserves_independent_installation_caches_despite_matching_dependency_metadata() {
    let context = DefinitionContext::new().unwrap();
    let dependency_v1 = KbcArtifact::from_program(
        ProgramFixture {
            exports: Default::default(),
            root: ModuleRef::new(0),
            modules: vec![module_with_public_function_and_constant(
                BuiltinType::I32,
                1,
            )],
        }
        .build(&context),
        ArtifactBuildOptions::default(),
        &context,
        &Default::default(),
    )
    .unwrap();
    let dependency_v2 = KbcArtifact::from_program(
        ProgramFixture {
            exports: Default::default(),
            root: ModuleRef::new(0),
            modules: vec![module_with_public_function_and_constant(
                BuiltinType::I32,
                2,
            )],
        }
        .build(&context),
        ArtifactBuildOptions::default(),
        &context,
        &Default::default(),
    )
    .unwrap();
    let dependency_v1_snapshot =
        ReloadDependencySnapshot::from_artifact(&dependency_v1, &context.snapshot()).unwrap();
    let dependency_v2_compatibility = compatibility_for_artifact(&dependency_v2);
    let mut runtime = Runtime::default();
    let dependency_program = runtime
        .load_program(
            "dependency",
            DefinitionMetadata::checked(
                context.snapshot(),
                dependency_v1.program.clone(),
                &Default::default(),
            )
            .unwrap(),
        )
        .expect("dependency should load");
    let dependency = runtime.select_program(&dependency_program).unwrap();
    let consumer_program = runtime
        .load_program(
            "consumer",
            ProgramFixture {
                exports: Default::default(),
                root: ModuleRef::new(0),
                modules: vec![module_with_public_function(BuiltinType::I32)],
            }
            .metadata(context.snapshot(), &Default::default())
            .unwrap(),
        )
        .expect("consumer should load");
    let consumer = runtime.select_program(&consumer_program).unwrap();
    assert_ne!(dependency.installation_id(), consumer.installation_id());
    let mut consumer_snapshot = {
        let metadata = consumer.to_unverified(&Default::default()).unwrap();
        ReloadDependencySnapshot::from_bytecode(metadata.records(), metadata.definitions()).unwrap()
    };
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
            &dependency_program,
            None,
            DefinitionMetadata::checked(context.snapshot(), dependency_v2, &Default::default())
                .unwrap(),
            &dependency_v2_compatibility,
        )
        .and_then(|candidate| runtime.publish_staged_reload(candidate))
        .expect("compatible dependency implementation should reload");

    assert!(runtime.interpreter_cache(interpreter_cache).is_some());
    assert!(runtime.interpreter_cache(function_cache).is_some());
    assert!(runtime.interpreter_caches.get(interpreter_cache).is_some());
    assert!(runtime.interpreter_caches.get(function_cache).is_some());
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
    let context = DefinitionContext::new().unwrap();
    let mut runtime = Runtime::default();
    let loaded_program = runtime
        .load_program(
            "reloadable",
            ProgramFixture {
                exports: Default::default(),
                root: ModuleRef::new(0),
                modules: vec![module_with_public_function_and_constant(
                    BuiltinType::I32,
                    1,
                )],
            }
            .metadata(context.snapshot(), &Default::default())
            .unwrap(),
        )
        .expect("module should load");
    let loaded = runtime.select_program(&loaded_program).unwrap();
    let artifact = runtime
        .register_interpreter_cache(loaded.key(), None, {
            let metadata = loaded.to_unverified(&Default::default()).unwrap();
            ReloadDependencySnapshot::from_bytecode(metadata.records(), metadata.definitions())
                .unwrap()
        })
        .expect("function cache should register");

    assert!(runtime.interpreter_cache(artifact).is_some());
    assert_eq!(
        runtime
            .modules()
            .retention_counts(loaded.key())
            .compiled_artifacts,
        0
    );

    runtime
        .stage_reload_program(
            &loaded_program,
            None,
            ProgramFixture {
                exports: Default::default(),
                root: ModuleRef::new(0),
                modules: vec![module_with_public_function_and_constant(
                    BuiltinType::I32,
                    2,
                )],
            }
            .metadata(context.snapshot(), &Default::default())
            .unwrap(),
        )
        .and_then(|candidate| runtime.publish_staged_reload(candidate))
        .expect("implementation-only reload should publish a new epoch");

    let reloaded = runtime.select_program(&loaded_program).unwrap();

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
    let context = DefinitionContext::new().unwrap();
    let dependency_v1 = artifact_with_loader_fingerprints(&context);
    let dependency_v1_snapshot =
        ReloadDependencySnapshot::from_artifact(&dependency_v1, &context.snapshot()).unwrap();
    let mut dependency_v2 = dependency_v1.clone();
    dependency_v2.program.modules[dependency_v2.program.root.index()]
        .constants
        .push(ConstantOperand::I32(2));
    let mut runtime = Runtime::default();
    let dependency_program = runtime
        .load_program(
            "dependency",
            DefinitionMetadata::checked(
                context.snapshot(),
                dependency_v1.program.clone(),
                &Default::default(),
            )
            .unwrap(),
        )
        .expect("dependency should load");
    let consumer_program = runtime
        .load_program(
            "consumer",
            ProgramFixture {
                exports: Default::default(),
                root: ModuleRef::new(0),
                modules: vec![module_with_public_function(BuiltinType::I32)],
            }
            .metadata(context.snapshot(), &Default::default())
            .unwrap(),
        )
        .expect("consumer should load");
    let consumer = runtime.select_program(&consumer_program).unwrap();
    let mut consumer_snapshot = {
        let metadata = consumer.to_unverified(&Default::default()).unwrap();
        ReloadDependencySnapshot::from_bytecode(metadata.records(), metadata.definitions()).unwrap()
    };
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
            &dependency_program,
            None,
            DefinitionMetadata::checked(context.snapshot(), dependency_v2, &Default::default())
                .unwrap(),
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
fn heap_allocations_update_runtime_resource_counters() {
    let mut runtime = Runtime::default();
    let owner = crate::test_support::layouts::allocation_owner(&mut runtime);
    let value = runtime
        .alloc_array(
            &owner,
            Ty::Builtin(BuiltinType::I32),
            vec![value::Value::I32(1), value::Value::I32(2)],
        )
        .unwrap();
    let _root = runtime.gc().root_value(value::Value::Array(value)).unwrap();
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

mod owned_cleanup;
mod runtime_substrate;
