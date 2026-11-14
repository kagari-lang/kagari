use super::*;
use crate::{
    RuntimeConfig, error::RuntimeErrorKind, frame::types::EnvironmentRecord,
    module::managed::source::RevisionSource, test_support::update_programs as programs,
    value::carriers::RootedValue,
};
use kagari_types::ty::Ty;
use rstest::rstest;

fn application(environment: Option<TypeEnvironment>) -> ApplicationRecord {
    ApplicationRecord::Semantic(MethodApplication {
        signature: Signature {
            params: vec![],
            result: Ty::Tuple(vec![]),
        },
        scoped_signature: None,
        environment,
        result_adapter: None,
    })
}

#[test]
fn rooted_application_keeps_its_actual_environment_edges_until_last_lease() {
    let mut runtime = Runtime::default();
    let execution = runtime.memory.enter_execution(&runtime.resources).unwrap();
    let group = runtime
        .alloc_operation_group(execution, vec![], &[])
        .unwrap();
    let mut record =
        EnvironmentRecord::new(&runtime.definition_context().snapshot(), vec![], vec![]).unwrap();
    record.add_receiver(group.record()).unwrap();
    let environment = runtime
        .alloc_environment(
            execution,
            record,
            &[MetadataSource::AdmittedOperations(&group)],
        )
        .unwrap();
    let prepared = runtime
        .alloc_application(
            execution,
            application(Some(environment.facts().clone())),
            &[MetadataSource::AdmittedEnvironment(&environment)],
        )
        .unwrap();
    let root = runtime
        .memory
        .scope(&runtime.resources, |cx, roots| {
            ExecutionRoots::with(roots, cx, |slots| {
                let value = slots.dependency(prepared.source())?;
                slots.scope().retain(value)
            })
        })
        .unwrap();
    let last_lease = root.clone();
    runtime
        .memory
        .retire_execution(&runtime.resources, execution)
        .unwrap();
    runtime.collect_garbage().unwrap();
    assert_eq!(runtime.gc_stats().objects, 3);
    assert_eq!(runtime.gc_stats().root_registrations, 1);
    assert_eq!(
        runtime.application_record(&prepared).unwrap_err().kind(),
        RuntimeErrorKind::ModuleValidation
    );
    let facts = runtime
        .application_record_in(&[MetadataSource::HostApplication(&root)], prepared.id())
        .unwrap();
    let ApplicationRecord::Semantic(facts) = facts.as_ref() else {
        panic!("semantic application")
    };
    assert!(facts.signature.params.is_empty());
    assert_eq!(facts.signature.result, Ty::Tuple(vec![]));
    assert_eq!(
        facts.environment.as_ref().unwrap().id,
        environment.facts().id
    );
    let detached = runtime
        .environment_snapshot_in(
            &[MetadataSource::HostApplication(&root)],
            environment.facts(),
        )
        .unwrap();
    assert_eq!(detached.groups.len(), 1);
    assert!(Arc::ptr_eq(
        &detached.groups[&group.record().id()],
        group.record()
    ));
    drop(root);
    runtime.collect_garbage().unwrap();
    assert_eq!(runtime.gc_stats().objects, 3);
    assert_eq!(runtime.gc_stats().root_registrations, 1);
    drop(last_lease);
    runtime.collect_garbage().unwrap();
    assert_eq!(runtime.gc_stats().objects, 0);
    assert_eq!(runtime.gc_stats().root_registrations, 0);
    // Pure descriptor snapshots retain values/types, never the managed graph.
    assert_eq!(detached.groups.len(), 1);
    assert!(!runtime.is_quarantined());
}

#[rstest]
#[case::automatic(Some(4))]
#[case::disabled(None)]
fn application_only_growth_is_collected_at_safepoints_unless_disabled(
    #[case] threshold: Option<usize>,
) {
    let mut config = RuntimeConfig::default();
    config.gc.collection_threshold = threshold;
    let mut runtime = Runtime::new(config);
    for _ in 0..64 {
        let execution = runtime.memory.enter_execution(&runtime.resources).unwrap();
        runtime
            .alloc_application(execution, application(None), &[])
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
fn application_sources_reject_foreign_reused_and_mismatched_facts_before_mutation() {
    let mut runtime = Runtime::default();
    let mut foreign = Runtime::default();
    let execution = runtime.memory.enter_execution(&runtime.resources).unwrap();
    let prepared = runtime
        .alloc_application(execution, application(None), &[])
        .unwrap();
    assert_eq!(
        foreign.application_record(&prepared).unwrap_err().kind(),
        RuntimeErrorKind::ModuleValidation
    );
    runtime
        .memory
        .retire_execution(&runtime.resources, execution)
        .unwrap();
    let replacement_execution = runtime.memory.enter_execution(&runtime.resources).unwrap();
    let replacement = runtime
        .alloc_application(replacement_execution, application(None), &[])
        .unwrap();
    assert_ne!(prepared.id(), replacement.id());
    assert_eq!(
        runtime.application_record(&prepared).unwrap_err().kind(),
        RuntimeErrorKind::ModuleValidation
    );
    let wrong_id = AdmittedApplication {
        id: prepared.id(),
        ..replacement.clone()
    };
    let wrong_record = AdmittedApplication {
        record: prepared.record.clone(),
        ..replacement.clone()
    };
    let before = runtime.gc_stats();
    for invalid in [&wrong_id, &wrong_record] {
        assert_eq!(
            runtime.application_record(invalid).unwrap_err().kind(),
            RuntimeErrorKind::ModuleValidation
        );
        assert_eq!(runtime.gc_stats(), before);
    }
    assert_eq!(
        runtime
            .application_record_in(&[], replacement.id())
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::ModuleValidation
    );
    assert_eq!(runtime.gc_stats(), before);
    assert!(Arc::ptr_eq(
        &runtime.application_record(&replacement).unwrap(),
        replacement.record()
    ));
    runtime
        .memory
        .retire_execution(&runtime.resources, replacement_execution)
        .unwrap();
    runtime.collect_garbage().unwrap();
    assert_eq!(runtime.gc_stats().objects, 0);
    assert!(!runtime.is_quarantined());
    assert!(!foreign.is_quarantined());
}

#[test]
fn admitted_application_retains_exact_body_without_using_current_as_an_old_source() {
    let mut runtime = Runtime::default();
    let installed = runtime
        .load_verified_program(
            "application-body",
            programs::source_code(include_str!(
                "../../../tests/fixtures/metadata/interface-parents.kgr"
            )),
        )
        .unwrap();
    let old = runtime.select_program(&installed).unwrap();
    runtime.collect_garbage().unwrap();
    let roots = runtime.gc_stats().root_registrations;
    let interface = runtime
        .make_interface(&old, 0, &RootedValue::from(7_i32))
        .unwrap();
    let description = runtime.interface_description(&interface).unwrap();
    let selected_method = runtime
        .resolve_interface_method_slot(&interface, description.interface_type(), 0, &[])
        .unwrap();
    let method = selected_method.admit(&mut runtime).unwrap();
    drop(selected_method);
    let record = method.invocation.computation().unwrap().record().clone();
    let ApplicationRecord::Admitted(computation) = record.as_ref() else {
        panic!("method computation")
    };
    let computation = computation.clone();
    let selected_target = computation.selected.target();
    let route = computation
        .selected
        .owner()
        .bind_call_route(selected_target)
        .unwrap();
    let old_execution = runtime.memory.enter_execution(&runtime.resources).unwrap();
    let admitted = runtime
        .application_in(
            old_execution,
            method.invocation.computation().unwrap().id(),
            &method.sources().unwrap(),
        )
        .unwrap();
    drop((interface, method));
    let candidate = runtime
        .stage_reload_verified_program(&installed, None, old.verified_program().clone())
        .unwrap();
    runtime.publish_staged_reload(candidate).unwrap();
    let current = runtime.select_program(&installed).unwrap();
    assert_ne!(current.program_identity(), old.program_identity());
    let fresh_selection = runtime.select_call_route(&route).unwrap();
    assert_eq!(
        fresh_selection.owner().program_identity(),
        current.program_identity()
    );
    let destination = runtime.memory.enter_execution(&runtime.resources).unwrap();
    let before = runtime.gc_stats();
    assert_eq!(
        runtime
            .alloc_application(
                destination,
                ApplicationRecord::Admitted(computation.clone()),
                &[MetadataSource::Revision {
                    owner: &old,
                    source: RevisionSource::Current
                }]
            )
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::ModuleValidation
    );
    assert_eq!(runtime.gc_stats(), before);
    // The actual admitted node supplies the old revision after publication.
    let copied = runtime
        .alloc_application(
            destination,
            ApplicationRecord::Admitted(computation),
            &[MetadataSource::AdmittedApplication(&admitted)],
        )
        .unwrap();
    runtime
        .memory
        .retire_execution(&runtime.resources, old_execution)
        .unwrap();
    runtime.collect_garbage().unwrap();
    assert_eq!(
        runtime.application_record(&admitted).unwrap_err().kind(),
        RuntimeErrorKind::ModuleValidation
    );
    let record = runtime.application_record(&copied).unwrap();
    let ApplicationRecord::Admitted(computation) = record.as_ref() else {
        panic!("admitted computation")
    };
    assert_eq!(
        computation.selected.owner().program_identity(),
        old.program_identity()
    );
    assert_eq!(computation.selected.target(), selected_target);
    assert!(runtime.modules.loaded(old.key()).is_some());
    assert_eq!(runtime.gc_stats().root_registrations, roots);
    runtime
        .memory
        .retire_execution(&runtime.resources, destination)
        .unwrap();
    runtime.collect_garbage().unwrap();
    assert!(runtime.modules.loaded(old.key()).is_none());
    assert!(runtime.modules.loaded(current.key()).is_some());
    assert_eq!(runtime.gc_stats().root_registrations, roots);
    assert!(!runtime.is_quarantined());
}
