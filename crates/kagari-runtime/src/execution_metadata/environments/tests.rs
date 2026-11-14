use super::*;
use crate::{
    RuntimeConfig, error::RuntimeErrorKind, frame::types::construction::ConstructionScope,
    module::managed::source::RevisionSource,
    objects::interfaces::description::InterfaceResultBinding,
    test_support::update_programs as programs,
};
use kagari_bytecode::{
    instruction::{BytecodeInstruction, CallTarget},
    module::CallableTarget,
};
use kagari_types::{scalar::BuiltinType, ty::Ty};
use rstest::rstest;

fn empty(runtime: &Runtime) -> EnvironmentRecord {
    EnvironmentRecord::new(&runtime.definition_context().snapshot(), vec![], vec![]).unwrap()
}

fn with_parent(runtime: &Runtime, parent: &AdmittedEnvironment) -> EnvironmentRecord {
    let mut record = empty(runtime);
    record.include(Some(parent.facts().clone())).unwrap();
    record
}

#[test]
fn actual_parent_edges_survive_independent_sources_and_only_genuine_host_roots_reopen_them() {
    let mut runtime = Runtime::default();
    let parent_source = runtime.memory.enter_execution(&runtime.resources).unwrap();
    let record = empty(&runtime);
    let parent = runtime
        .alloc_environment(parent_source, record, &[])
        .unwrap();
    let child_source = runtime.memory.enter_execution(&runtime.resources).unwrap();
    let record = with_parent(&runtime, &parent);
    let child = runtime
        .alloc_environment(
            child_source,
            record,
            &[MetadataSource::Environment(parent.source())],
        )
        .unwrap();
    let detached = runtime.environment_record(&child).unwrap();
    // A frame publishes its own typed environment coordinate. Retiring the
    // supplying source must not retire that frame's graph or register a Host root.
    let frame_source = runtime.memory.enter_execution(&runtime.resources).unwrap();
    let (mark, frame_environment, completion_environment) = runtime
        .memory
        .scope(&runtime.resources, |cx, roots| {
            ExecutionRoots::with(roots, cx, |slots| {
                let environment = child.resolve(slots)?;
                // Async result conversion belongs to the invocation prefix,
                // which must survive retirement of its factory frame.
                let completion = AdmittedEnvironment::publish(slots, frame_source, environment)?;
                let mark = slots.enter_frame(frame_source, 0, 0, || Ok::<_, GcError>(()))?;
                let environment = AdmittedEnvironment::publish(slots, frame_source, environment)?;
                Ok((mark, environment, completion))
            })
        })
        .unwrap();
    assert_eq!(frame_environment.facts().id, child.facts().id);
    assert_eq!(frame_environment.source().execution(), frame_source);
    assert_eq!(child.source().execution(), child_source);
    assert!(Arc::ptr_eq(
        &frame_environment.facts().types,
        &child.facts().types
    ));
    assert_eq!(runtime.gc_stats().root_registrations, 0);
    runtime
        .memory
        .retire_execution(&runtime.resources, parent_source)
        .unwrap();
    runtime
        .memory
        .retire_execution(&runtime.resources, child_source)
        .unwrap();
    runtime.collect_garbage().unwrap();
    assert_eq!(runtime.gc_stats().objects, 2);
    runtime.validate_environment(&frame_environment).unwrap();
    assert_eq!(
        runtime.validate_environment(&child).unwrap_err().kind(),
        RuntimeErrorKind::ModuleValidation
    );
    let root = runtime.retain_environment(&frame_environment).unwrap();
    runtime
        .memory
        .scope(&runtime.resources, |cx, roots| {
            ExecutionRoots::with(roots, cx, |slots| slots.retire_frame(mark))
        })
        .unwrap();
    assert_eq!(
        runtime
            .validate_environment(&frame_environment)
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::ModuleValidation
    );
    runtime
        .validate_environment(&completion_environment)
        .unwrap();
    runtime
        .memory
        .retire_execution(&runtime.resources, frame_source)
        .unwrap();
    assert_eq!(
        runtime
            .validate_environment(&completion_environment)
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::ModuleValidation
    );
    runtime.collect_garbage().unwrap();
    assert_eq!(runtime.gc_stats().objects, 2);
    assert_eq!(runtime.gc_stats().root_registrations, 1);
    assert_eq!(
        runtime.validate_environment(&child).unwrap_err().kind(),
        RuntimeErrorKind::ModuleValidation
    );
    let new_source = runtime.memory.enter_execution(&runtime.resources).unwrap();
    let record = with_parent(&runtime, &child);
    assert_eq!(
        runtime
            .alloc_environment(
                new_source,
                record,
                &[MetadataSource::Environment(child.source())]
            )
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::ModuleValidation
    );
    assert!(
        runtime
            .copy_environment_in(new_source, child.facts(), &[])
            .unwrap()
            .is_none()
    );
    let objects = runtime.gc_stats().objects;
    let copied = runtime
        .copy_environment_in(
            new_source,
            child.facts(),
            &[MetadataSource::HostEnvironment(&root)],
        )
        .unwrap()
        .unwrap();
    assert_eq!(copied.facts().id, child.facts().id);
    assert_eq!(runtime.gc_stats().objects, objects);
    assert_eq!(runtime.gc_stats().root_registrations, 1);
    let record = with_parent(&runtime, &child);
    let grandchild = runtime
        .alloc_environment(
            new_source,
            record,
            &[MetadataSource::AdmittedEnvironment(&copied)],
        )
        .unwrap();
    drop(root);
    runtime.collect_garbage().unwrap();
    assert_eq!(runtime.gc_stats().objects, 3);
    assert_eq!(runtime.gc_stats().root_registrations, 0);
    runtime.validate_environment(&grandchild).unwrap();
    runtime.validate_environment(&copied).unwrap();
    assert_eq!(
        runtime.validate_environment(&child).unwrap_err().kind(),
        RuntimeErrorKind::ModuleValidation
    );
    runtime
        .memory
        .retire_execution(&runtime.resources, new_source)
        .unwrap();
    runtime.collect_garbage().unwrap();
    assert_eq!(runtime.gc_stats().objects, 0);
    // Pure facts can outlive collection without keeping a single managed node
    // alive or authorizing access through the former source coordinate.
    assert!(Arc::ptr_eq(&detached.types, &child.facts().types));
    assert_eq!(
        runtime.validate_environment(&child).unwrap_err().kind(),
        RuntimeErrorKind::ModuleValidation
    );
    drop(detached);
    assert!(!runtime.is_quarantined());
}

#[test]
fn environment_publication_admits_original_sources_before_allocating_or_reusing_storage() {
    let mut runtime = Runtime::default();
    let execution = runtime.memory.enter_execution(&runtime.resources).unwrap();
    let record = empty(&runtime);
    let parent = runtime.alloc_environment(execution, record, &[]).unwrap();
    let before = runtime.gc_stats();
    let record = with_parent(&runtime, &parent);
    assert_eq!(
        runtime
            .alloc_environment(execution, record, &[])
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::ModuleValidation
    );
    assert_eq!(runtime.gc_stats().objects, before.objects);
    let mismatched_facts = TypeEnvironment {
        id: parent.facts().id,
        types: Arc::new(parent.facts().types.as_ref().clone()),
    };
    assert_eq!(
        runtime
            .copy_environment_in(
                execution,
                &mismatched_facts,
                &[MetadataSource::AdmittedEnvironment(&parent)]
            )
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::ModuleValidation
    );
    assert_eq!(runtime.gc_stats().objects, before.objects);
    assert_eq!(
        runtime.gc_stats().root_registrations,
        before.root_registrations
    );
    let mut foreign = Runtime::default();
    let foreign_execution = foreign.memory.enter_execution(&foreign.resources).unwrap();
    let record = empty(&foreign);
    let foreign_parent = foreign
        .alloc_environment(foreign_execution, record, &[])
        .unwrap();
    let record = with_parent(&runtime, &parent);
    assert_eq!(
        runtime
            .alloc_environment(
                execution,
                record,
                &[MetadataSource::Environment(foreign_parent.source())]
            )
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::ModuleValidation
    );
    assert_eq!(runtime.gc_stats().objects, before.objects);
    runtime.validate_environment(&parent).unwrap();
    runtime
        .memory
        .retire_execution(&runtime.resources, execution)
        .unwrap();
    let replacement_execution = runtime.memory.enter_execution(&runtime.resources).unwrap();
    let record = empty(&runtime);
    let replacement = runtime
        .alloc_environment(replacement_execution, record, &[])
        .unwrap();
    let mismatched = AdmittedEnvironment {
        facts: parent.facts().clone(),
        source: replacement.source(),
    };
    assert_eq!(
        runtime
            .validate_environment(&mismatched)
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::ModuleValidation
    );
    let record = with_parent(&runtime, &parent);
    let before = runtime.gc_stats().objects;
    assert_eq!(
        runtime
            .alloc_environment(
                replacement_execution,
                record,
                &[MetadataSource::Environment(parent.source())]
            )
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::ModuleValidation
    );
    assert_eq!(runtime.gc_stats().objects, before);
    assert_eq!(
        runtime
            .copy_environment_in(
                replacement_execution,
                parent.facts(),
                &[MetadataSource::Environment(parent.source())]
            )
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::ModuleValidation
    );
    runtime.validate_environment(&replacement).unwrap();
    runtime
        .memory
        .retire_execution(&runtime.resources, replacement_execution)
        .unwrap();
    foreign
        .memory
        .retire_execution(&foreign.resources, foreign_execution)
        .unwrap();
    runtime.collect_garbage().unwrap();
    foreign.collect_garbage().unwrap();
    assert_eq!(runtime.gc_stats().objects, 0);
    assert_eq!(foreign.gc_stats().objects, 0);
    assert!(!runtime.is_quarantined());
}

#[rstest]
#[case::automatic(Some(4))]
#[case::disabled(None)]
fn unreachable_environment_objects_contribute_to_runtime_collection_pressure(
    #[case] threshold: Option<usize>,
) {
    let mut config = RuntimeConfig::default();
    config.gc.collection_threshold = threshold;
    let mut runtime = Runtime::new(config);
    for _ in 0..64 {
        let execution = runtime.memory.enter_execution(&runtime.resources).unwrap();
        let record = empty(&runtime);
        runtime.alloc_environment(execution, record, &[]).unwrap();
        runtime
            .memory
            .retire_execution(&runtime.resources, execution)
            .unwrap();
        runtime.gc_safepoint().unwrap();
    }
    let stats = runtime.gc_stats();
    if threshold.is_some() {
        assert!(stats.completed_cycles > 0);
        assert!(stats.objects < 64);
    } else {
        assert_eq!(stats.completed_cycles, 0);
        assert_eq!(stats.objects, 64);
    }
    assert_eq!(stats.root_registrations, 0);
    runtime.collect_garbage().unwrap();
    assert_eq!(runtime.gc_stats().objects, 0);
}

#[test]
fn an_admitted_environment_handle_cannot_keep_runtime_alive_or_authorize_foreign_access() {
    let mut runtime = Runtime::default();
    let execution = runtime.memory.enter_execution(&runtime.resources).unwrap();
    let record = empty(&runtime);
    let environment = runtime.alloc_environment(execution, record, &[]).unwrap();
    let owner = runtime.resources().lifetime_probe();
    drop(runtime);
    assert!(owner.upgrade().is_none());
    let mut foreign = Runtime::default();
    assert_eq!(
        foreign
            .validate_environment(&environment)
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::ModuleValidation
    );
    assert!(!foreign.is_quarantined());
}

#[test]
fn transported_environments_keep_lexical_providers_and_retire_with_preparation() {
    let mut runtime = Runtime::default();
    let installed = runtime
        .load_verified_program(
            "environment-transport",
            programs::source_code(include_str!(
                "../../../tests/fixtures/metadata/dictionary.kgr"
            )),
        )
        .unwrap();
    let old = runtime.select_program(&installed).unwrap();
    let (owner, function) = old.exported_function("main").unwrap();
    let route = owner
        .bind_call_route(CallableTarget::Script(function))
        .unwrap();
    let execution = runtime.memory.enter_execution(&runtime.resources).unwrap();
    let table = old
        .bytecode
        .interface_tables
        .iter()
        .position(|table| {
            table
                .methods
                .iter()
                .any(|method| old.definition_name(method.method) == Some("read"))
        })
        .unwrap();
    let binding = InterfaceResultBinding {
        owner: old.clone(),
        table,
        arguments: old.bytecode.interface_tables[table].arguments.clone(),
        environment: None,
    };
    let dictionary = runtime
        .bind_table_operations(
            execution,
            &binding,
            &[MetadataSource::Revision {
                owner: &old,
                source: RevisionSource::Current,
            }],
        )
        .unwrap();
    let mut record = empty(&runtime);
    record.set_constructions(ConstructionScope::new(&old));
    record.add_receiver(dictionary.record()).unwrap();
    let environment = runtime
        .alloc_environment(
            execution,
            record,
            &[MetadataSource::AdmittedOperations(&dictionary)],
        )
        .unwrap();
    let candidate = runtime
        .stage_reload_verified_program(
            &installed,
            None,
            programs::source_code(include_str!(
                "../../../tests/fixtures/metadata/dictionary_updated.kgr"
            )),
        )
        .unwrap();
    runtime.publish_staged_reload(candidate).unwrap();
    let current = runtime.select_program(&installed).unwrap();
    let selected = runtime.select_call_route(&route).unwrap();
    assert_eq!(
        selected.owner().program_identity(),
        current.program_identity()
    );
    assert_ne!(selected.owner().program_identity(), old.program_identity());
    runtime.collect_garbage().unwrap();
    let roots = runtime.gc_stats().root_registrations;
    let preparation = runtime
        .memory
        .scope(&runtime.resources, |cx, roots| {
            ExecutionRoots::with(roots, cx, |slots| slots.mark_locals(execution))
        })
        .unwrap();
    let transported = runtime
        .transport_environment(execution, &old, &selected, &environment)
        .unwrap();
    assert_ne!(environment.facts().id, transported.facts().id);
    let record = runtime.environment_record(&transported).unwrap();
    assert_eq!(record.constructions.programs().len(), 1);
    assert_eq!(
        record.constructions.programs()[0].program_identity(),
        old.program_identity()
    );
    let snapshot = runtime.environment_snapshot(&transported).unwrap();
    assert_eq!(snapshot.groups.len(), 1);
    assert!(Arc::ptr_eq(
        &snapshot.groups[&dictionary.record().id()],
        dictionary.record()
    ));
    let mapping = selected
        .mapping_from(old.publication_definitions())
        .unwrap();
    for id in dictionary.record().operation_ids() {
        let operation = dictionary.record().get(id).unwrap();
        assert_eq!(operation.owner.program_identity(), old.program_identity());
        assert_eq!(
            snapshot.operation_slot(
                &operation.receiver_identity.transported(&mapping).unwrap(),
                &mapping
                    .remap_closed(&operation.interface_key, &Default::default())
                    .unwrap(),
                operation.slot,
            ),
            Some(id)
        );
    }
    let objects = runtime.gc_stats().objects;
    let unchanged = runtime
        .transport_environment(execution, &current, &selected, &transported)
        .unwrap();
    assert_eq!(unchanged.facts().id, transported.facts().id);
    assert_eq!(runtime.gc_stats().objects, objects);
    assert_eq!(runtime.gc_stats().root_registrations, roots);
    runtime.collect_garbage().unwrap();
    let objects = runtime.gc_stats().objects;
    runtime
        .memory
        .scope(&runtime.resources, |cx, roots| {
            ExecutionRoots::with(roots, cx, |slots| slots.retire_locals(preparation))
        })
        .unwrap();
    assert_eq!(runtime.collect_garbage().unwrap().retired_owners, 1);
    assert_eq!(runtime.gc_stats().objects, objects - 1);
    assert_eq!(
        runtime
            .validate_environment(&transported)
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::ModuleValidation
    );
    runtime.validate_environment(&environment).unwrap();
    assert!(runtime.modules.loaded(old.key()).is_some());
    runtime
        .memory
        .retire_execution(&runtime.resources, execution)
        .unwrap();
    runtime.collect_garbage().unwrap();
    assert!(runtime.modules.loaded(old.key()).is_none());
    assert_eq!(runtime.gc_stats().root_registrations, roots);
}

#[test]
fn shared_preparation_reuses_protected_environments_and_reconstructs_expired_cache_backing() {
    let mut runtime = Runtime::default();
    let installed = runtime
        .load_verified_program(
            "shared-environment",
            programs::source_code(include_str!(
                "../../../tests/fixtures/metadata/applications.kgr"
            )),
        )
        .unwrap();
    let loaded = runtime.select_program(&installed).unwrap();
    let (parameters, module, target, contract) = loaded
        .bytecode
        .functions
        .iter()
        .find_map(|function| {
            function
                .instructions
                .iter()
                .find_map(|instruction| match instruction {
                    BytecodeInstruction::Call {
                        callee:
                            CallTarget::Shared {
                                module,
                                target,
                                contract,
                            },
                        ..
                    } => Some((
                        function
                            .metadata
                            .semantic
                            .generic
                            .as_ref()
                            .unwrap()
                            .parameters
                            .clone(),
                        *module,
                        *target,
                        contract.clone(),
                    )),
                    _ => None,
                })
        })
        .unwrap();
    let owner = loaded.member(module).unwrap();
    let selected = runtime
        .select_call_route(&owner.bind_call_route(target).unwrap())
        .unwrap();
    runtime.collect_garbage().unwrap();
    let objects = runtime.gc_stats().objects;
    let roots = runtime.gc_stats().root_registrations;
    let execution = runtime.memory.enter_execution(&runtime.resources).unwrap();
    let types = vec![Ty::Builtin(BuiltinType::I32); parameters.len()];
    let arguments = runtime.resolve_type_arguments(&loaded, &types).unwrap();
    let mut record = EnvironmentRecord::new(loaded.definitions(), parameters, arguments).unwrap();
    record.set_constructions(ConstructionScope::new(&loaded));
    let caller = runtime
        .alloc_environment(
            execution,
            record,
            &[MetadataSource::Revision {
                owner: &loaded,
                source: RevisionSource::Current,
            }],
        )
        .unwrap();
    let mark = runtime
        .memory
        .scope(&runtime.resources, |cx, roots| {
            ExecutionRoots::with(roots, cx, |slots| slots.mark_locals(execution))
        })
        .unwrap();
    let mut sources = vec![MetadataSource::AdmittedEnvironment(&caller)];
    let prepared = runtime
        .prepare_selected_shared_environment(
            execution,
            &loaded,
            Some(caller.facts().clone()),
            &selected,
            &contract,
            &mut sources,
        )
        .unwrap();
    let CallableTarget::Script(function) = selected.target() else {
        panic!("shared script fixture")
    };
    let result = selected.owner().bytecode.functions[function.index()]
        .metadata
        .semantic
        .result
        .as_ref()
        .unwrap();
    assert_eq!(
        prepared.facts().types.resolve(result).unwrap(),
        Ty::Builtin(BuiltinType::I32)
    );
    assert!(prepared.facts().types.matches(selected.generic().unwrap()));
    let before = runtime.gc_stats().objects;
    let again = runtime
        .prepare_selected_shared_environment(
            execution,
            &loaded,
            Some(caller.facts().clone()),
            &selected,
            &contract,
            &mut sources,
        )
        .unwrap();
    assert_eq!(again.facts().id, prepared.facts().id);
    assert_eq!(runtime.gc_stats().objects, before);
    assert_eq!(runtime.gc_stats().root_registrations, roots);
    runtime
        .memory
        .scope(&runtime.resources, |cx, roots| {
            ExecutionRoots::with(roots, cx, |slots| slots.retire_locals(mark))
        })
        .unwrap();
    assert_eq!(runtime.collect_garbage().unwrap().retired_owners, 1);
    assert_eq!(
        runtime.validate_environment(&prepared).unwrap_err().kind(),
        RuntimeErrorKind::ModuleValidation
    );
    assert_eq!(
        runtime
            .prepare_selected_shared_environment(
                execution,
                &loaded,
                Some(caller.facts().clone()),
                &selected,
                &contract,
                &mut sources
            )
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::ModuleValidation
    );
    // The caller remains valid; its optional cache does not own the old result.
    let mut sources = vec![MetadataSource::AdmittedEnvironment(&caller)];
    let replacement = runtime
        .prepare_selected_shared_environment(
            execution,
            &loaded,
            Some(caller.facts().clone()),
            &selected,
            &contract,
            &mut sources,
        )
        .unwrap();
    assert_ne!(replacement.facts().id, prepared.facts().id);
    // Reusing the same execution suffix cannot revive an old admitted handle.
    assert_eq!(
        runtime.validate_environment(&prepared).unwrap_err().kind(),
        RuntimeErrorKind::ModuleValidation
    );
    assert_eq!(
        runtime.retain_environment(&prepared).unwrap_err().kind(),
        RuntimeErrorKind::ModuleValidation
    );
    assert_eq!(
        replacement.facts().types.resolve(result).unwrap(),
        Ty::Builtin(BuiltinType::I32)
    );
    assert_eq!(runtime.gc_stats().root_registrations, roots);
    runtime
        .memory
        .retire_execution(&runtime.resources, execution)
        .unwrap();
    runtime.collect_garbage().unwrap();
    assert_eq!(runtime.gc_stats().objects, objects);
    assert!(!runtime.is_quarantined());
}
