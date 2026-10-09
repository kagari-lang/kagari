mod construction;
use super::{compile_program, native_boundary_functions::fixture};
use kagari_abi::representation::ValueType;
use kagari_bytecode::{instruction::ModuleSlot, module::BytecodeModuleSlot};
use kagari_runtime::{
    Runtime,
    error::RuntimeErrorKind,
    native::{
        binding::NativeResult,
        builder::ModuleBuilder,
        collections::{map::ScriptMap, set::ScriptSet, vector::ScriptVec},
        conversion::context::ConversionContext,
        module::NativeModule,
        objects::Object,
        registration::FunctionSpec,
        typed::NativeContext,
    },
    value::Value,
};
use kagari_stdlib::declarations::StandardDeclarations;
use kagari_vm::vm::Vm;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};

#[test]
fn builtin_hash_handles_preserve_aliases_access_and_retained_values() {
    let (vm, owner) = fixture(
        r#"
        use std::collections::{HashMap, HashSet};
        pub fn make() -> (HashMap<String, Vec<i32>>, HashSet<i32>) {
            val map = HashMap::new(); map.insert("a", [20, 22]);
            val set = HashSet::new(); set.insert(7);
            (map, set)
        }
        pub fn size(map: HashMap<String, Vec<i32>>) -> usize { map.len() }
    "#,
        None,
    );
    type Containers = (ScriptMap<String, ScriptVec<i32>>, ScriptSet<i32>);
    let make = vm
        .runtime()
        .bind_function::<(), Containers>(&owner, "make")
        .unwrap();
    let (map, set) = vm.call(&make, ()).unwrap();
    let alias = map.clone();
    let mut cx = vm.context(&owner).unwrap();
    let values = map.get(&mut cx, "a".into()).unwrap().unwrap();
    assert_eq!(values.get(&mut cx, 1).unwrap(), Some(22));
    assert!(map.contains_key(&cx, "a".into()).unwrap());
    map.insert(&cx, "b".into(), values.clone()).unwrap();
    assert_eq!(alias.len(&cx).unwrap(), 2);
    assert!(!set.insert(&cx, 7).unwrap());
    assert!(set.insert(&cx, 8).unwrap());
    assert!(set.contains(&cx, 8).unwrap());
    assert!(set.remove(&cx, 7).unwrap());
    assert!(!set.remove(&cx, 7).unwrap());
    assert!(map.read_only().clear(&cx).is_err());
    assert!(set.read_only().insert(&cx, 9).is_err());
    let size = vm
        .runtime()
        .bind_function::<(ScriptMap<String, ScriptVec<i32>>,), usize>(&owner, "size")
        .unwrap();
    assert!(vm.call(&size, (map.read_only(),)).is_err());
    assert_eq!(vm.call(&size, (map.clone(),)).unwrap(), 2);
    let removed = map.remove(&mut cx, "a".into()).unwrap().unwrap();
    map.clear(&cx).unwrap();
    cx.collect_garbage().unwrap();
    assert_eq!(removed.get(&mut cx, 0).unwrap(), Some(20));
    let (foreign, other) = fixture("pub fn main() {}", None);
    assert!(set.len(&foreign.context(&other).unwrap()).is_err());
    drop((removed, values, map, alias, set, make, size, cx));
    assert_eq!(vm.runtime().collect_garbage().unwrap().live_objects, 0);
}

#[test]
fn hash_snapshots_root_replaced_values_and_release_iteration_guards() {
    let (vm, owner) = fixture(
        r#"
        use std::collections::{HashMap, HashSet};
        pub fn make() -> (HashMap<i32, Vec<i32>>, HashSet<i32>) {
            val map = HashMap::new(); map.insert(1, [20]); map.insert(2, [22]);
            val set = HashSet::new(); set.insert(1); set.insert(2);
            (map, set)
        }
    "#,
        None,
    );
    type Containers = (ScriptMap<i32, ScriptVec<i32>>, ScriptSet<i32>);
    let make = vm
        .runtime()
        .bind_function::<(), Containers>(&owner, "make")
        .unwrap();
    let (map, set) = vm.call(&make, ()).unwrap();
    let mut cx = vm.context(&owner).unwrap();
    let replacement = cx.create_vec(vec![0]).unwrap();
    let mut sum = 0;
    map.for_each(&mut cx, |cx, _, value| {
        assert!(map.clear(cx).is_err());
        assert!(map.insert(cx, 3, replacement.clone()).is_err());
        map.insert(cx, 1, replacement.clone())?;
        map.insert(cx, 2, replacement.clone())?;
        cx.collect_garbage()?;
        sum += value.get(cx, 0)?.unwrap();
        Ok(())
    })
    .unwrap();
    assert_eq!(sum, 42);
    assert!(set.for_each(&mut cx, |cx, _| set.clear(cx)).is_err());
    set.clear(&cx).unwrap();
    map.clear(&cx).unwrap();
    assert!(map.is_empty(&cx).unwrap());
    assert!(set.is_empty(&cx).unwrap());
}

const CUSTOM: &str = r#"
    use std::collections::{HashMap, HashSet};
    use std::hash::Hash;
    use example::keys::probe;
    pub struct Key { pub val value: i32 }
    impl PartialEq for Key {
        fn eq(self, other: Key) -> bool {
            probe();
            if other.value == 99 { self.value / (other.value - other.value) == 0 }
            else { self.value == other.value }
        }
    }
    impl Eq for Key {}
    impl Hash for Key { fn hash(self) -> i64 { probe(); 7 } }
    pub fn make() -> (HashMap<Key, i32>, HashSet<Key>) { (HashMap::new(), HashSet::new()) }
    pub fn key(value: i32) -> Key { Key { value: value } }
"#;

#[test]
fn custom_hash_handles_use_selected_calls_with_gc_reentry_and_trap_cleanup() {
    let held: Arc<Mutex<Option<ScriptMap<Object, i32>>>> = Arc::default();
    let visits = Arc::new(AtomicUsize::new(0));
    let mut module = ModuleBuilder::new(
        "example::keys",
        &StandardDeclarations::default().catalog().unwrap(),
    );
    let alias = held.clone();
    let observed = visits.clone();
    module
        .add_function(
            FunctionSpec::new("probe"),
            move |cx: &mut NativeContext<'_>, (): ()| -> NativeResult<()> {
                cx.collect_garbage()?;
                if let Some(map) = alias.lock().unwrap().as_ref() {
                    assert!(map.clear(cx).is_err());
                }
                observed.fetch_add(1, Ordering::Relaxed);
                Ok(())
            },
        )
        .unwrap();
    let (vm, owner) = fixture(CUSTOM, Some(&module.finish().unwrap()));
    type Containers = (ScriptMap<Object, i32>, ScriptSet<Object>);
    let make = vm
        .runtime()
        .bind_function::<(), Containers>(&owner, "make")
        .unwrap();
    let key = vm
        .runtime()
        .bind_function::<(i32,), Object>(&owner, "key")
        .unwrap();
    let (map, set) = vm.call(&make, ()).unwrap();
    let first = vm.call(&key, (1,)).unwrap();
    let equal_first = vm.call(&key, (1,)).unwrap();
    let second = vm.call(&key, (2,)).unwrap();
    let bad = vm.call(&key, (99,)).unwrap();
    let mut cx = vm.context(&owner).unwrap();
    *held.lock().unwrap() = Some(map.clone());
    map.insert(&cx, first.clone(), 20).unwrap();
    map.insert(&cx, second.clone(), 22).unwrap();
    assert_eq!(map.get(&mut cx, equal_first.clone()).unwrap(), Some(20));
    let before = visits.load(Ordering::Relaxed);
    assert!(map.read_only().insert(&cx, first.clone(), 0).is_err());
    assert_eq!(visits.load(Ordering::Relaxed), before);
    let error = map.get(&mut cx, bad).unwrap_err();
    assert_eq!(error.kind(), RuntimeErrorKind::ScriptTrap);
    assert_eq!(map.get(&mut cx, second.clone()).unwrap(), Some(22));
    assert_eq!(map.remove(&mut cx, first.clone()).unwrap(), Some(20));
    assert_eq!(map.remove(&mut cx, equal_first.clone()).unwrap(), None);
    assert_eq!(map.len(&cx).unwrap(), 1);
    held.lock().unwrap().take();
    assert!(set.insert(&cx, first.clone()).unwrap());
    assert!(!set.insert(&cx, equal_first.clone()).unwrap());
    assert!(set.insert(&cx, second.clone()).unwrap());
    assert!(set.contains(&cx, equal_first.clone()).unwrap());
    assert!(set.remove(&cx, equal_first.clone()).unwrap());
    assert!(!set.contains(&cx, first.clone()).unwrap());
    set.clear(&cx).unwrap();
    map.clear(&cx).unwrap();
    drop((cx, first, second, equal_first, map, set, make, key));
    assert_eq!(vm.runtime().collect_garbage().unwrap().live_objects, 0);
}

#[test]
fn stored_key_protocol_keeps_old_code_and_collects_its_module_cycle() {
    let source = |version| {
        format!(
            r#"
        use std::collections::HashMap;
        use std::hash::Hash;
        pub struct Key<T> {{ pub val value: T }}
        impl<T: Eq + Hash> PartialEq for Key<T> {{
            fn eq(self, other: Key<T>) -> bool {{ self.value == other.value }}
        }}
        impl<T: Eq + Hash> Eq for Key<T> {{}}
        impl<T: Eq + Hash> Hash for Key<T> {{ fn hash(self) -> i64 {{ self.value.hash() + {version} }} }}
        pub fn make() -> (HashMap<Key<i32>, i32>, Key<i32>) {{
            val map: HashMap<Key<i32>, i32> = HashMap::new();
            val key = Key {{ value: 1 }};
            map.insert(key, 42);
            (map, key)
        }}
    "#
        )
    };
    let code = |version| {
        let mut program = compile_program(&source(version), None);
        program.modules[program.root.index()]
            .module_slots
            .push(BytecodeModuleSlot {
                name: "saved".into(),
                ty: ValueType::HeapObject,
                mutable: true,
            });
        program
    };
    let mut runtime = Runtime::default();
    NativeModule::install_all(&kagari_stdlib::modules().unwrap(), &mut runtime).unwrap();
    let old = runtime.load_program("hash-cycle", code(1)).unwrap();
    let vm = Vm::new(runtime);
    let result = vm.execute(&old, "make").unwrap().return_value;
    let value = result.value(vm.runtime().gc()).unwrap();
    let Value::Tuple(items) = &value else {
        panic!("tuple");
    };
    vm.runtime()
        .write_module_slot(
            &old,
            ModuleSlot::new(0),
            vm.runtime().gc().tuple(*items).unwrap()[0],
        )
        .unwrap();
    let new = vm.reload_program(&old, "hash-cycle", code(100)).unwrap();
    let collected = vm.runtime().collect_garbage().unwrap();
    assert!(!collected.reclaimed_modules.contains(&old.key()));
    // Only a raw rooted value survived reload. The map's stored evidence must
    // retain the original executable selection for this applied key type.
    let result_type = old
        .bytecode
        .functions
        .iter()
        .find(|function| function.name == "make")
        .unwrap()
        .metadata
        .semantic
        .result
        .clone()
        .unwrap();
    let result_type = vm
        .runtime()
        .resolve_type_arguments(&old, &[result_type])
        .unwrap()
        .pop()
        .unwrap();
    let (map, key): (ScriptMap<Object, i32>, Object) = {
        let mut conversion = ConversionContext::new(vm.runtime(), &old).unwrap();
        conversion.decode_value(&result_type, &value).unwrap()
    };
    drop(result);
    {
        let mut cx = vm.context(&new).unwrap();
        let _session = vm
            .runtime()
            .begin_execution(&new, vm.runtime().execution_options())
            .unwrap();
        assert_eq!(map.get(&mut cx, key.clone()).unwrap(), Some(42));
    }
    drop((map, key));
    let collected = vm.runtime().collect_garbage().unwrap();
    assert!(collected.reclaimed_modules.contains(&old.key()));
    assert_eq!(collected.live_objects, 0);
    assert!(!vm.runtime().gc().validate_value(&value));
    assert!(vm.runtime().modules().loaded(new.key()).is_some());
}

#[test]
fn typed_native_hash_arguments_share_script_storage_and_return_handles() {
    type Containers = (ScriptMap<i32, i32>, ScriptSet<i32>);
    let mut module = ModuleBuilder::new(
        "example::maps",
        &StandardDeclarations::default().catalog().unwrap(),
    );
    module
        .add_function(
            FunctionSpec::new("edit").parameter_names(["map", "set"]),
            |cx: &mut NativeContext<'_>, (map, set): Containers| -> NativeResult<Containers> {
                map.insert(cx, 1, 20)?;
                map.insert(cx, 2, 22)?;
                set.insert(cx, 42)?;
                Ok((map, set))
            },
        )
        .unwrap();
    let (vm, owner) = fixture(
        r#"
        use std::collections::{HashMap, HashSet};
        use example::maps::edit;
        pub fn run() -> i32 {
            val map: HashMap<i32, i32> = HashMap::new();
            val set: HashSet<i32> = HashSet::new();
            val result = edit(map, set);
            if !set.contains(42) { return -1; }
            match map.get(1) { Some(a) => match map.get(2) { Some(b) => a + b, None => -2 }, None => -3 }
        }
    "#,
        Some(&module.finish().unwrap()),
    );
    let run = vm
        .runtime()
        .bind_function::<(), i32>(&owner, "run")
        .unwrap();
    assert_eq!(vm.call(&run, ()).unwrap(), 42);
    assert_eq!(vm.runtime().collect_garbage().unwrap().live_objects, 0);
}
