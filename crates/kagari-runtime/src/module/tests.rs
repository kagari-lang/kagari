use crate::module::state::initialization::InitializationAttempt;
use crate::{
    Runtime, RuntimeConfig,
    error::{RuntimeErrorKind, fault::EngineFault},
    execution::memory::ManagedMemory,
    host::HostRegistryId,
    module::{
        managed::{
            InstallationState, cell::StaticCellObject, revision::RevisionState,
            source::RevisionSource,
        },
        retention::RetentionOrigin,
        staging::{PreparedProgramMetadata, StagedProgram},
        state::tests::{add_static, program as static_program, ready},
    },
    reload::validate_verified_reload_candidate,
    resource::ResourceState,
    test_support::native::program::ProgramFixture,
    value::{
        carriers::{RootedValue, ScopedValue},
        strings::StringObject,
    },
};
use std::sync::atomic::AtomicBool;

use kagari_types::visibility::Visibility;
use kagari_types::{scalar::BuiltinType, ty::Ty};

use kagari_bytecode::instruction::{ConstantId, ConstantOperand, ModuleSlot};
use kagari_bytecode::program::ModuleRef;

use super::*;

fn collect(
    store: &ModuleStore,
    memory: &mut ManagedMemory,
    resources: &ResourceState,
) -> Vec<ModuleKey> {
    let abandoned = store.take_abandonment();
    memory.collect(resources).unwrap();
    store.dispose_retired(abandoned).unwrap()
}

fn prepare(
    store: &ModuleStore,
    memory: &mut ManagedMemory,
    metadata: PreparedProgramMetadata,
    resources: &ResourceState,
) -> StagedProgram {
    let candidate = metadata.into_managed(store, memory, resources).unwrap();
    if candidate.is_initial() {
        candidate.finish_bindings(store, memory, resources).unwrap();
        candidate
    } else {
        let baseline = store.capture_current(&candidate.installation).unwrap();
        candidate
            .bind_reload(store, &baseline, memory, resources)
            .unwrap()
    }
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
    let mut other_root = code.clone();
    other_root.root = ModuleRef::new(code.root.index() + 1);
    assert!(!code.same_version(&other_root));
    drop(other_root);
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
    assert_ne!(token, current.verified_program().revision());
    runtime.collect_garbage().unwrap();
    assert!(runtime.modules().loaded(old.key()).is_none());
    assert!(runtime.validate_executable_module(&old).is_err());
    assert!(
        runtime
            .retain_program(
                &old,
                ModuleEpochRetention::ActiveCall,
                RevisionSource::Current
            )
            .is_none()
    );
    assert!(runtime.validate_executable_module(&current).is_ok());
    drop(old);
    assert!(original.iter().all(|weak| weak.upgrade().is_none()));
    assert!(current_graph.iter().all(|weak| weak.upgrade().is_none()));
    assert!(descriptor.upgrade().is_none());
    assert!(execution.upgrade().is_none());
    drop(token);
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

    // Products retain their original immutable identity scope and code. Local
    // admission records and execution state belong to their respective Runtime.
    assert!(Arc::ptr_eq(&first.bytecode, &second.bytecode));
    assert!(Arc::ptr_eq(
        &first.program.code.execution,
        &second.program.code.execution
    ));
    assert!(Arc::ptr_eq(
        first.verified_program().dispatch_facts().unwrap(),
        second.verified_program().dispatch_facts().unwrap(),
    ));
    assert!(Arc::ptr_eq(
        &first.program.code.layout_facts,
        &second.program.code.layout_facts,
    ));
    assert!(Arc::ptr_eq(
        &first.program.retained_demands,
        &second.program.retained_demands
    ));
    assert_eq!(
        first.program.code.revision(),
        second.program.code.revision()
    );
    assert_eq!(first.definitions().id(), second.definitions().id());
    assert_ne!(
        first.identity_admission().target().id(),
        second.identity_admission().target().id()
    );
    assert!(first_runtime.validate_executable_module(&first).is_ok());
    assert!(second_runtime.validate_executable_module(&second).is_ok());
    assert!(first_runtime.validate_executable_module(&second).is_err());
    assert!(second_runtime.validate_executable_module(&first).is_err());
    // A populated pure-fact cache must still honor each caller's cancellation.
    let cancellation = CancellationToken::default();
    cancellation.cancel();
    assert!(matches!(
        RetainedDemands::initial(first.verified_program(), &cancellation),
        Err(ReloadValidationError::Runtime(error)) if error.kind() == RuntimeErrorKind::Cancelled
    ));
    let history = RetainedDemands::initial(first.verified_program(), &Default::default()).unwrap();
    assert!(Arc::ptr_eq(&history, &first.program.retained_demands));
    let shared = ConstantId::new(0);
    let escaped = first_runtime
        .read_constant(&first, shared, RetentionOrigin::Current)
        .unwrap();
    let before = first_runtime.memory.stats();
    for _ in 0..100 {
        let repeated = first_runtime
            .read_constant(&first, shared, RetentionOrigin::Current)
            .unwrap();
        first_runtime
            .memory
            .scope(&first_runtime.resources, |cx, _| {
                assert!(
                    escaped
                        .get(cx)?
                        .object()
                        .unwrap()
                        .ptr_eq(repeated.get(cx)?.object().unwrap())
                );
                Ok(())
            })
            .unwrap();
    }
    assert!(
        first_runtime
            .read_constant(&second, shared, RetentionOrigin::Current)
            .is_err()
    );
    assert!(
        first_runtime
            .read_constant(&first, ConstantId::new(2), RetentionOrigin::Current)
            .is_err()
    );
    let after = first_runtime.memory.stats();
    assert_eq!(before.objects, after.objects);
    assert_eq!(before.root_registrations, after.root_registrations);
    let second_value = second_runtime
        .read_constant(&second, shared, RetentionOrigin::Current)
        .unwrap();
    for (runtime, value) in [
        (&mut first_runtime, &escaped),
        (&mut second_runtime, &second_value),
    ] {
        assert_constant_text(runtime, value, "shared");
    }
    assert!(
        second_runtime
            .memory
            .scope(&second_runtime.resources, |cx, _| escaped
                .get(cx)
                .map(|_| ()))
            .is_err()
    );
    ready(&mut first_runtime, &first, 0, &RootedValue::from(7i32));
    let first_snapshot = first_runtime.module_instance_snapshot(&first).unwrap();
    let StaticState::Ready(value) = &first_snapshot.module_slots[0] else {
        panic!("ready first instance");
    };
    assert_eq!(value.as_i32(), Some(7));
    assert!(matches!(
        second_runtime
            .module_instance_snapshot(&second)
            .unwrap()
            .module_slots[0],
        StaticState::Uninitialized(_)
    ));
    drop(first_snapshot);

    let pool_only = first_runtime
        .read_constant(&first, ConstantId::new(1), RetentionOrigin::Current)
        .unwrap();
    assert_constant_text(&mut first_runtime, &pool_only, "pool-only");
    drop(pool_only);
    first_runtime.collect_garbage().unwrap();
    let retained = first_runtime
        .retain_module(
            &first,
            ModuleEpochRetention::RuntimeValue,
            RetentionOrigin::Current,
        )
        .unwrap();
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
    first_runtime.collect_garbage().unwrap();
    assert!(first_runtime.modules().loaded(first.key()).is_some());
    assert!(
        first_runtime
            .read_constant(&first, shared, RetentionOrigin::Current)
            .is_err()
    );
    assert!(
        first_runtime
            .read_constant(&second, shared, RetentionOrigin::Retained(&retained))
            .is_err()
    );
    let exact = first_runtime
        .read_constant(&first, shared, RetentionOrigin::Retained(&retained))
        .unwrap();
    first_runtime
        .memory
        .scope(&first_runtime.resources, |cx, _| {
            assert!(
                exact
                    .get(cx)?
                    .object()
                    .unwrap()
                    .ptr_eq(escaped.get(cx)?.object().unwrap())
            );
            Ok(())
        })
        .unwrap();
    drop(exact);
    drop(retained);
    first_runtime.collect_garbage().unwrap();
    assert!(first_runtime.modules().loaded(first.key()).is_none());
    assert_constant_text(&mut first_runtime, &escaped, "shared");
    assert!(
        first_runtime
            .read_constant(&first, shared, RetentionOrigin::Current)
            .is_err()
    );
    let before_drop = first_runtime.memory.stats().objects;
    drop(escaped);
    first_runtime.collect_garbage().unwrap();
    assert_eq!(first_runtime.memory.stats().objects, before_drop - 1);
    second_runtime.collect_garbage().unwrap();
    assert_constant_text(&mut second_runtime, &second_value, "shared");
    let repeated = second_runtime
        .read_constant(&second, shared, RetentionOrigin::Current)
        .unwrap();
    second_runtime
        .memory
        .scope(&second_runtime.resources, |cx, _| {
            assert!(
                repeated
                    .get(cx)?
                    .object()
                    .unwrap()
                    .ptr_eq(second_value.get(cx)?.object().unwrap())
            );
            Ok(())
        })
        .unwrap();
}

fn assert_constant_text(runtime: &mut Runtime, value: &RootedValue, expected: &str) {
    runtime
        .memory
        .scope(&runtime.resources, |cx, _| {
            let object = cx.downcast::<StringObject>(value.get(cx)?.object().unwrap())?;
            cx.read(object, |object| assert_eq!(object.as_str(), expected));
            Ok(())
        })
        .unwrap();
}

#[test]
fn staged_programs_keep_instances_alive_without_activating_them() {
    let context = DefinitionContext::new().unwrap();
    let mut store = ModuleStore::default();
    let resources = &ResourceState::default();
    let mut memory = ManagedMemory::new(Default::default());
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
        if epoch == 2 {
            // Only the abandoned candidate introduces this initializer. Attaching
            // its new cell to current bindings would retain the entire candidate.
            add_static(
                &context,
                &mut root,
                "unpublished",
                Ty::Builtin(BuiltinType::I32),
                true,
                Visibility::Private,
            );
        }
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
        let admission = store
            .capture_current(&installation)
            .ok()
            .map(|baseline| validate_verified_reload_candidate(&baseline, &code).unwrap());
        {
            let code = code;
            let definitions =
                DefinitionAdmission::prepare(&code, &context, &Default::default()).unwrap();
            store.stage_verified_program(
                installation.clone(),
                "game.player",
                ModuleEpoch(epoch),
                code,
                LinkedProgramBindings {
                    definitions,
                    members: vec![LinkedHostBindings::default(), LinkedHostBindings::default()],
                    shared_traits: Default::default(),
                },
                admission,
            )
        }
        .unwrap()
    };
    let baseline = prepare(&store, &mut memory, stage(1), resources)
        .publish(&store, &mut memory, resources)
        .unwrap();
    let abandoned = prepare(&store, &mut memory, stage(2), resources);
    let candidate = prepare(&store, &mut memory, stage(3), resources);
    let abandoned_keys = abandoned
        .module
        .members()
        .map(|m| m.key())
        .collect::<Vec<_>>();
    memory
        .scope(resources, |cx, _| {
            let revision = cx.get(candidate.revision())?;
            for member in candidate.module.members() {
                let cell =
                    RevisionState::static_cell(cx, revision, member.slot(), ModuleSlot::new(0))?;
                let attempt = Arc::new(InitializationAttempt::new(
                    Arc::new(AtomicBool::new(false)),
                    None,
                ));
                StaticCellObject::begin(cx, cell, attempt.clone())?;
                StaticCellObject::complete(cx, cell, ScopedValue::from(73_i32), &attempt)?;
            }
            Ok(())
        })
        .unwrap();
    assert_eq!(
        store.capture_current(&installation).unwrap().key(),
        baseline.key()
    );
    assert_eq!(store.loaded_count(), 6);
    assert!(collect(&store, &mut memory, resources).is_empty());
    memory
        .scope(resources, |cx, _| {
            let revision = cx.get(candidate.revision())?;
            for member in candidate.module.members() {
                let cell =
                    RevisionState::static_cell(cx, revision, member.slot(), ModuleSlot::new(0))?;
                assert_eq!(
                    StaticCellObject::read(cx, cell)?.scalar_payload(),
                    ScopedValue::from(73_i32).scalar_payload()
                );
            }
            Ok(())
        })
        .unwrap();

    drop(abandoned);
    for key in &abandoned_keys {
        assert!(store.loaded(*key).is_none());
    }
    assert_eq!(store.loaded_count(), 4);
    assert_eq!(
        store.capture_current(&installation).unwrap().key(),
        baseline.key()
    );
    assert_eq!(
        collect(&store, &mut memory, resources)
            .into_iter()
            .collect::<HashSet<_>>(),
        abandoned_keys.into_iter().collect()
    );

    let published = candidate.publish(&store, &mut memory, resources).unwrap();
    assert_eq!(
        store.capture_current(&installation).unwrap().key(),
        published.key()
    );
    memory
        .scope(resources, |cx, _| {
            let current = store.current.borrow();
            let installation = cx.get(&current[&installation.id].root)?;
            let revision = InstallationState::current(cx, installation).unwrap();
            for member in published.members() {
                let cell =
                    RevisionState::static_cell(cx, revision, member.slot(), ModuleSlot::new(0))?;
                assert_eq!(
                    StaticCellObject::read(cx, cell)?.scalar_payload(),
                    ScopedValue::from(73_i32).scalar_payload()
                );
            }
            Ok(())
        })
        .unwrap();
    assert_eq!(collect(&store, &mut memory, resources).len(), 2);
    assert_eq!(store.loaded_count(), 2);
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
        let code = VerifiedProgram::new(
            ProgramFixture {
                exports: Default::default(),
                root: ModuleRef::new(0),
                modules: vec![BytecodeModule::default()],
            }
            .metadata(context.snapshot(), &Default::default())
            .unwrap(),
            &context,
        )
        .unwrap();
        let definitions =
            DefinitionAdmission::prepare(&code, &context, &Default::default()).unwrap();
        store.stage_verified_program(
            installation.clone(),
            "candidate",
            ModuleEpoch(epoch),
            code,
            LinkedProgramBindings {
                definitions,
                members: vec![LinkedHostBindings::default()],
                shared_traits: Default::default(),
            },
            None,
        )
    };
    let candidate = stage(1).unwrap();
    assert_eq!(store.loaded_count(), 1);
    assert!(store.capture_current(&installation).is_err());
    drop(candidate);
    assert_eq!(store.loaded_count(), 0);
    let candidate = stage(3).unwrap();
    runtime.resources().quarantine(EngineFault::HostInvariant(
        "test failed initializer invariant",
    ));
    drop(candidate);
    assert_eq!(store.loaded_count(), 0);
    assert!(runtime.is_quarantined());
}

#[test]
fn assigns_stable_module_ids_across_epochs() {
    let context = DefinitionContext::new().unwrap();
    let mut store = ModuleStore::default();
    let resources = &ResourceState::default();
    let mut memory = ManagedMemory::new(Default::default());
    let installation = store
        .reserve_installation(
            HostRegistryId::default(),
            BytecodeModule::default().identity,
        )
        .unwrap();
    let first = {
        let code = VerifiedProgram::new(
            ProgramFixture {
                exports: Default::default(),
                root: ModuleRef::new(0),
                modules: vec![BytecodeModule::default()],
            }
            .metadata(context.snapshot(), &Default::default())
            .unwrap(),
            &context,
        )
        .unwrap();
        let definitions =
            DefinitionAdmission::prepare(&code, &context, &Default::default()).unwrap();
        store.stage_verified_program(
            installation.clone(),
            "game.player",
            ModuleEpoch(1),
            code,
            LinkedProgramBindings {
                definitions,
                members: vec![LinkedHostBindings::default()],
                shared_traits: Default::default(),
            },
            None,
        )
    }
    .unwrap();
    let first = prepare(&store, &mut memory, first, resources)
        .publish(&store, &mut memory, resources)
        .unwrap();
    let second = {
        let code = VerifiedProgram::new(
            ProgramFixture {
                exports: Default::default(),
                root: ModuleRef::new(0),
                modules: vec![BytecodeModule::default()],
            }
            .metadata(context.snapshot(), &Default::default())
            .unwrap(),
            &context,
        )
        .unwrap();
        let definitions =
            DefinitionAdmission::prepare(&code, &context, &Default::default()).unwrap();
        store.stage_verified_program(
            installation.clone(),
            "game.player",
            ModuleEpoch(2),
            code,
            LinkedProgramBindings {
                definitions,
                members: vec![LinkedHostBindings::default()],
                shared_traits: Default::default(),
            },
            Some(validate_verified_reload_candidate(&first, first.verified_program()).unwrap()),
        )
    }
    .unwrap();
    let second = prepare(&store, &mut memory, second, resources)
        .publish(&store, &mut memory, resources)
        .unwrap();
    let other_installation = store
        .reserve_installation(
            HostRegistryId::default(),
            BytecodeModule::default().identity,
        )
        .unwrap();
    let other = {
        let code = VerifiedProgram::new(
            ProgramFixture {
                exports: Default::default(),
                root: ModuleRef::new(0),
                modules: vec![BytecodeModule::default()],
            }
            .metadata(context.snapshot(), &Default::default())
            .unwrap(),
            &context,
        )
        .unwrap();
        let definitions =
            DefinitionAdmission::prepare(&code, &context, &Default::default()).unwrap();
        store.stage_verified_program(
            other_installation,
            "game.world",
            ModuleEpoch(1),
            code,
            LinkedProgramBindings {
                definitions,
                members: vec![LinkedHostBindings::default()],
                shared_traits: Default::default(),
            },
            None,
        )
    }
    .unwrap();
    let other = prepare(&store, &mut memory, other, resources)
        .publish(&store, &mut memory, resources)
        .unwrap();

    assert_eq!(first.id, second.id);
    assert_ne!(first.id, other.id);
    assert_eq!(first.id.index(), 0);
    assert_eq!(other.id.index(), 1);
    assert_eq!(store.loaded_count(), 3);
    assert_eq!(
        store.capture_current(&installation).unwrap().epoch,
        ModuleEpoch(2)
    );
}

#[test]
fn keeps_latest_and_retained_old_epochs_reachable() {
    let context = DefinitionContext::new().unwrap();
    let mut store = ModuleStore::default();
    let resources = &ResourceState::default();
    let mut memory = ManagedMemory::new(Default::default());
    let installation = store
        .reserve_installation(
            HostRegistryId::default(),
            BytecodeModule::default().identity,
        )
        .unwrap();
    let first = {
        let code = VerifiedProgram::new(
            ProgramFixture {
                exports: Default::default(),
                root: ModuleRef::new(0),
                modules: vec![BytecodeModule::default()],
            }
            .metadata(context.snapshot(), &Default::default())
            .unwrap(),
            &context,
        )
        .unwrap();
        let definitions =
            DefinitionAdmission::prepare(&code, &context, &Default::default()).unwrap();
        store.stage_verified_program(
            installation.clone(),
            "game.player",
            ModuleEpoch(1),
            code,
            LinkedProgramBindings {
                definitions,
                members: vec![LinkedHostBindings::default()],
                shared_traits: Default::default(),
            },
            None,
        )
    }
    .unwrap();
    let first = prepare(&store, &mut memory, first, resources)
        .publish(&store, &mut memory, resources)
        .unwrap();
    let retained = memory
        .scope(resources, |cx, _| {
            let current = store.current.borrow();
            let installation = cx.get(&current[&installation.id].root)?;
            let revision = InstallationState::current(cx, installation).unwrap();
            cx.retain(revision)
        })
        .unwrap();
    let retained_copy = retained.clone();
    let second = {
        let code = VerifiedProgram::new(
            ProgramFixture {
                exports: Default::default(),
                root: ModuleRef::new(0),
                modules: vec![BytecodeModule::default()],
            }
            .metadata(context.snapshot(), &Default::default())
            .unwrap(),
            &context,
        )
        .unwrap();
        let definitions =
            DefinitionAdmission::prepare(&code, &context, &Default::default()).unwrap();
        store.stage_verified_program(
            installation.clone(),
            "game.player",
            ModuleEpoch(2),
            code,
            LinkedProgramBindings {
                definitions,
                members: vec![LinkedHostBindings::default()],
                shared_traits: Default::default(),
            },
            Some(validate_verified_reload_candidate(&first, first.verified_program()).unwrap()),
        )
    }
    .unwrap();
    let second = prepare(&store, &mut memory, second, resources)
        .publish(&store, &mut memory, resources)
        .unwrap();

    assert!(collect(&store, &mut memory, resources).is_empty());
    assert!(store.loaded(first.key()).is_some());
    memory
        .scope(resources, |cx, _| {
            let revision = cx.get(&retained)?;
            assert_eq!(
                cx.read(revision, |revision| revision.code().key()),
                first.key()
            );
            let installation = RevisionState::installation(cx, revision);
            let selected = InstallationState::current(cx, installation).unwrap();
            assert_eq!(
                cx.read(selected, |revision| revision.code().key()),
                second.key()
            );
            Ok(())
        })
        .unwrap();
    drop(retained);
    assert!(collect(&store, &mut memory, resources).is_empty());
    drop(retained_copy);
    assert_eq!(collect(&store, &mut memory, resources), vec![first.key()]);
    assert!(store.loaded(first.key()).is_none());
    assert!(store.loaded(second.key()).is_some());
    store.revoke_publications(&mut memory, resources).unwrap();
    assert_eq!(collect(&store, &mut memory, resources), vec![second.key()]);
    assert_eq!(store.loaded_count(), 0);
    assert_eq!(memory.stats().objects, 0);
    assert_eq!(memory.stats().root_registrations, 0);
    assert_eq!(memory.stats().provider_units, 0);
}

mod enum_declarations;

mod package_exports;
mod update_admission;

mod struct_layouts;
