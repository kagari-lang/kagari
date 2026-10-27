use super::compile_program;
use crate::vm::Vm;
use kagari_runtime::{Runtime, native::typed::NativeContext};
use kagari_stdlib::collections::{map::ScriptMap, set::ScriptSet, vector::ScriptVec};

#[test]
fn candidate_heap_mutations_cannot_modify_preexisting_containers() {
    let program = compile_program(
        "use std::collections::{HashMap, HashSet};\npub fn containers() -> (Vec<i32>, HashMap<i32,i32>, HashSet<i32>) { val map: HashMap<i32,i32> = HashMap::new(); map.insert(1,7); val set: HashSet<i32> = HashSet::new(); set.insert(7); (Vec::from([7]), map, set) }",
        None,
    );
    let mut runtime = Runtime::default();
    kagari_runtime::native::module::NativeModule::install_all(
        &kagari_stdlib::modules().unwrap(),
        &mut runtime,
    )
    .unwrap();
    let baseline = runtime.load_program("main", program.clone()).unwrap();
    let _baseline_program = baseline;
    let baseline = runtime.select_program(&_baseline_program).unwrap();
    let mut vm = Vm::new(runtime);
    let (array, map, set): (ScriptVec<i32>, ScriptMap<i32, i32>, ScriptSet<i32>) =
        vm.execute_typed(&baseline, "containers", ()).unwrap();
    let runtime = vm.runtime_mut();
    let candidate = runtime
        .stage_reload_program(&_baseline_program, Some("main".into()), program)
        .unwrap();
    let session = runtime.begin_candidate_initialization(&candidate).unwrap();
    let before = runtime.resources().counters();
    let mut cx = NativeContext::new(runtime, candidate.module()).unwrap();
    assert!(array.push(&mut cx, 9).is_err());
    assert!(array.insert(&mut cx, 0, 9).is_err());
    assert!(array.set(&mut cx, 0, 9).is_err());
    assert!(array.pop(&mut cx).is_err());
    assert!(array.remove(&mut cx, 0).is_err());
    assert!(array.clear(&mut cx).is_err());
    assert!(map.insert(&mut cx, 1, 9).is_err());
    assert!(map.remove(&mut cx, 1).is_err());
    assert!(map.clear(&mut cx).is_err());
    assert!(set.insert(&mut cx, 9).is_err());
    assert!(set.remove(&mut cx, 7).is_err());
    assert!(set.clear(&mut cx).is_err());
    assert!(array.len(&cx).is_err());
    assert!(array.get(&mut cx, 0).is_err());
    assert!(map.to_vec(&mut cx).is_err());
    assert!(map.len(&cx).is_err());
    assert!(set.to_vec(&mut cx).is_err());
    assert!(set.len(&cx).is_err());
    assert_eq!(runtime.resources().counters(), before);
    let local = ScriptVec::create(&mut cx, vec![1i32]).unwrap();
    local.push(&mut cx, 2).unwrap();
    assert_eq!(local.len(&cx).unwrap(), 2);
    runtime.collect_garbage().unwrap();
    drop(cx);
    drop(session);
    let mut cx = NativeContext::new(runtime, &baseline).unwrap();
    assert_eq!(array.len(&cx).unwrap(), 1);
    assert_eq!(array.get(&mut cx, 0).unwrap(), Some(7));
    assert_eq!(map.to_vec(&mut cx).unwrap(), vec![(1, 7)]);
    assert_eq!(set.to_vec(&mut cx).unwrap(), vec![7]);
    drop(candidate);
    array.push(&mut cx, 9).unwrap();
    assert_eq!(array.len(&cx).unwrap(), 2);
    assert!(!runtime.is_quarantined());
}
