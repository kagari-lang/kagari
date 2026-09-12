use kagari_runtime::{Runtime, RuntimeErrorKind, value::Value, value_semantics::script_equal};

#[test]
fn foreign_handles_and_wrong_value_tags_are_rejected_before_mutation_or_accounting() {
    let first = Runtime::default();
    let second = Runtime::default();
    let own = first.alloc_array(vec![Value::I32(1)]).unwrap();
    let foreign = second.alloc_array(vec![Value::I32(2)]).unwrap();
    assert_eq!(own.index(), foreign.index());
    assert_ne!(own, foreign);
    let before = first.gc().stats();
    assert!(first.gc().array_get(foreign, 0).is_none());
    assert!(first.gc().array_push(foreign, Value::I32(3)).is_err());
    assert!(first.gc().array_push(own, Value::Array(foreign)).is_err());
    assert!(
        first
            .gc()
            .array_set(own, 0, Value::Tuple(vec![Value::Array(foreign)]))
            .is_none()
    );
    assert!(
        first
            .gc()
            .alloc_enum("E".into(), "V".into(), vec![Value::Array(foreign)])
            .is_err()
    );
    assert!(first.root_value(Value::Array(foreign)).is_none());
    assert!(first.root_value(Value::Map(own)).is_none());
    let allocation_units = first.resources().counters().allocation_units;
    assert_eq!(
        first
            .alloc_array(vec![Value::Array(foreign)])
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::ScriptTrap
    );
    assert_eq!(
        first.resources().counters().allocation_units,
        allocation_units
    );
    assert_eq!(first.gc().stats(), before);
    assert_eq!(first.gc().array_snapshot(own), Some(vec![Value::I32(1)]));
    assert!(script_equal(first.gc(), &Value::Array(foreign), &Value::Array(foreign)).is_err());
}

#[test]
fn rooted_clones_keep_values_alive_and_reused_slots_reject_stale_handles() {
    let runtime = Runtime::default();
    let object = runtime.alloc_array(vec![Value::I32(42)]).unwrap();
    let naked_copy = Value::Array(object);
    let root = runtime.root_value(naked_copy.clone()).unwrap();
    let retained = root.clone();
    drop(root);
    assert_eq!(runtime.collect_garbage().unwrap().reclaimed_objects, 0);
    assert_eq!(runtime.gc().array_get(object, 0), Some(Value::I32(42)));
    assert_eq!(runtime.gc().active_roots(), 1);
    drop(retained);
    let collected = runtime.collect_garbage().unwrap();
    assert_eq!(
        (collected.reclaimed_objects, collected.reclaimed_units),
        (1, 2)
    );
    assert!(runtime.gc().array_len(object).is_none());
    assert!(runtime.root_value(naked_copy).is_none());
    let next = runtime.alloc_array(vec![]).unwrap();
    assert_eq!(next.index(), object.index());
    assert!(next.generation() > object.generation());
    let before = runtime.gc().stats();
    assert!(runtime.gc().array_push(object, Value::I32(7)).is_err());
    assert_eq!(runtime.gc().stats(), before);
    assert!(script_equal(runtime.gc(), &Value::Array(object), &Value::Array(object)).is_err());
}

#[test]
fn mark_sweep_traces_tuples_enum_payloads_and_cycles_without_retaining_unreachable_graphs() {
    let runtime = Runtime::default();
    let array = runtime.alloc_array(vec![]).unwrap();
    let map = runtime
        .alloc_map(vec![(Value::I32(1), Value::Array(array))])
        .unwrap();
    runtime.gc().array_push(array, Value::Map(map)).unwrap();
    let variant = runtime
        .alloc_enum(
            "E".into(),
            "V".into(),
            vec![Value::Tuple(vec![Value::Array(array)])],
        )
        .unwrap();
    let root = runtime
        .root_value(Value::Tuple(vec![Value::Enum(variant)]))
        .unwrap();
    runtime.alloc_set(vec![Value::I32(1)]).unwrap();
    let collection = runtime.collect_garbage().unwrap();
    assert_eq!(
        (collection.reclaimed_objects, collection.live_objects),
        (1, 3)
    );
    assert_eq!(runtime.gc().stats().current_heap_units, 6);
    root.set(runtime.gc(), Value::Unit).unwrap();
    let collection = runtime.collect_garbage().unwrap();
    assert_eq!(
        (collection.reclaimed_objects, collection.live_objects),
        (3, 0)
    );
    assert_eq!(runtime.gc().stats().current_heap_units, 0);
}

#[test]
fn roots_reject_foreign_replacement_and_execution_root_sets_release_on_drop() {
    let runtime = Runtime::default();
    let foreign = Runtime::default();
    let array = runtime.alloc_array(vec![]).unwrap();
    let root = runtime.root_value(Value::Array(array)).unwrap();
    assert!(root.set(foreign.gc(), Value::Unit).is_none());
    let other = foreign.alloc_array(vec![]).unwrap();
    assert!(root.set(runtime.gc(), Value::Array(other)).is_none());
    let slots = runtime
        .gc()
        .root_execution_values(vec![root.value()])
        .unwrap();
    drop(root);
    assert_eq!(runtime.collect_garbage().unwrap().live_objects, 1);
    assert!(slots.set(foreign.gc(), 0, Value::Unit).is_none());
    slots.set(runtime.gc(), 0, Value::Unit).unwrap();
    assert_eq!(runtime.collect_garbage().unwrap().reclaimed_objects, 1);
    drop(slots);
    assert_eq!(runtime.gc().active_roots(), 0);
}

#[test]
fn host_callbacks_can_retain_explicit_roots_without_requiring_cross_thread_storage() {
    use kagari_runtime::{
        CapabilitySet, HostExposurePolicy, LanguageProfile, RuntimeConfig, SecurityContext,
        host::{HostFunction, HostFunctionDeclaration, HostValueType},
    };
    let mut runtime = Runtime::new(RuntimeConfig {
        security: SecurityContext {
            profile: LanguageProfile {
                allow_host_calls: true,
                ..Default::default()
            },
            capabilities: CapabilitySet {
                host_calls: true,
                ..Default::default()
            },
        },
        host_exposure: HostExposurePolicy {
            allow_host_functions: true,
            ..Default::default()
        },
        ..Default::default()
    });
    let object = runtime.alloc_array(vec![Value::I32(7)]).unwrap();
    let retained = runtime.root_value(Value::Array(object)).unwrap();
    runtime
        .register_host_function(HostFunction::new(
            HostFunctionDeclaration::new("host.retained", vec![], HostValueType::Bool),
            move |_, _| Ok(Value::Bool(retained.value() == Value::Array(object))),
        ))
        .unwrap();
    assert_eq!(runtime.collect_garbage().unwrap().live_objects, 1);
    assert_eq!(
        runtime.invoke_host("host.retained", &[]).unwrap(),
        Value::Bool(true)
    );
    assert_eq!(runtime.gc().array_get(object, 0), Some(Value::I32(7)));
}

#[test]
fn tracing_a_deep_heap_chain_uses_an_explicit_work_stack() {
    let runtime = Runtime::default();
    let mut value = Value::Unit;
    for _ in 0..10_000 {
        value = Value::Array(runtime.alloc_array(vec![value]).unwrap());
    }
    let root = runtime.root_value(value).unwrap();
    assert_eq!(runtime.collect_garbage().unwrap().live_objects, 10_000);
    drop(root);
    assert_eq!(runtime.collect_garbage().unwrap().reclaimed_objects, 10_000);
}
