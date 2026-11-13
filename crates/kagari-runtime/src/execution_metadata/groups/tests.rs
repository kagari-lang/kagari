use super::*;
#[cfg(test)]
use crate::test_support::bytecode::load_bytecode;
use crate::{
    Runtime, RuntimeConfig,
    error::RuntimeErrorKind,
    execution_metadata::{
        application_key::{ApplicationArguments, ApplicationKey, MethodIdentity},
        applications::{ApplicationRecord, MethodApplication},
    },
    frame::types::{
        EnvironmentRecord, construction::ConstructionScope, operations::OperationSegment,
    },
    module::{ExecutableModule, InstalledProgram},
    objects::interfaces::description::InterfaceResultBinding,
    value::carriers::RootedValue,
};
use crate::{
    execution::roots::SessionId, execution_metadata::managed::sources::MetadataSource,
    module::managed::source::RevisionSource,
};
use kagari_bytecode::module::CallableTarget;
use kagari_compiler::{bytecode::lower_program_to_bytecode, source::program::lower_program_to_mir};
use kagari_contract::{
    callable::witness::OperationWitness,
    effects::EffectSet,
    native_import::callables::{NativeCallableApplication, NativeCallableOrigin},
};
use kagari_hir::{analysis::AnalysisDatabase, compile_input::program::CheckedCompilation};
use kagari_source::source_database::{SourceDatabase, SourceLayer};
use kagari_types::{callable::CallableImplementation, ty::Ty};
use rstest::rstest;
use std::sync::Arc;

#[rstest]
#[case::automatic(Some(4))]
#[case::disabled(None)]
fn dictionary_objects_contribute_to_runtime_collection_pressure(#[case] threshold: Option<usize>) {
    let mut config = RuntimeConfig::default();
    config.gc.collection_threshold = threshold;
    let mut runtime = Runtime::new(config);
    for _ in 0..64 {
        let execution = runtime.memory.enter_execution(&runtime.resources).unwrap();
        runtime
            .alloc_operation_group(execution, vec![], &[])
            .unwrap();
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
fn dictionary_edges_survive_source_retirement_without_reviving_old_capabilities() {
    let mut runtime = Runtime::default();
    let source = runtime.memory.enter_execution(&runtime.resources).unwrap();
    let dictionary = runtime.alloc_operation_group(source, vec![], &[]).unwrap();
    let mut record =
        EnvironmentRecord::new(&runtime.definition_context().snapshot(), vec![], vec![]).unwrap();
    record.add_receiver(dictionary.record()).unwrap();
    let destination = runtime.memory.enter_execution(&runtime.resources).unwrap();
    let environment = runtime
        .alloc_environment(
            destination,
            record,
            &[MetadataSource::AdmittedOperations(&dictionary)],
        )
        .unwrap();
    runtime
        .memory
        .retire_execution(&runtime.resources, source)
        .unwrap();
    runtime.collect_garbage().unwrap();
    assert_eq!(runtime.gc_stats().objects, 2);
    assert_eq!(
        runtime
            .operation_group_record(&dictionary)
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::ModuleValidation
    );
    let snapshot = runtime.environment_snapshot(&environment).unwrap();
    assert_eq!(snapshot.groups.len(), 1);
    assert!(Arc::ptr_eq(
        &snapshot.groups[&dictionary.record().id()],
        dictionary.record()
    ));
    runtime
        .memory
        .retire_execution(&runtime.resources, destination)
        .unwrap();
    assert_eq!(runtime.collect_garbage().unwrap().retired_owners, 2);
    assert_eq!(runtime.gc_stats().objects, 0);
    // Immutable descriptors outlive GC without retaining their actual graph.
    assert_eq!(snapshot.groups.len(), 1);
    assert_eq!(runtime.gc_stats().root_registrations, 0);
    assert!(!runtime.is_quarantined());
}

#[test]
fn dictionary_sources_check_foreign_execution_reuse_and_exact_facts() {
    let mut runtime = Runtime::default();
    let mut foreign = Runtime::default();
    let source = runtime.memory.enter_execution(&runtime.resources).unwrap();
    let dictionary = runtime.alloc_operation_group(source, vec![], &[]).unwrap();
    assert_eq!(
        foreign
            .operation_group_record(&dictionary)
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::ModuleValidation
    );
    runtime
        .memory
        .retire_execution(&runtime.resources, source)
        .unwrap();
    let replacement_source = runtime.memory.enter_execution(&runtime.resources).unwrap();
    let replacement = runtime
        .alloc_operation_group(replacement_source, vec![], &[])
        .unwrap();
    assert_ne!(replacement.record().id(), dictionary.record().id());
    assert_eq!(
        runtime
            .operation_group_record(&dictionary)
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::ModuleValidation
    );
    let mismatched = AdmittedOperationGroup {
        record: dictionary.record.clone(),
        source: replacement.source,
    };
    let before = runtime.gc_stats();
    assert_eq!(
        runtime
            .operation_group_record(&mismatched)
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::ModuleValidation
    );
    assert_eq!(runtime.gc_stats(), before);
    assert!(Arc::ptr_eq(
        &runtime.operation_group_record(&replacement).unwrap(),
        replacement.record()
    ));
    runtime
        .memory
        .retire_execution(&runtime.resources, replacement_source)
        .unwrap();
    runtime.collect_garbage().unwrap();
    assert_eq!(runtime.gc_stats().objects, 0);
    assert!(!runtime.is_quarantined());
    assert!(!foreign.is_quarantined());
}

fn load() -> (Runtime, InstalledProgram, ExecutableModule) {
    load_source(include_str!(
        "../../../tests/fixtures/metadata/dictionary.kgr"
    ))
}

fn load_source(source: &str) -> (Runtime, InstalledProgram, ExecutableModule) {
    let mut sources = SourceDatabase::default();
    let root = sources
        .set("group.kgr", source.into(), SourceLayer::Base)
        .unwrap();
    let mut analysis = AnalysisDatabase::default();
    let snapshot = analysis
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    let checked = snapshot.check_program(root, &Default::default()).unwrap();
    let mir = lower_program_to_mir(
        &CheckedCompilation::from_program(&checked, &Default::default()).unwrap(),
        &Default::default(),
    )
    .unwrap();
    let code = lower_program_to_bytecode(&mir, &Default::default())
        .map(|program| program.into_unverified())
        .unwrap();
    let mut runtime = Runtime::default();
    let loaded_program = load_bytecode(&mut runtime, "group", code).unwrap();
    let loaded = runtime.select_program(&loaded_program).unwrap();
    (runtime, loaded_program, loaded)
}

#[test]
fn rooted_operation_application_releases_its_old_provider_graph_after_reload() {
    for selected in [false, true] {
        let (mut runtime, installed, loaded, execution, dictionary) = selections();
        let operation_id = dictionary.record().operation_ids().next().unwrap();
        let operation = dictionary.record().get(operation_id).unwrap();
        let mut record =
            EnvironmentRecord::new(&runtime.definition_context().snapshot(), vec![], vec![])
                .unwrap();
        if selected {
            record
                .add_operation(dictionary.record(), operation_id)
                .unwrap();
        } else {
            record.add_receiver(dictionary.record()).unwrap();
        }
        let environment = runtime
            .alloc_environment(
                execution,
                record,
                &[MetadataSource::AdmittedOperations(&dictionary)],
            )
            .unwrap();
        let application = runtime
            .alloc_application(
                execution,
                ApplicationRecord::Semantic(MethodApplication {
                    signature: operation.signature.clone(),
                    scoped_signature: None,
                    environment: Some(environment.facts().clone()),
                    result_adapter: None,
                }),
                &[MetadataSource::AdmittedEnvironment(&environment)],
            )
            .unwrap();
        let key = ApplicationKey::new(
            MethodIdentity::Operation(operation_id),
            &ApplicationArguments::new(&loaded, vec![], ConstructionScope::new(&loaded)).unwrap(),
            Some(dictionary.record().id()),
            &Default::default(),
        );
        runtime
            .publish_method_application(
                &loaded,
                key.clone(),
                &application,
                &[MetadataSource::AdmittedApplication(&application)],
            )
            .unwrap();
        runtime.collect_garbage().unwrap();
        let roots = runtime.gc_stats().root_registrations;
        let root = runtime.retain_application(&application).unwrap();
        runtime
            .memory
            .retire_execution(&runtime.resources, execution)
            .unwrap();
        let candidate = runtime
            .stage_reload_verified_program(&installed, None, loaded.verified_program().clone())
            .unwrap();
        runtime.publish_staged_reload(candidate).unwrap();
        let current = runtime.select_program(&installed).unwrap();
        runtime.collect_garbage().unwrap();
        assert!(runtime.modules().loaded(loaded.key()).is_some());
        assert_eq!(runtime.gc_stats().root_registrations, roots + 1);
        assert_eq!(
            runtime
                .operation_group_record(&dictionary)
                .unwrap_err()
                .kind(),
            RuntimeErrorKind::ModuleValidation
        );
        let facts = runtime
            .application_record_in(&[MetadataSource::HostApplication(&root)], application.id())
            .unwrap();
        let ApplicationRecord::Semantic(facts) = facts.as_ref() else {
            panic!("semantic operation application");
        };
        assert_eq!(facts.signature, operation.signature);
        assert_eq!(
            facts.environment.as_ref().unwrap().id,
            environment.facts().id
        );
        let snapshot = runtime
            .environment_snapshot_in(
                &[MetadataSource::HostApplication(&root)],
                environment.facts(),
            )
            .unwrap();
        assert!(snapshot.record.operations().validate(&snapshot.groups));
        assert!(Arc::ptr_eq(
            &snapshot.groups[&dictionary.record().id()],
            dictionary.record()
        ));
        assert_eq!(
            snapshot.operation(&operation.requirement_key, &operation.receiver_identity),
            Some(operation_id)
        );
        assert_eq!(
            runtime.modules.method_application(&loaded, &key),
            Some(application.id())
        );
        let live = runtime.gc_stats().objects;
        drop(root);
        let collected = runtime.collect_garbage().unwrap();
        assert!(
            collected.retired_owners >= 3,
            "application, environment and dictionary must retire"
        );
        assert!(runtime.gc_stats().objects <= live - 3);
        assert!(
            runtime.modules().loaded(loaded.key()).is_none(),
            "pure cache/descriptors must not keep the old supplying revision alive"
        );
        assert!(runtime.modules().loaded(current.key()).is_some());
        assert_eq!(runtime.gc_stats().root_registrations, roots);
        // Still-retained immutable records and snapshots are facts, not GC roots.
        assert_eq!(snapshot.groups.len(), 1);
        assert_eq!(facts.signature, operation.signature);
        assert!(!runtime.is_quarantined());
    }
}

fn selections() -> (
    Runtime,
    InstalledProgram,
    ExecutableModule,
    SessionId,
    AdmittedOperationGroup,
) {
    let (mut runtime, loaded_program, loaded) = load();
    let table = loaded
        .bytecode
        .interface_tables
        .iter()
        .position(|table| {
            table
                .methods
                .iter()
                .any(|method| loaded.definition_name(method.method) == Some("read"))
        })
        .unwrap();
    let binding = InterfaceResultBinding {
        owner: loaded.clone(),
        table,
        arguments: loaded.bytecode.interface_tables[table].arguments.clone(),
        environment: None,
    };
    let execution = runtime.memory.enter_execution(&runtime.resources).unwrap();
    let dictionary = runtime
        .bind_table_operations(
            execution,
            &binding,
            &[MetadataSource::Revision {
                owner: &loaded,
                source: RevisionSource::Current,
            }],
        )
        .unwrap();
    (runtime, loaded_program, loaded, execution, dictionary)
}

#[test]
fn selected_witnesses_keep_the_group_without_exposing_siblings() {
    let (mut runtime, _installed, _loaded, source, dictionary) = selections();
    runtime.collect_garbage().unwrap();
    let ids: Vec<_> = dictionary.record().operation_ids().collect();
    assert_eq!(ids.len(), 2);
    let first = dictionary.record().get(ids[0]).unwrap();
    let sibling = dictionary.record().get(ids[1]).unwrap();
    let destination = runtime.memory.enter_execution(&runtime.resources).unwrap();
    let mut environment =
        EnvironmentRecord::new(&runtime.definition_context().snapshot(), vec![], vec![]).unwrap();
    environment
        .add_operation(dictionary.record(), ids[0])
        .unwrap();
    let environment = runtime
        .alloc_environment(
            destination,
            environment,
            &[MetadataSource::AdmittedOperations(&dictionary)],
        )
        .unwrap();
    // Whole-receiver selection deliberately exposes its complete verified table.
    let mut receiver =
        EnvironmentRecord::new(&runtime.definition_context().snapshot(), vec![], vec![]).unwrap();
    receiver.add_receiver(dictionary.record()).unwrap();
    let receiver = runtime
        .alloc_environment(
            destination,
            receiver,
            &[MetadataSource::AdmittedOperations(&dictionary)],
        )
        .unwrap();
    let roots = runtime.gc_stats().root_registrations;
    runtime
        .memory
        .retire_execution(&runtime.resources, source)
        .unwrap();
    runtime.collect_garbage().unwrap();
    let selected = runtime.environment_snapshot(&environment).unwrap();
    assert!(selected.record.operations().validate(&selected.groups));
    assert_eq!(
        selected.operation(&first.requirement_key, &first.receiver_identity),
        Some(ids[0])
    );
    assert!(selected.groups[&ids[1].group].get(ids[1]).is_some());
    assert_eq!(
        selected.operation(&sibling.requirement_key, &sibling.receiver_identity),
        None
    );
    assert_eq!(
        selected.operation_slot(
            &sibling.receiver_identity,
            &sibling.interface_key,
            sibling.slot
        ),
        None
    );
    let whole = runtime.environment_snapshot(&receiver).unwrap();
    assert_eq!(
        whole.operation(&sibling.requirement_key, &sibling.receiver_identity),
        Some(ids[1])
    );
    assert_eq!(
        whole.operation_slot(
            &sibling.receiver_identity,
            &sibling.interface_key,
            sibling.slot
        ),
        Some(ids[1])
    );
    assert_eq!(runtime.gc_stats().root_registrations, roots);
    let objects = runtime.gc_stats().objects;
    runtime
        .memory
        .retire_execution(&runtime.resources, destination)
        .unwrap();
    // Both environments own the dictionary and its checked table environment.
    assert_eq!(runtime.collect_garbage().unwrap().retired_owners, 4);
    assert_eq!(runtime.gc_stats().objects, objects - 4);
    // Snapshots still contain immutable facts, but cannot reopen retired state.
    assert_eq!(selected.groups.len(), 1);
    assert_eq!(whole.groups.len(), 1);
    assert_eq!(
        runtime
            .environment_snapshot(&environment)
            .err()
            .unwrap()
            .kind(),
        RuntimeErrorKind::ModuleValidation
    );
    assert_eq!(
        runtime
            .environment_snapshot(&receiver)
            .err()
            .unwrap()
            .kind(),
        RuntimeErrorKind::ModuleValidation
    );
}

#[test]
fn operation_members_require_bounds_and_the_actual_supplying_dictionary() {
    let (mut runtime, _installed, _loaded, source, dictionary) = selections();
    let id = dictionary.record().operation_ids().next().unwrap();
    let invalid = OperationId {
        group: id.group,
        member: usize::MAX,
    };
    assert!(dictionary.record().get(invalid).is_none());
    let mut record =
        EnvironmentRecord::new(&runtime.definition_context().snapshot(), vec![], vec![]).unwrap();
    let types = record.types.clone();
    assert_eq!(
        record
            .add_operation(dictionary.record(), invalid)
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::ModuleValidation
    );
    assert!(record.operations().is_empty());
    assert!(Arc::ptr_eq(&record.types, &types));
    record.add_operation(dictionary.record(), id).unwrap();
    let destination = runtime.memory.enter_execution(&runtime.resources).unwrap();
    let before = runtime.gc_stats();
    assert_eq!(
        runtime
            .alloc_environment(destination, record.clone(), &[])
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::ModuleValidation
    );
    assert_eq!(runtime.gc_stats(), before);
    // Valid pure facts from an unrelated dictionary cannot satisfy the required ID.
    let other = runtime
        .alloc_operation_group(destination, vec![], &[])
        .unwrap();
    let before = runtime.gc_stats();
    assert_eq!(
        runtime
            .alloc_environment(
                destination,
                record.clone(),
                &[MetadataSource::AdmittedOperations(&other)]
            )
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::ModuleValidation
    );
    assert_eq!(runtime.gc_stats(), before);
    let environment = runtime
        .alloc_environment(
            destination,
            record,
            &[MetadataSource::AdmittedOperations(&dictionary)],
        )
        .unwrap();
    runtime
        .memory
        .retire_execution(&runtime.resources, source)
        .unwrap();
    runtime.collect_garbage().unwrap();
    runtime.validate_environment(&environment).unwrap();
    assert!(!runtime.is_quarantined());
    runtime
        .memory
        .retire_execution(&runtime.resources, destination)
        .unwrap();
    runtime.collect_garbage().unwrap();
}

#[test]
fn associated_type_snapshots_survive_selection_collection_without_owning_environments() {
    let (mut runtime, _installed, loaded) = load_source(include_str!(
        "../../../tests/fixtures/metadata/associated-dictionary.kgr"
    ));
    runtime.collect_garbage().unwrap();
    let baseline = runtime.gc_stats();
    let table = loaded
        .bytecode
        .interface_tables
        .iter()
        .position(|table| {
            table
                .methods
                .iter()
                .any(|method| loaded.definition_name(method.method) == Some("read"))
        })
        .unwrap();
    let value = runtime
        .make_interface(&loaded, table, &RootedValue::from(7i32))
        .unwrap();
    let description = runtime.interface_description(&value).unwrap();
    let execution = runtime.memory.enter_execution(&runtime.resources).unwrap();
    let dictionary = runtime
        .bind_table_operations(
            execution,
            &description.receiver_table,
            &[MetadataSource::RootedInterface(&value)],
        )
        .unwrap();
    let receiver = description.concrete_type.clone();
    let interface = description.interface_type().clone();
    let (member, expected) = interface.associated_types.iter().next().unwrap();
    let member = *member;
    let expected = expected.clone();
    drop((description, value));
    let mut record =
        EnvironmentRecord::new(&runtime.definition_context().snapshot(), vec![], vec![]).unwrap();
    let before = record.types.clone();
    record.add_receiver(dictionary.record()).unwrap();
    assert!(
        before
            .associated_output(&receiver, &interface, member)
            .is_none()
    );
    let environment = runtime
        .alloc_environment(
            execution,
            record,
            &[MetadataSource::AdmittedOperations(&dictionary)],
        )
        .unwrap();
    let mut child =
        EnvironmentRecord::new(&runtime.definition_context().snapshot(), vec![], vec![]).unwrap();
    child.include(Some(environment.facts().clone())).unwrap();
    let retained = child.types.clone();
    drop(child);
    runtime
        .memory
        .retire_execution(&runtime.resources, execution)
        .unwrap();
    assert!(runtime.collect_garbage().unwrap().retired_owners >= 2);
    assert_eq!(runtime.gc_stats().objects, baseline.objects);
    assert_eq!(
        runtime.gc_stats().root_registrations,
        baseline.root_registrations
    );
    assert_eq!(
        runtime
            .environment_snapshot(&environment)
            .err()
            .unwrap()
            .kind(),
        RuntimeErrorKind::ModuleValidation
    );
    let actual = retained
        .associated_output(&receiver, &interface, member)
        .unwrap();
    assert_eq!(actual.closed().unwrap().as_ref(), &expected);
    assert_eq!(actual.owner.key(), loaded.key());
    let projection = Ty::Projection {
        receiver: Box::new(receiver),
        interface: Box::new(interface),
        member,
        arguments: vec![],
    };
    assert_eq!(retained.resolve(&projection).unwrap(), expected);
    // Facts remain usable after collection, but cannot authenticate publication.
    let mut invalid =
        EnvironmentRecord::new(&runtime.definition_context().snapshot(), vec![], vec![]).unwrap();
    invalid.add_receiver(dictionary.record()).unwrap();
    assert!(!invalid.operations().is_empty());
    let destination = runtime.memory.enter_execution(&runtime.resources).unwrap();
    let before = runtime.gc_stats();
    for sources in [
        vec![],
        vec![MetadataSource::AdmittedOperations(&dictionary)],
    ] {
        let error = runtime
            .alloc_environment(destination, invalid.clone(), &sources)
            .unwrap_err();
        assert_eq!(error.kind(), RuntimeErrorKind::ModuleValidation);
        assert_eq!(runtime.gc_stats(), before);
    }
    runtime
        .memory
        .retire_execution(&runtime.resources, destination)
        .unwrap();
    assert!(!runtime.is_quarantined());
}

#[test]
fn witness_cache_reuses_protected_dictionaries_and_replaces_expired_optional_backing() {
    let (mut runtime, _installed, loaded, execution, dictionary) = selections();
    runtime.collect_garbage().unwrap();
    let operation = dictionary
        .record()
        .get(dictionary.record().operation_ids().next().unwrap())
        .unwrap();
    let CallableTarget::Script(function) = operation.target else {
        panic!("script fixture member")
    };
    let witness = OperationWitness::Selected(Box::new(NativeCallableApplication {
        origin: NativeCallableOrigin::Implementation,
        requirement: operation.requirement.clone(),
        instance: loaded.bytecode.functions[function.index()]
            .identity
            .clone()
            .unwrap(),
        implementation: CallableImplementation::Script,
        signature: operation.signature.clone(),
        effects: EffectSet::native_call(),
    }));
    assert!(witness.structurally_valid(&[], loaded.definitions()));
    let witnesses = [witness];
    let mut sources = vec![MetadataSource::Revision {
        owner: &loaded,
        source: RevisionSource::Current,
    }];
    let roots = runtime.gc_stats().root_registrations;
    let bindings = runtime
        .bind_operations_in(execution, &loaded, None, &witnesses, &mut sources)
        .unwrap();
    let identity = bindings.identity().unwrap();
    let [OperationSegment::Selected(first)] = identity.as_slice() else {
        panic!("one selected witness")
    };
    let before = runtime.gc_stats();
    let again = runtime
        .bind_operations_in(execution, &loaded, None, &witnesses, &mut sources)
        .unwrap();
    assert_eq!(again.identity().as_deref(), bindings.identity().as_deref());
    assert_eq!(runtime.gc_stats(), before);
    assert_eq!(
        runtime
            .publish_operation_bindings(&loaded, None, &witnesses, bindings.clone(), &sources)
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::ModuleValidation
    );
    assert_eq!(runtime.gc_stats(), before);
    runtime
        .memory
        .retire_execution(&runtime.resources, execution)
        .unwrap();
    runtime.collect_garbage().unwrap();
    let objects = runtime.gc_stats().objects;
    // The bounded cache keeps pure facts, not executable graph ownership.
    assert!(!runtime.operations_are_protected(&[], &bindings).unwrap());
    assert_eq!(
        runtime
            .operations_are_protected(&sources, &bindings)
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::ModuleValidation
    );
    let replacement_execution = runtime.memory.enter_execution(&runtime.resources).unwrap();
    let mut sources = vec![MetadataSource::Revision {
        owner: &loaded,
        source: RevisionSource::Current,
    }];
    let replacement = runtime
        .bind_operations_in(
            replacement_execution,
            &loaded,
            None,
            &witnesses,
            &mut sources,
        )
        .unwrap();
    let replacement_identity = replacement.identity().unwrap();
    let [OperationSegment::Selected(next)] = replacement_identity.as_slice() else {
        panic!("replacement witness")
    };
    assert_ne!(first.group, next.group);
    assert_eq!(runtime.gc_stats().objects, objects + 1);
    let before = runtime.gc_stats();
    let again = runtime
        .bind_operations_in(
            replacement_execution,
            &loaded,
            None,
            &witnesses,
            &mut sources,
        )
        .unwrap();
    assert_eq!(
        again.identity().as_deref(),
        replacement.identity().as_deref()
    );
    assert_eq!(runtime.gc_stats(), before);
    assert_eq!(runtime.gc_stats().root_registrations, roots);
    runtime
        .memory
        .retire_execution(&runtime.resources, replacement_execution)
        .unwrap();
    runtime.collect_garbage().unwrap();
    assert_eq!(runtime.gc_stats().objects, objects);
    assert!(!runtime.is_quarantined());
}
