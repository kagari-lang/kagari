use super::compile_program;
#[cfg(test)]
use crate::tests::support::bytecode::load_bytecode;
use kagari_runtime::{Runtime, value::Value};

use kagari_types::{scalar::BuiltinType, ty::Ty};

#[test]
fn candidate_heap_mutations_cannot_modify_preexisting_arrays() {
    let program = compile_program(
        include_str!("../../../../tests/fixtures/native/candidate_array_access.kgr"),
        None,
    );
    let mut runtime = Runtime::default();
    kagari_runtime::native::module::NativeModule::install_all(
        &crate::modules().unwrap(),
        &mut runtime,
    )
    .unwrap();
    let baseline_program = load_bytecode(&mut runtime, "main", program.clone()).unwrap();
    let baseline = runtime.select_program(&baseline_program).unwrap();
    let mut vm = runtime;
    let retained = vm
        .execute_module(&baseline, "values", vm.execution_options())
        .unwrap()
        .return_value;
    let Value::Array(array) = retained.value(vm.gc()).unwrap() else {
        panic!("array")
    };
    let runtime = &mut vm;
    let candidate = runtime
        .stage_reload_program(&baseline_program, Some("main".into()), program)
        .unwrap();
    let mut session = runtime.begin_candidate_initialization(&candidate).unwrap();
    let local_root = {
        let runtime = session.runtime_mut();
        let before = runtime.resources().counters();
        assert!(runtime.gc().array_set(array, 0, Value::I32(9)).is_err());
        assert!(runtime.gc().array_fill(array, Value::I32(9)).is_err());
        assert!(
            runtime
                .gc()
                .array_copy_within(
                    array,
                    std::ops::Bound::Included(0),
                    std::ops::Bound::Excluded(1),
                    0
                )
                .is_err()
        );
        assert!(runtime.gc().array_snapshot(array).is_none());
        assert!(runtime.gc().array_len(array).is_none());
        assert!(runtime.gc().array_get(array, 0).is_none());
        assert_eq!(runtime.resources().counters(), before);
        let local = runtime
            .alloc_array(
                candidate.module(),
                Ty::Builtin(BuiltinType::I32),
                vec![Value::I32(1)],
            )
            .unwrap();
        let local_root = runtime.root_value(Value::Array(local)).unwrap();
        runtime.gc().array_set(local, 0, Value::I32(2)).unwrap();
        assert_eq!(runtime.gc().array_get(local, 0), Some(Value::I32(2)));
        runtime.collect_garbage().unwrap();
        assert!(
            runtime
                .gc()
                .validate_value(&retained.value(runtime.gc()).unwrap())
        );
        local_root
    };
    drop(session);
    assert_eq!(
        runtime.gc().array_snapshot(array).unwrap(),
        vec![Value::I32(7)]
    );
    drop(candidate);
    runtime.gc().array_set(array, 0, Value::I32(9)).unwrap();
    assert_eq!(runtime.gc().array_get(array, 0), Some(Value::I32(9)));
    assert!(!runtime.is_quarantined());
    drop(local_root);
}
