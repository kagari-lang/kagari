use kagari_abi::{scalar::BuiltinType, types::AbiType};
use kagari_bytecode::{
    module::BytecodeModule,
    program::{BytecodeProgram, ModuleRef},
};
use kagari_runtime::module::LoadedModule;
use kagari_runtime::{
    Runtime, RuntimeConfig, error::RuntimeErrorKind, resource::ResourcePolicy, value::Value,
};

fn limited(heap: Option<usize>, allocation: Option<usize>) -> (Runtime, LoadedModule) {
    let mut runtime = Runtime::new(RuntimeConfig {
        resources: ResourcePolicy {
            max_heap_units: heap,
            max_allocation_units: allocation,
            ..Default::default()
        },
        ..Default::default()
    });
    let owner = runtime
        .load_program(
            "allocations",
            BytecodeProgram {
                root: ModuleRef::new(0),
                modules: vec![BytecodeModule::default()],
            },
        )
        .unwrap();
    (runtime, owner)
}

#[test]
fn rejected_allocations_leave_all_counters_and_free_slots_unchanged() {
    for (heap, allocation, reason) in [
        (Some(2), None, "heap units"),
        (None, Some(2), "allocation units"),
    ] {
        let (runtime, owner) = limited(heap, allocation);
        let first = runtime
            .alloc_array(
                &owner,
                AbiType::Builtin(BuiltinType::I32),
                vec![Value::I32(1)],
            )
            .unwrap();
        let before = runtime.resources().counters();
        let heap_before = runtime.gc().stats();
        let error = runtime
            .alloc_array(&owner, AbiType::Builtin(BuiltinType::I32), vec![])
            .unwrap_err();
        assert_eq!(error.kind(), RuntimeErrorKind::ResourceLimitExceeded);
        assert!(error.message().contains(reason));
        assert_eq!(runtime.resources().counters(), before);
        assert_eq!(runtime.gc().stats(), heap_before);
        assert_eq!(runtime.gc().array_get(first, 0), Some(Value::I32(1)));
    }
    let (runtime, owner) = limited(Some(2), None);
    let old = runtime
        .alloc_array(&owner, AbiType::Builtin(BuiltinType::I32), vec![])
        .unwrap();
    runtime.collect_garbage().unwrap();
    let before = runtime.gc().stats();
    assert!(
        runtime
            .alloc_array(
                &owner,
                AbiType::Builtin(BuiltinType::I32),
                vec![Value::I32(1), Value::I32(2)]
            )
            .is_err()
    );
    assert_eq!(runtime.gc().stats(), before);
    let next = runtime
        .alloc_array(&owner, AbiType::Builtin(BuiltinType::I32), vec![])
        .unwrap();
    assert_eq!(next.index(), old.index());
    assert_ne!(next, old);
}
