use crate::test_support::native::program::ProgramFixture;
use crate::{Runtime, module::ExecutableModule, value::Value};
use kagari_bytecode::{module::BytecodeModule, program::ModuleRef};
use kagari_common::identity::map::DefinitionContext;
use kagari_types::{scalar::BuiltinType, ty::Ty};

fn runtime_with_owner() -> (Runtime, ExecutableModule) {
    let context = DefinitionContext::new().unwrap();
    let mut runtime = Runtime::default();
    let owner_program = runtime
        .load_program(
            "allocations",
            ProgramFixture {
                exports: Default::default(),
                root: ModuleRef::new(0),
                modules: vec![BytecodeModule::default()],
            }
            .metadata(context.snapshot(), &Default::default())
            .unwrap(),
        )
        .unwrap();
    let owner = runtime.select_program(&owner_program).unwrap();
    (runtime, owner)
}

#[test]
fn rejected_allocations_leave_all_counters_and_free_slots_unchanged() {
    let (runtime, owner) = runtime_with_owner();
    let first = runtime
        .alloc_array(&owner, Ty::Builtin(BuiltinType::I32), vec![Value::I32(1)])
        .unwrap();
    let before = runtime.resources().counters();
    let heap_before = runtime.gc().stats();
    assert!(
        runtime
            .alloc_array(
                &owner,
                Ty::Builtin(BuiltinType::I32),
                vec![Value::Bool(true)]
            )
            .is_err()
    );
    assert_eq!(runtime.resources().counters(), before);
    assert_eq!(runtime.gc().stats(), heap_before);
    assert_eq!(runtime.gc().array_get(first, 0), Some(Value::I32(1)));
    let (runtime, owner) = runtime_with_owner();
    runtime
        .alloc_array(&owner, Ty::Builtin(BuiltinType::I32), vec![])
        .unwrap();
    assert_eq!(runtime.collect_garbage().unwrap().reclaimed_objects, 1);
    let before = runtime.gc().stats();
    assert!(
        runtime
            .alloc_array(
                &owner,
                Ty::Builtin(BuiltinType::I32),
                vec![Value::Bool(true)]
            )
            .is_err()
    );
    assert_eq!(runtime.gc().stats(), before);
    let next = runtime
        .alloc_array(&owner, Ty::Builtin(BuiltinType::I32), vec![])
        .unwrap();
    assert_eq!(runtime.gc().array_len(next), Some(0));
    assert_eq!(runtime.gc().stats().allocated_objects, 1);
}
