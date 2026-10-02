use kagari_abi::{scalar::BuiltinType, types::AbiType};
use kagari_bytecode::{
    module::BytecodeModule,
    program::{BytecodeProgram, ModuleRef},
};
use kagari_runtime::module::LoadedModule;
use kagari_runtime::{Runtime, value::Value};

fn runtime_with_owner() -> (Runtime, LoadedModule) {
    let mut runtime = Runtime::default();
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
    let (runtime, owner) = runtime_with_owner();
    let first = runtime
        .alloc_array(
            &owner,
            AbiType::Builtin(BuiltinType::I32),
            vec![Value::I32(1)],
        )
        .unwrap();
    let before = runtime.resources().counters();
    let heap_before = runtime.gc().stats();
    assert!(
        runtime
            .alloc_array(
                &owner,
                AbiType::Builtin(BuiltinType::I32),
                vec![Value::Bool(true)]
            )
            .is_err()
    );
    assert_eq!(runtime.resources().counters(), before);
    assert_eq!(runtime.gc().stats(), heap_before);
    assert_eq!(runtime.gc().array_get(first, 0), Some(Value::I32(1)));
    let (runtime, owner) = runtime_with_owner();
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
                vec![Value::Bool(true)]
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
