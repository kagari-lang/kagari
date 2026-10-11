use super::compile_program;
use kagari_runtime::{Runtime, value::Value};
use kagari_vm::vm::Vm;

#[test]
fn candidate_heap_mutations_cannot_modify_preexisting_containers() {
    let program = compile_program(
        "use std::collections::{HashMap, HashSet};\nfn containers() -> (Vec<i32>, HashMap<i32,i32>, HashSet<i32>) { val map: HashMap<i32,i32> = HashMap::new(); map.insert(1,7); val set: HashSet<i32> = HashSet::new(); set.insert(7); (Vec::from([7]), map, set) }",
        None,
    );
    let mut runtime = Runtime::default();
    kagari_runtime::native::module::NativeModule::install_all(
        &kagari_stdlib::modules().unwrap(),
        &mut runtime,
    )
    .unwrap();
    let baseline = runtime.load_program("main", program.clone()).unwrap();
    let mut vm = Vm::new(runtime);
    let Value::Tuple(values) = vm
        .execute(&baseline, "containers")
        .unwrap()
        .return_value
        .value(vm.runtime().gc())
        .expect("retained execution result")
    else {
        panic!("container tuple")
    };
    let values = vm.runtime().gc().tuple(values).unwrap().to_vec();
    let [Value::GcHandle(array), Value::Map(map), Value::Set(set)] = values.as_slice() else {
        panic!("container handles")
    };
    let (array, map, set) = (*array, *map, *set);
    let runtime = vm.runtime_mut();
    let candidate = runtime
        .stage_reload_program(&baseline, "main", program)
        .unwrap();
    let retained = runtime
        .root_value(
            runtime
                .gc()
                .alloc_tuple(vec![
                    Value::GcHandle(array),
                    Value::Map(map),
                    Value::Set(set),
                ])
                .unwrap(),
        )
        .unwrap();
    let session = runtime.begin_candidate_initialization(&candidate).unwrap();
    let before = runtime.resources().counters();
    let heap = runtime.gc();
    assert!(heap.sequence_push(array, Value::I32(9)).is_err());
    assert!(heap.sequence_insert(array, 0, Value::I32(9)).is_err());
    assert!(heap.sequence_set(array, 0, Value::I32(9)).is_err());
    assert!(heap.sequence_pop(array).is_err());
    assert!(heap.sequence_remove(array, 0).is_err());
    assert!(heap.sequence_clear(array).is_err());
    assert!(heap.map_insert(map, Value::I32(1), Value::I32(9)).is_err());
    assert!(heap.map_remove(map, &Value::I32(1)).is_err());
    assert!(heap.map_clear(map).is_err());
    assert!(heap.set_insert(set, Value::I32(9)).is_err());
    assert!(heap.set_remove(set, &Value::I32(7)).is_err());
    assert!(heap.set_clear(set).is_err());
    assert!(heap.sequence_snapshot(array).is_none());
    assert!(heap.sequence_len(array).is_none());
    assert!(heap.sequence_get(array, 0).is_none());
    assert!(heap.map_snapshot(map).is_none());
    assert!(heap.map_len(map).is_none());
    assert!(heap.set_snapshot(set).is_none());
    assert!(heap.set_len(set).is_none());
    assert_eq!(runtime.resources().counters(), before);
    let local_root = kagari_runtime::native::conversion::context::ConversionContext::new(
        runtime,
        candidate.module(),
    )
    .unwrap()
    .encode(vec![1i32])
    .unwrap();
    let Value::GcHandle(local) = local_root.value(heap).unwrap() else {
        panic!("Vec")
    };
    heap.sequence_push(local, Value::I32(2)).unwrap();
    assert_eq!(heap.sequence_len(local), Some(2));
    runtime.collect_garbage().unwrap();
    assert!(
        runtime
            .gc()
            .validate_value(&retained.value(runtime.gc()).unwrap())
    );
    drop(session);
    assert_eq!(heap.sequence_snapshot(array).unwrap(), vec![Value::I32(7)]);
    assert_eq!(
        heap.map_snapshot(map).unwrap(),
        vec![(Value::I32(1), Value::I32(7))]
    );
    assert_eq!(heap.set_snapshot(set).unwrap(), vec![Value::I32(7)]);
    drop(candidate);
    runtime.gc().sequence_push(array, Value::I32(9)).unwrap();
    assert_eq!(runtime.gc().sequence_len(array), Some(2));
    assert!(!runtime.is_quarantined());
}
