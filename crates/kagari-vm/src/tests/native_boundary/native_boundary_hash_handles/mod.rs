use kagari_stdlib::collections::{
    map::{MapStorage, ScriptMap},
    set::ScriptSet,
    vector::ScriptVec,
};
mod construction;
use crate::tests::native_boundary::{
    compile_program, native_boundary_functions::fixture, publish_unused_version,
};
use crate::vm::Vm;
use kagari_runtime::{
    Runtime,
    error::{RuntimeError, RuntimeErrorKind},
    frame::types::arguments::TypeArgument,
    native::{
        binding::NativeResult,
        builder::ModuleBuilder,
        catalog::DeclarationCatalog,
        conversion::context::ConversionContext,
        conversion::{FromKagari, KagariType},
        declarations::FunctionDecl,
        module::NativeModule,
        objects::Object,
        payload::Opaque,
        typed::NativeContext,
        types::Type,
        value_handle::ScriptValue,
    },
    value::Value,
};

#[derive(Debug)]
struct RejectRemovedString;

impl KagariType for RejectRemovedString {
    fn kagari_type(catalog: &DeclarationCatalog) -> NativeResult<Type> {
        String::kagari_type(catalog)
    }
}

impl FromKagari for RejectRemovedString {
    fn from_kagari(
        cx: &mut ConversionContext<'_>,
        _: &TypeArgument,
        value: &ScriptValue,
    ) -> NativeResult<Self> {
        cx.runtime().collect_garbage()?;
        value.inspect(cx.runtime(), |_| Ok(()))?;
        Err(RuntimeError::module_validation("rejected removed value"))
    }
}

#[test]
fn host_removal_commits_before_rejecting_result_conversion() {
    let (vm, program) = fixture(
        r#"use std::collections::HashMap;
        pub fn make(value: String) -> HashMap<i32, String> {
            val map = HashMap::new(); map.insert(1, value); map
        }"#,
        None,
    );
    let owner = vm.runtime().select_program(&program).unwrap();
    let make = vm
        .runtime()
        .bind_function::<(String,), ScriptMap<i32, RejectRemovedString>>(&program.root(), "make")
        .unwrap();
    let map = vm.call(&make, ("retained result".to_owned(),)).unwrap();
    let mut cx = vm.context(&owner).unwrap();
    let error = map.remove(&mut cx, 1).unwrap_err();
    assert!(error.message().contains("rejected removed value"));
    assert!(map.is_empty(&cx).unwrap());
    assert!(map.remove(&mut cx, 1).unwrap().is_none());
    drop((map, make, cx));
    publish_unused_version(&vm, &program);
    assert_eq!(vm.runtime().collect_garbage().unwrap().live_objects, 0);
    assert_eq!(vm.runtime().gc().active_roots(), 0);
}
use kagari_stdlib::declarations::StandardDeclarations;
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
            val map = HashMap::new(); map.insert("a", Vec::from([20, 22]));
            val set = HashSet::new(); set.insert(7);
            (map, set)
        }
        pub fn size(map: HashMap<String, Vec<i32>>) -> u64 { map.len() }
        pub fn get(map: HashMap<String, Vec<i32>>, key: String) -> Option<Vec<i32>> {
            map.get(key)
        }
    "#,
        None,
    );
    let _owner_program = owner;
    let owner = vm.runtime().select_program(&_owner_program).unwrap();
    type Containers = (ScriptMap<String, ScriptVec<i32>>, ScriptSet<i32>);
    let make = vm
        .runtime()
        .bind_function::<(), Containers>(&_owner_program.root(), "make")
        .unwrap();
    let (map, set) = vm.call(&make, ()).unwrap();
    let alias = map.clone();
    let mut cx = vm.context(&owner).unwrap();
    let get = vm
        .runtime()
        .bind_function::<(ScriptMap<String, ScriptVec<i32>>, String), Option<ScriptVec<i32>>>(
            &_owner_program.root(),
            "get",
        )
        .unwrap();
    let script_values = vm.call(&get, (map.clone(), "a".into())).unwrap().unwrap();
    assert!(
        vm.call(&get, (map.clone(), "absent".into()))
            .unwrap()
            .is_none()
    );
    let values = map.get(&mut cx, "a".into()).unwrap().unwrap();
    assert_eq!(values.get(&mut cx, 1).unwrap(), Some(22));
    assert!(map.contains_key(&mut cx, "a".into()).unwrap());
    map.insert(&mut cx, "b".into(), values.clone()).unwrap();
    assert_eq!(alias.len(&cx).unwrap(), 2);
    assert!(!set.insert(&mut cx, 7).unwrap());
    assert!(set.insert(&mut cx, 8).unwrap());
    assert!(set.contains(&mut cx, 8).unwrap());
    assert!(set.remove(&mut cx, 7).unwrap());
    assert!(!set.remove(&mut cx, 7).unwrap());
    assert!(map.read_only().clear(&mut cx).is_err());
    assert!(set.read_only().insert(&mut cx, 9).is_err());
    let size = vm
        .runtime()
        .bind_function::<(ScriptMap<String, ScriptVec<i32>>,), usize>(
            &_owner_program.root(),
            "size",
        )
        .unwrap();
    assert!(vm.call(&size, (map.read_only(),)).is_err());
    assert_eq!(vm.call(&size, (map.clone(),)).unwrap(), 2);
    let removed = map.remove(&mut cx, "a".into()).unwrap().unwrap();
    map.clear(&mut cx).unwrap();
    cx.collect_garbage().unwrap();
    assert_eq!(removed.get(&mut cx, 0).unwrap(), Some(20));
    assert_eq!(script_values.get(&mut cx, 1).unwrap(), Some(22));
    let (foreign, other) = fixture("pub fn main() {}", None);
    let _other_program = other;
    let other = foreign.runtime().select_program(&_other_program).unwrap();
    assert!(set.len(&foreign.context(&other).unwrap()).is_err());
    drop((
        removed,
        values,
        script_values,
        map,
        alias,
        set,
        make,
        size,
        get,
        cx,
    ));
    publish_unused_version(&vm, &_owner_program);
    assert_eq!(vm.runtime().collect_garbage().unwrap().live_objects, 0);
}

#[test]
fn hash_owned_snapshots_root_replaced_values_and_detect_structural_edits() {
    let (vm, owner) = fixture(
        r#"
        use std::collections::{HashMap, HashSet};
        pub fn make() -> (HashMap<i32, Vec<i32>>, HashSet<i32>) {
            val map = HashMap::new(); map.insert(1, Vec::from([20])); map.insert(2, Vec::from([22]));
            val set = HashSet::new(); set.insert(1); set.insert(2);
            (map, set)
        }
    "#,
        None,
    );
    let _owner_program = owner;
    let owner = vm.runtime().select_program(&_owner_program).unwrap();
    type Containers = (ScriptMap<i32, ScriptVec<i32>>, ScriptSet<i32>);
    let make = vm
        .runtime()
        .bind_function::<(), Containers>(&_owner_program.root(), "make")
        .unwrap();
    let (map, set) = vm.call(&make, ()).unwrap();
    let mut cx = vm.context(&owner).unwrap();
    let replacement = ScriptVec::create(&mut cx, vec![0]).unwrap();
    let mut sum = 0;
    for (_, value) in map.to_vec(&mut cx).unwrap() {
        map.insert(&mut cx, 1, replacement.clone()).unwrap();
        map.insert(&mut cx, 2, replacement.clone()).unwrap();
        cx.collect_garbage().unwrap();
        sum += value.get(&mut cx, 0).unwrap().unwrap();
    }
    assert_eq!(sum, 42);
    assert!(set.for_each(&mut cx, |cx, _| set.clear(cx)).is_err());
    set.clear(&mut cx).unwrap();
    map.clear(&mut cx).unwrap();
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
    )
    .unwrap();
    let alias = held.clone();
    let observed = visits.clone();
    {
        let registered_function = module.define_function(FunctionDecl::new("probe")).unwrap();
        module
            .bind(
                registered_function.clone(),
                move |cx: &mut NativeContext<'_>, (): ()| -> NativeResult<()> {
                    cx.collect_garbage()?;
                    if let Some(map) = alias.lock().unwrap().as_ref() {
                        assert!(map.clear(cx).is_err());
                    }
                    observed.fetch_add(1, Ordering::Relaxed);
                    Ok(())
                },
            )
            .map(|()| registered_function)
    }
    .unwrap();
    let (vm, owner) = fixture(CUSTOM, Some(&module.finish().unwrap()));
    let _owner_program = owner;
    let owner = vm.runtime().select_program(&_owner_program).unwrap();
    type Containers = (ScriptMap<Object, i32>, ScriptSet<Object>);
    let make = vm
        .runtime()
        .bind_function::<(), Containers>(&_owner_program.root(), "make")
        .unwrap();
    let key = vm
        .runtime()
        .bind_function::<(i32,), Object>(&_owner_program.root(), "key")
        .unwrap();
    let (map, set) = vm.call(&make, ()).unwrap();
    let first = vm.call(&key, (1,)).unwrap();
    let equal_first = vm.call(&key, (1,)).unwrap();
    let second = vm.call(&key, (2,)).unwrap();
    let bad = vm.call(&key, (99,)).unwrap();
    let mut cx = vm.context(&owner).unwrap();
    *held.lock().unwrap() = Some(map.clone());
    map.insert(&mut cx, first.clone(), 20).unwrap();
    map.insert(&mut cx, second.clone(), 22).unwrap();
    assert_eq!(map.get(&mut cx, equal_first.clone()).unwrap(), Some(20));
    let before = visits.load(Ordering::Relaxed);
    assert!(map.read_only().insert(&mut cx, first.clone(), 0).is_err());
    assert_eq!(visits.load(Ordering::Relaxed), before);
    let error = map.get(&mut cx, bad).unwrap_err();
    assert_eq!(error.kind(), RuntimeErrorKind::ScriptTrap);
    assert_eq!(map.get(&mut cx, second.clone()).unwrap(), Some(22));
    assert_eq!(map.remove(&mut cx, first.clone()).unwrap(), Some(20));
    assert_eq!(map.remove(&mut cx, equal_first.clone()).unwrap(), None);
    assert_eq!(map.len(&cx).unwrap(), 1);
    held.lock().unwrap().take();
    assert!(set.insert(&mut cx, first.clone()).unwrap());
    assert!(!set.insert(&mut cx, equal_first.clone()).unwrap());
    assert!(set.insert(&mut cx, second.clone()).unwrap());
    assert!(set.contains(&mut cx, equal_first.clone()).unwrap());
    assert!(set.remove(&mut cx, equal_first.clone()).unwrap());
    assert!(!set.contains(&mut cx, first.clone()).unwrap());
    set.clear(&mut cx).unwrap();
    map.clear(&mut cx).unwrap();
    drop((cx, first, second, equal_first, map, set, make, key));
    assert_eq!(vm.runtime().collect_garbage().unwrap().live_objects, 0);
}

#[test]
fn stored_key_protocol_selects_current_code_and_keeps_metadata_until_last_value_root() {
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
        static var observed: i32 = 0;
        pub fn marker() -> i32 {{ observed }}
        impl<T: Eq + Hash> Hash for Key<T> {{ fn hash(self) -> i64 {{ observed = {version}; self.value.hash() }} }}
        pub fn make() -> (HashMap<Key<i32>, i32>, Key<i32>) {{
            val map: HashMap<Key<i32>, i32> = HashMap::new();
            val key = Key {{ value: 1 }};
            map.insert(key, 42);
            (map, key)
        }}
    "#
        )
    };
    let code = |version| compile_program(&source(version), None);
    let mut runtime = Runtime::default();
    NativeModule::install_all(&kagari_stdlib::modules().unwrap(), &mut runtime).unwrap();
    let old = runtime.load_program("hash-cycle", code(1)).unwrap();
    let _old_program = old;
    let old = runtime.select_program(&_old_program).unwrap();
    let vm = Vm::new(runtime);
    let result = vm.execute(&old, "make").unwrap().return_value;
    let value = unsafe { result.value_transport(vm.runtime().gc()) }.unwrap();
    let Value::Tuple(_) = &value else {
        panic!("tuple");
    };
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
        /* SAFETY: The execution result root retains this tuple and its children throughout conversion. */
        unsafe { conversion.decode_transport(&result_type, &value) }.unwrap()
    };
    let stored = {
        let mut cx = vm.context(&old).unwrap();
        let conversion = cx.conversion();
        let map_type = conversion.parameter(&result_type, 0).unwrap();
        let value = conversion.encode_value(&map_type, map.clone()).unwrap();
        let object: Opaque<MapStorage> = conversion.decode_value(&map_type, &value).unwrap();
        object.stored_function::<(Object,), i64>(&cx, 0).unwrap()
    };
    let prepared = stored.prepare_call(vm.runtime()).unwrap();
    vm.reload_program(&_old_program, Some("hash-cycle".into()), code(100))
        .unwrap();
    let _new_program = _old_program.clone();
    let new = vm.runtime().select_program(&_new_program).unwrap();
    let collected = vm.runtime().collect_garbage().unwrap();
    assert!(!collected.reclaimed_modules.contains(&old.key()));
    // Owning adapters and selected relationship evidence survive reload.
    // Ordinary callbacks select current bodies; explicit preparation pins its body.
    drop(result);
    {
        let mut cx = vm.context(&new).unwrap();
        let _session = vm
            .runtime()
            .begin_execution(&new, vm.runtime().execution_options())
            .unwrap();
        prepared
            .call_with_options::<_, i64>(&mut cx, (key.clone(),), Default::default())
            .unwrap();
        assert_eq!(vm.execute_typed::<(), i32>(&old, "marker", ()).unwrap(), 1);
        stored.call(&mut cx, (key.clone(),)).unwrap();
        assert_eq!(
            vm.execute_typed::<(), i32>(&new, "marker", ()).unwrap(),
            100
        );
        assert_eq!(map.get(&mut cx, key.clone()).unwrap(), Some(42));
        assert_eq!(
            vm.execute_typed::<(), i32>(&new, "marker", ()).unwrap(),
            100
        );
    }
    let changed_hash = source(200).replace("self.value.hash()", "self.value.hash() + 1i64");
    vm.reload_program(
        &_old_program,
        Some("hash-cycle".into()),
        compile_program(&changed_hash, None),
    )
    .unwrap();
    let new = vm.runtime().select_program(&_old_program).unwrap();
    {
        let mut cx = vm.context(&new).unwrap();
        // The changed hash is permitted, but lookup semantics are the caller's
        // responsibility. Observe execution without asserting presence or absence.
        let _ = map.get(&mut cx, key.clone()).unwrap();
        assert_eq!(
            vm.execute_typed::<(), i32>(&new, "marker", ()).unwrap(),
            200
        );
        assert_eq!(map.len(&cx).unwrap(), 1);
    }
    drop((map, key, stored));
    assert!(
        !vm.runtime()
            .collect_garbage()
            .unwrap()
            .reclaimed_modules
            .contains(&old.key())
    );
    drop(prepared);
    let collected = vm.runtime().collect_garbage().unwrap();
    assert!(collected.reclaimed_modules.contains(&old.key()));
    assert_eq!(collected.live_objects, 0);
    assert!(!unsafe { vm.runtime().gc().validate_value_transport(&value) });
    assert!(vm.runtime().modules().loaded(new.key()).is_some());
}

#[test]
fn typed_native_hash_arguments_share_script_storage_and_return_handles() {
    type Containers = (ScriptMap<i32, i32>, ScriptSet<i32>);
    let mut module = ModuleBuilder::new(
        "example::maps",
        &StandardDeclarations::default().catalog().unwrap(),
    )
    .unwrap();
    {
        let registered_function = module
            .define_function(
                FunctionDecl::new("edit")
                    .parameter(
                        "map",
                        <ScriptMap<i32, i32>>::kagari_type(module.declarations()).unwrap(),
                    )
                    .parameter(
                        "set",
                        <ScriptSet<i32>>::kagari_type(module.declarations()).unwrap(),
                    )
                    .returns(<Containers>::kagari_type(module.declarations()).unwrap()),
            )
            .unwrap();
        module
            .bind(
                registered_function.clone(),
                |cx: &mut NativeContext<'_>, (map, set): Containers| -> NativeResult<Containers> {
                    map.insert(cx, 1, 20)?;
                    map.insert(cx, 2, 22)?;
                    set.insert(cx, 42)?;
                    Ok((map, set))
                },
            )
            .map(|()| registered_function)
    }
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
    let _owner_program = owner;
    let run = vm
        .runtime()
        .bind_function::<(), i32>(&_owner_program.root(), "run")
        .unwrap();
    assert_eq!(vm.call(&run, ()).unwrap(), 42);
    assert_eq!(vm.runtime().collect_garbage().unwrap().live_objects, 0);
}

#[test]
fn host_hash_traversals_retain_only_current_entries_and_observe_replacements() {
    let (vm, program) = fixture(
        r#"use std::collections::{HashMap, HashSet};
        pub fn make()->(HashMap<i32,i32>,HashSet<i32>) {
            val map=HashMap::new(); val set=HashSet::new();
            var i=0; while i<64 { map.insert(i,i); set.insert(i); i+=1; }
            (map,set)
        }"#,
        None,
    );
    let owner = vm.runtime().select_program(&program).unwrap();
    let make = vm
        .runtime()
        .bind_function::<(), (ScriptMap<i32, i32>, ScriptSet<i32>)>(&program.root(), "make")
        .unwrap();
    let (map, set) = vm.call(&make, ()).unwrap();
    let mut cx = vm.context(&owner).unwrap();
    let roots = vm.runtime().gc().active_roots();
    let mut visited = 0;
    map.for_each(&mut cx, |cx, key, value| {
        assert!(cx.runtime().gc().active_roots() <= roots + 2);
        cx.collect_garbage()?;
        if visited == 0 {
            assert_eq!(key, value);
            for key in 0..64 {
                map.insert(cx, key, 99)?;
            }
        } else {
            assert_eq!(value, 99, "unvisited replacements are read on demand");
        }
        visited += 1;
        Ok(())
    })
    .unwrap();
    assert_eq!(visited, 64);
    assert_eq!(vm.runtime().gc().active_roots(), roots);
    let mut visited = 0;
    let error = set
        .for_each(&mut cx, |cx, _| {
            assert!(cx.runtime().gc().active_roots() <= roots + 1);
            cx.collect_garbage()?;
            visited += 1;
            Err(RuntimeError::module_validation("stop after current member"))
        })
        .unwrap_err();
    assert!(error.message().contains("stop after current member"));
    assert_eq!(visited, 1);
    assert_eq!(vm.runtime().gc().active_roots(), roots);
    drop((map, set, make, cx));
    publish_unused_version(&vm, &program);
    assert_eq!(vm.runtime().collect_garbage().unwrap().live_objects, 0);
    assert_eq!(vm.runtime().gc().active_roots(), 0);
}

#[test]
fn host_hash_traversals_detect_committed_edits_including_the_final_callback() {
    let (vm, owner) = fixture(
        r#"use std::collections::{HashMap, HashSet};
        pub fn make()->(HashMap<i32,i32>,HashSet<i32>) {
            val map=HashMap::new();map.insert(1,10);
            val set=HashSet::new();set.insert(1);(map,set)
        }"#,
        None,
    );
    let _owner_program = owner;
    let owner = vm.runtime().select_program(&_owner_program).unwrap();
    type Containers = (ScriptMap<i32, i32>, ScriptSet<i32>);
    let make = vm
        .runtime()
        .bind_function::<(), Containers>(&_owner_program.root(), "make")
        .unwrap();
    let (map, set) = vm.call(&make, ()).unwrap();
    let mut cx = vm.context(&owner).unwrap();
    let error = map
        .for_each(&mut cx, |cx, key, value| {
            assert_eq!((key, value), (1, 10));
            map.insert(cx, 2, 20)?;
            cx.collect_garbage()?;
            Ok(())
        })
        .unwrap_err();
    assert!(
        error
            .message()
            .contains("native structure changed during operation")
    );
    assert_eq!(map.get(&mut cx, 2).unwrap(), Some(20));
    let error = set
        .for_each(&mut cx, |cx, value| {
            assert_eq!(value, 1);
            set.insert(cx, 2)?;
            cx.collect_garbage()?;
            Ok(())
        })
        .unwrap_err();
    assert!(
        error
            .message()
            .contains("native structure changed during operation")
    );
    assert!(set.contains(&mut cx, 2).unwrap());
}
