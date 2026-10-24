use crate::module::state::tests::{add_static, program as static_program, ready};
use crate::test_support::native::program::ProgramFixture;
use crate::value::Value;
use kagari_types::visibility::Visibility;
use kagari_types::{scalar::BuiltinType, ty::Ty};

use crate::host::HostRegistryId;
use crate::reload::validate_verified_reload_candidate;
use crate::resource::ResourceState;
use crate::{Runtime, RuntimeConfig, error::RuntimeErrorKind, gc::GcHeap};
use kagari_bytecode::instruction::{ConstantId, ConstantOperand};
use kagari_bytecode::program::ModuleRef;

use super::*;

fn collect(store: &ModuleStore) -> Vec<ModuleKey> {
    GcHeap::new(Default::default(), ResourceState::default())
        .collect(store, &[])
        .unwrap()
        .reclaimed_modules
}

fn slot_values(store: &ModuleStore) -> Vec<Value> {
    let graph = store.collection_graph().unwrap();
    let mut values = Vec::new();
    for key in graph.roots() {
        graph
            .trace(key, &mut |value| values.push(*value), &mut Vec::new())
            .unwrap();
    }
    values
}

#[test]
fn version_token_preserves_root_identity_without_retaining_execution() {
    let context = DefinitionContext::new().unwrap();
    let input = ProgramFixture {
        exports: Default::default(),
        root: ModuleRef::new(0),
        modules: vec![BytecodeModule::default()],
    }
    .metadata(context.snapshot(), &Default::default())
    .unwrap();
    let code = VerifiedProgram::new(input.clone(), &context).unwrap();
    let token = code.version.clone();
    let execution = Arc::downgrade(&code.execution);
    let original = code
        .modules()
        .iter()
        .map(Arc::downgrade)
        .collect::<Vec<_>>();
    let same_context = code.normalized(&context, &Default::default()).unwrap();
    assert!(Arc::ptr_eq(&code.modules, &same_context.modules));
    assert!(code.same_version(&same_context));
    let mut other_root = code.clone();
    other_root.root = ModuleRef::new(code.root.index() + 1);
    assert!(!code.same_version(&other_root));
    drop((same_context, other_root));
    let mut runtime = Runtime::default();
    let old_program = runtime.load_verified_program("token", code).unwrap();
    let old = runtime.select_program(&old_program).unwrap();
    let descriptor = Arc::downgrade(&old.program);
    let current_graph = old
        .verified_program()
        .modules()
        .iter()
        .map(Arc::downgrade)
        .collect::<Vec<_>>();
    let next = runtime
        .stage_reload_program(&old_program, None, input)
        .unwrap();
    runtime.publish_staged_reload(next).unwrap();

    let current = runtime.select_program(&old_program).unwrap();
    runtime.collect_garbage().unwrap();
    assert!(runtime.modules().loaded(old.key()).is_none());
    assert!(runtime.validate_executable_module(&old).is_err());
    assert!(
        runtime
            .retain_program(&old, ModuleEpochRetention::ActiveCall)
            .is_none()
    );
    assert!(runtime.validate_executable_module(&current).is_ok());
    drop(old);
    assert!(original.iter().all(|weak| weak.upgrade().is_none()));
    assert!(current_graph.iter().all(|weak| weak.upgrade().is_none()));
    assert!(descriptor.upgrade().is_none());
    assert!(execution.upgrade().is_none());
    assert_eq!(Arc::strong_count(&token), 1);
}

#[test]
fn invalid_code_cannot_become_a_shared_verified_program() {
    let context = DefinitionContext::new().unwrap();
    let invalid = ProgramFixture {
        exports: Default::default(),
        root: ModuleRef::new(1),
        modules: vec![BytecodeModule::default()],
    }
    .build(&context);
    assert!(
        VerifiedProgram::new(
            DefinitionMetadata::checked(context.snapshot(), invalid, &Default::default()).unwrap(),
            &context
        )
        .is_err()
    );
}

#[test]
fn shared_verification_rejects_in_memory_resource_exhaustion() {
    let context = DefinitionContext::new().unwrap();
    let oversized = ProgramFixture {
        exports: Default::default(),
        root: ModuleRef::new(0),
        modules: vec![BytecodeModule::default(); 1_025],
    }
    .build(&context);
    assert_eq!(
        VerifiedProgram::new(
            DefinitionMetadata::checked(context.snapshot(), oversized, &Default::default())
                .unwrap(),
            &context
        )
        .unwrap_err()
        .kind(),
        RuntimeErrorKind::ResourceLimitExceeded
    );
}

#[test]
fn verified_code_is_shared_but_runtime_linkage_and_instances_are_private() {
    let context = DefinitionContext::new().unwrap();
    let mut program = static_program(Ty::Builtin(BuiltinType::I32), true)
        .import_into(&context, &Default::default())
        .unwrap()
        .into_records();
    program.modules[0].constants = vec![
        ConstantOperand::Str("shared".into()),
        ConstantOperand::Str("pool-only".into()),
    ];
    let code = VerifiedProgram::new(
        DefinitionMetadata::checked(context.snapshot(), program, &Default::default()).unwrap(),
        &context,
    )
    .unwrap();
    let mut first_runtime = Runtime::new(RuntimeConfig::default());
    let mut second_runtime = Runtime::new(RuntimeConfig::default());
    let first_program = first_runtime
        .load_verified_program("shared", code.clone())
        .unwrap();
    let first = first_runtime.select_program(&first_program).unwrap();
    let second_program = second_runtime
        .load_verified_program("shared", code)
        .unwrap();
    let second = second_runtime.select_program(&second_program).unwrap();

    // Scoped definition IDs are runtime-local; immutable executable preparation
    // and the source revision remain shared across the identity adoption.
    assert!(Arc::ptr_eq(
        &first.program.code.execution,
        &second.program.code.execution
    ));
    assert!(Arc::ptr_eq(
        &first.program.code.version,
        &second.program.code.version
    ));
    assert_ne!(first.definitions().id(), second.definitions().id());
    assert!(first_runtime.validate_executable_module(&first).is_ok());
    assert!(second_runtime.validate_executable_module(&second).is_ok());
    assert!(first_runtime.validate_executable_module(&second).is_err());
    assert!(second_runtime.validate_executable_module(&first).is_err());
    let shared = ConstantId::new(0);
    let escaped = first_runtime.read_constant(&first, shared).unwrap();
    let before = first_runtime.gc().stats();
    for _ in 0..100 {
        assert_eq!(
            first_runtime.read_constant(&first, shared).unwrap(),
            escaped
        );
    }
    assert!(first_runtime.read_constant(&second, shared).is_err());
    assert!(
        first_runtime
            .read_constant(&first, ConstantId::new(2))
            .is_err()
    );
    assert_eq!(first_runtime.gc().stats(), before);
    let second_value = second_runtime.read_constant(&second, shared).unwrap();
    assert_ne!(escaped, second_value);
    for (runtime, value) in [(&first_runtime, escaped), (&second_runtime, second_value)] {
        let Value::Str(id) = value else {
            panic!("expected string")
        };
        assert_eq!(&*runtime.gc().string(id).unwrap(), "shared");
    }
    ready(&first_runtime, &first, 0, Value::I32(7));
    assert!(matches!(
        first_runtime
            .module_instance_snapshot(&first)
            .unwrap()
            .module_slots[0],
        StaticState::Ready(Value::I32(7))
    ));
    assert!(matches!(
        second_runtime
            .module_instance_snapshot(&second)
            .unwrap()
            .module_slots[0],
        StaticState::Uninitialized(_)
    ));

    let pool_only = first_runtime
        .read_constant(&first, ConstantId::new(1))
        .unwrap();
    assert_eq!(
        first_runtime.collect_garbage().unwrap().reclaimed_objects,
        0
    );
    let escaped_root = first_runtime.root_value(escaped).unwrap();
    let (definitions, mut replacement) = first
        .to_unverified(&CancellationToken::default())
        .unwrap()
        .into_parts();
    replacement.constants.clear();
    let candidate = first_runtime
        .stage_reload_program(
            &first_program,
            None,
            ProgramFixture {
                exports: Default::default(),
                root: ModuleRef::new(0),
                modules: vec![replacement],
            }
            .metadata(definitions, &Default::default())
            .unwrap(),
        )
        .unwrap();
    first_runtime.publish_staged_reload(candidate).unwrap();
    let collected = first_runtime.collect_garbage().unwrap();
    assert_eq!(collected.reclaimed_modules, vec![first.key()]);
    assert_eq!(collected.reclaimed_objects, 1);
    assert!(!first_runtime.gc().validate_value(&pool_only));
    assert!(first_runtime.gc().validate_value(&escaped));
    assert!(first_runtime.read_constant(&first, shared).is_err());
    drop(escaped_root);
    assert_eq!(
        first_runtime.collect_garbage().unwrap().reclaimed_objects,
        1
    );
    assert!(!first_runtime.gc().validate_value(&escaped));
    assert_eq!(
        second_runtime.collect_garbage().unwrap().reclaimed_objects,
        0
    );
    assert_eq!(
        second_runtime.read_constant(&second, shared).unwrap(),
        second_value
    );
}

#[test]
fn staged_programs_keep_instances_alive_without_activating_them() {
    let context = DefinitionContext::new().unwrap();
    let mut store = ModuleStore::default();
    let mut root_identity = BytecodeModule::default().identity;
    root_identity.path.push("root".into());
    let installation = store
        .reserve_installation(HostRegistryId::default(), root_identity)
        .unwrap();
    let stage = |epoch| {
        let mut dependency = BytecodeModule::default();
        add_static(
            &context,
            &mut dependency,
            "state",
            Ty::Builtin(BuiltinType::I32),
            true,
            Visibility::Private,
        );
        let mut root = BytecodeModule::default();
        root.identity.path.push("root".into());
        root.dependencies.push(ModuleRef::new(0));
        add_static(
            &context,
            &mut root,
            "state",
            Ty::Builtin(BuiltinType::I32),
            true,
            Visibility::Private,
        );
        let code = VerifiedProgram::new(
            ProgramFixture {
                exports: Default::default(),
                root: ModuleRef::new(1),
                modules: vec![dependency, root],
            }
            .metadata(context.snapshot(), &Default::default())
            .unwrap(),
            &context,
        )
        .unwrap();
        let admission = installation
            .capture_current()
            .ok()
            .map(|baseline| validate_verified_reload_candidate(&baseline, &code).unwrap());
        store
            .stage_verified_program(
                installation.clone(),
                "game.player",
                ModuleEpoch(epoch),
                code,
                LinkedProgramBindings {
                    members: vec![LinkedHostBindings::default(), LinkedHostBindings::default()],
                    shared_traits: Default::default(),
                },
                admission,
            )
            .unwrap()
    };
    let baseline = stage(1).publish(&store).unwrap();
    let abandoned = stage(2);
    let candidate = stage(3);
    let abandoned_keys = abandoned
        .module
        .members()
        .map(|m| m.key())
        .collect::<Vec<_>>();
    for member in candidate.module.members() {
        let mut inner = store.inner.borrow_mut();
        let id = inner.resolve(&member).unwrap().bindings[0];
        inner.cells.get_mut(&id).unwrap().state = StaticState::Ready(Value::I32(73));
        drop(inner);
        assert!(store.is_program_root(member.key()));
    }
    assert_eq!(
        installation.capture_current().unwrap().key(),
        baseline.key()
    );
    assert_eq!(store.loaded_count(), 6);
    assert!(collect(&store).is_empty());
    assert_eq!(
        slot_values(&store)
            .into_iter()
            .filter(|value| *value == Value::I32(73))
            .count(),
        2
    );

    drop(abandoned);
    for key in &abandoned_keys {
        assert!(store.loaded(*key).is_none());
        assert!(store.instance_snapshot(*key).is_none());
    }
    assert_eq!(store.loaded_count(), 4);
    assert_eq!(
        installation.capture_current().unwrap().key(),
        baseline.key()
    );
    assert_eq!(
        collect(&store).into_iter().collect::<HashSet<_>>(),
        abandoned_keys.into_iter().collect()
    );

    let published = candidate.publish(&store).unwrap();
    assert_eq!(
        installation.capture_current().unwrap().key(),
        published.key()
    );
    for member in published.members() {
        let instance = store.instance_snapshot(member.key()).unwrap();
        assert!(matches!(
            instance.module_slots.as_slice(),
            [StaticState::Ready(Value::I32(73))]
        ));
    }
    assert_eq!(collect(&store).len(), 2);
    assert_eq!(store.loaded_count(), 2);
    assert_eq!(
        slot_values(&store)
            .into_iter()
            .filter(|value| *value == Value::I32(73))
            .count(),
        2
    );
}

#[test]
fn abandoning_candidates_invalidates_access_even_after_quarantine() {
    let context = DefinitionContext::new().unwrap();
    let mut runtime = Runtime::default();
    let installation = runtime
        .modules
        .reserve_installation(runtime.host.owner(), BytecodeModule::default().identity)
        .unwrap();
    let store = runtime.modules();
    let stage = |epoch| {
        store.stage_verified_program(
            installation.clone(),
            "candidate",
            ModuleEpoch(epoch),
            VerifiedProgram::new(
                ProgramFixture {
                    exports: Default::default(),
                    root: ModuleRef::new(0),
                    modules: vec![BytecodeModule::default()],
                }
                .metadata(context.snapshot(), &Default::default())
                .unwrap(),
                &context,
            )
            .unwrap(),
            LinkedProgramBindings {
                members: vec![LinkedHostBindings::default()],
                shared_traits: Default::default(),
            },
            None,
        )
    };
    let candidate = stage(1).unwrap();
    assert_eq!(store.loaded_count(), 1);
    assert!(installation.capture_current().is_err());
    drop(candidate);
    assert_eq!(store.loaded_count(), 0);
    let candidate = stage(3).unwrap();
    runtime
        .resources()
        .quarantine("test failed initializer invariant");
    drop(candidate);
    assert_eq!(store.loaded_count(), 0);
    assert!(runtime.is_quarantined());
}

#[test]
fn assigns_stable_module_ids_across_epochs() {
    let context = DefinitionContext::new().unwrap();
    let mut store = ModuleStore::default();
    let installation = store
        .reserve_installation(
            HostRegistryId::default(),
            BytecodeModule::default().identity,
        )
        .unwrap();
    let first = store
        .stage_verified_program(
            installation.clone(),
            "game.player",
            ModuleEpoch(1),
            VerifiedProgram::new(
                ProgramFixture {
                    exports: Default::default(),
                    root: ModuleRef::new(0),
                    modules: vec![BytecodeModule::default()],
                }
                .metadata(context.snapshot(), &Default::default())
                .unwrap(),
                &context,
            )
            .unwrap(),
            LinkedProgramBindings {
                members: vec![LinkedHostBindings::default()],
                shared_traits: Default::default(),
            },
            None,
        )
        .unwrap()
        .publish(&store)
        .unwrap();
    let second = store
        .stage_verified_program(
            installation.clone(),
            "game.player",
            ModuleEpoch(2),
            VerifiedProgram::new(
                ProgramFixture {
                    exports: Default::default(),
                    root: ModuleRef::new(0),
                    modules: vec![BytecodeModule::default()],
                }
                .metadata(context.snapshot(), &Default::default())
                .unwrap(),
                &context,
            )
            .unwrap(),
            LinkedProgramBindings {
                members: vec![LinkedHostBindings::default()],
                shared_traits: Default::default(),
            },
            Some(validate_verified_reload_candidate(&first, first.verified_program()).unwrap()),
        )
        .unwrap()
        .publish(&store)
        .unwrap();
    let other_installation = store
        .reserve_installation(
            HostRegistryId::default(),
            BytecodeModule::default().identity,
        )
        .unwrap();
    let other = store
        .stage_verified_program(
            other_installation,
            "game.world",
            ModuleEpoch(1),
            VerifiedProgram::new(
                ProgramFixture {
                    exports: Default::default(),
                    root: ModuleRef::new(0),
                    modules: vec![BytecodeModule::default()],
                }
                .metadata(context.snapshot(), &Default::default())
                .unwrap(),
                &context,
            )
            .unwrap(),
            LinkedProgramBindings {
                members: vec![LinkedHostBindings::default()],
                shared_traits: Default::default(),
            },
            None,
        )
        .unwrap()
        .publish(&store)
        .unwrap();

    assert_eq!(first.id, second.id);
    assert_ne!(first.id, other.id);
    assert_eq!(first.id.index(), 0);
    assert_eq!(other.id.index(), 1);
    assert_eq!(store.loaded_count(), 3);
    assert_eq!(
        installation.capture_current().unwrap().epoch,
        ModuleEpoch(2)
    );
}

#[test]
fn snapshot_during_mutable_instance_borrow_does_not_panic() {
    let context = DefinitionContext::new().unwrap();
    let mut store = ModuleStore::default();
    let installation = store
        .reserve_installation(
            HostRegistryId::default(),
            BytecodeModule::default().identity,
        )
        .unwrap();
    let module = store
        .stage_verified_program(
            installation.clone(),
            "game.snapshot",
            ModuleEpoch(1),
            VerifiedProgram::new(
                ProgramFixture {
                    exports: Default::default(),
                    root: ModuleRef::new(0),
                    modules: vec![BytecodeModule::default()],
                }
                .metadata(context.snapshot(), &Default::default())
                .unwrap(),
                &context,
            )
            .unwrap(),
            LinkedProgramBindings {
                members: vec![LinkedHostBindings::default()],
                shared_traits: Default::default(),
            },
            None,
        )
        .unwrap()
        .publish(&store)
        .unwrap();
    let held = store.inner.borrow_mut();
    assert!(store.instance_snapshot(module.key()).is_none());
    drop(held);
    assert!(store.instance_snapshot(module.key()).is_some());
}

#[test]
fn keeps_latest_and_retained_old_epochs_reachable() {
    let context = DefinitionContext::new().unwrap();
    let mut store = ModuleStore::default();
    let installation = store
        .reserve_installation(
            HostRegistryId::default(),
            BytecodeModule::default().identity,
        )
        .unwrap();
    let first = store
        .stage_verified_program(
            installation.clone(),
            "game.player",
            ModuleEpoch(1),
            VerifiedProgram::new(
                ProgramFixture {
                    exports: Default::default(),
                    root: ModuleRef::new(0),
                    modules: vec![BytecodeModule::default()],
                }
                .metadata(context.snapshot(), &Default::default())
                .unwrap(),
                &context,
            )
            .unwrap(),
            LinkedProgramBindings {
                members: vec![LinkedHostBindings::default()],
                shared_traits: Default::default(),
            },
            None,
        )
        .unwrap()
        .publish(&store)
        .unwrap();
    let second = store
        .stage_verified_program(
            installation.clone(),
            "game.player",
            ModuleEpoch(2),
            VerifiedProgram::new(
                ProgramFixture {
                    exports: Default::default(),
                    root: ModuleRef::new(0),
                    modules: vec![BytecodeModule::default()],
                }
                .metadata(context.snapshot(), &Default::default())
                .unwrap(),
                &context,
            )
            .unwrap(),
            LinkedProgramBindings {
                members: vec![LinkedHostBindings::default()],
                shared_traits: Default::default(),
            },
            Some(validate_verified_reload_candidate(&first, first.verified_program()).unwrap()),
        )
        .unwrap()
        .publish(&store)
        .unwrap();

    assert!(store.is_program_root(second.key()));
    assert!(!store.is_program_root(first.key()));
    let call = store
        .retain_module(&first, ModuleEpochRetention::ActiveCall)
        .unwrap();
    let value = store
        .retain_module(&first, ModuleEpochRetention::RuntimeValue)
        .unwrap();
    let artifact = store
        .retain_module(&first, ModuleEpochRetention::CompiledArtifact)
        .unwrap();
    assert!(store.is_program_root(first.key()));
    assert_eq!(store.retention_counts(first.key()).active_calls, 1);
    assert_eq!(store.retention_counts(first.key()).runtime_values, 1);
    assert_eq!(store.retention_counts(first.key()).compiled_artifacts, 1);
    assert!(collect(&store).is_empty());
    assert!(store.loaded(first.key()).is_some());

    drop(call);
    drop(value);
    drop(artifact);
    assert_eq!(collect(&store), vec![first.key()]);
    assert!(store.loaded(first.key()).is_none());
    assert!(store.loaded(second.key()).is_some());
}

mod enum_declarations;

mod package_exports;

mod struct_layouts;
