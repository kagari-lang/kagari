use super::compile_program;
use crate::Runtime;
#[cfg(test)]
use crate::test_support::bytecode::load_bytecode;
use crate::value::Value;
use kagari_types::{scalar::BuiltinType, ty::Ty};

#[test]
fn candidate_heap_mutations_cannot_modify_preexisting_arrays() {
    let program = compile_program(
        include_str!("../../../tests/fixtures/native/candidate_array_access.kgr"),
        None,
    );
    let mut runtime = Runtime::default();
    crate::native::module::NativeModule::install_all(
        &kagari_stdlib::modules().unwrap(),
        &mut runtime,
    )
    .unwrap();
    let baseline_program = load_bytecode(&mut runtime, "main", program.clone()).unwrap();
    let baseline = runtime.select_program(&baseline_program).unwrap();
    let vm = runtime;
    let retained = vm
        .execute_module(&baseline, "values", vm.execution_options())
        .unwrap()
        .return_value;
    let Value::Array(array) = retained.value(vm.gc()).unwrap() else {
        panic!("array")
    };
    let runtime = &vm;
    let candidate = runtime
        .stage_reload_program(&baseline_program, Some("main".into()), program)
        .unwrap();
    let session = runtime.begin_candidate_initialization(&candidate).unwrap();
    let before = runtime.resources().counters();
    let heap = runtime.gc();
    assert!(heap.array_set(array, 0, Value::I32(9)).is_err());
    assert!(heap.array_fill(array, Value::I32(9)).is_err());
    assert!(
        heap.array_copy_within(
            array,
            std::ops::Bound::Included(0),
            std::ops::Bound::Excluded(1),
            0
        )
        .is_err()
    );
    assert!(heap.array_snapshot(array).is_none());
    assert!(heap.array_len(array).is_none());
    assert!(heap.array_get(array, 0).is_none());
    assert_eq!(runtime.resources().counters(), before);
    let local = runtime
        .alloc_array(
            candidate.module(),
            Ty::Builtin(BuiltinType::I32),
            vec![Value::I32(1)],
        )
        .unwrap();
    let local_root = runtime.root_value(Value::Array(local)).unwrap();
    heap.array_set(local, 0, Value::I32(2)).unwrap();
    assert_eq!(heap.array_get(local, 0), Some(Value::I32(2)));
    runtime.collect_garbage().unwrap();
    assert!(
        runtime
            .gc()
            .validate_value(&retained.value(runtime.gc()).unwrap())
    );
    drop(session);
    assert_eq!(heap.array_snapshot(array).unwrap(), vec![Value::I32(7)]);
    drop(candidate);
    runtime.gc().array_set(array, 0, Value::I32(9)).unwrap();
    assert_eq!(runtime.gc().array_get(array, 0), Some(Value::I32(9)));
    assert!(!runtime.is_quarantined());
    drop(local_root);
}
