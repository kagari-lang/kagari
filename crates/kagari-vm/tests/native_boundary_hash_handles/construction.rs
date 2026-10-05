use super::{CUSTOM, compile_program, fixture};
use kagari_runtime::native::{
    binding::NativeResult,
    builder::ModuleBuilder,
    collections::{map::ScriptMap, set::ScriptSet},
    conversion::context::ConversionContext,
    objects::Object,
    registration::FunctionSpec,
    typed::NativeContext,
};
use kagari_stdlib::declarations::StandardDeclarations;

#[test]
fn factories_reuse_checked_constructors_and_return_independent_retained_collections() {
    let (vm, owner) = fixture(
        r#"
        use std::collections::{HashMap, HashSet};
        pub fn evidence() -> (HashMap<String, i32>, HashSet<i32>) {
            (HashMap::new(), HashSet::new())
        }
    "#,
        None,
    );
    let runtime = vm.runtime();
    let conversion = ConversionContext::new(runtime, &owner).unwrap();
    let key = conversion.type_for::<String>().unwrap();
    let value = conversion.type_for::<i32>().unwrap();
    let factory = runtime
        .bind_map_constructor::<String, i32>(&owner, &key, &value)
        .unwrap();
    let again = runtime
        .bind_map_constructor::<String, i32>(&owner, &key, &value)
        .unwrap();
    let set_factory = runtime.bind_set_constructor::<i32>(&owner, &value).unwrap();
    let mut cx = vm.context(&owner).unwrap();
    let map = factory.call(&mut cx, ()).unwrap();
    let other = again.call(&mut cx, ()).unwrap();
    let set = set_factory.call(&mut cx, ()).unwrap();
    map.insert(&cx, "answer".into(), 42).unwrap();
    assert!(other.is_empty(&cx).unwrap());
    assert!(set.insert(&cx, 7).unwrap());
    runtime.collect_garbage().unwrap();
    assert_eq!(map.get(&mut cx, "answer".into()).unwrap(), Some(42));
    assert!(set.contains(&cx, 7).unwrap());
    let convenience: ScriptMap<String, i32> = cx.create_map().unwrap();
    let convenience_set: ScriptSet<i32> = cx.create_set().unwrap();
    assert!(convenience.is_empty(&cx).unwrap());
    assert!(convenience_set.is_empty(&cx).unwrap());
    assert!(
        runtime
            .bind_map_constructor::<bool, i32>(&owner, &key, &value)
            .is_err()
    );
    let missing = conversion.type_for::<i64>().unwrap();
    assert!(
        runtime
            .bind_set_constructor::<i64>(&owner, &missing)
            .is_err()
    );
    drop((map, other, set, convenience, convenience_set));
    assert_eq!(runtime.collect_garbage().unwrap().live_objects, 0);
    assert_eq!(runtime.gc().active_roots(), 0);
}

#[test]
fn custom_key_factories_pin_selected_operations_through_reentry_and_reload() {
    let mut module = ModuleBuilder::new(
        "example::keys",
        &StandardDeclarations::default().catalog().unwrap(),
    );
    module
        .add_function(
            FunctionSpec::new("probe"),
            |cx: &mut NativeContext<'_>, (): ()| -> NativeResult<()> {
                cx.collect_garbage()?;
                Ok(())
            },
        )
        .unwrap();
    let module = module.finish().unwrap();
    let (vm, old) = fixture(CUSTOM, Some(&module));
    let key_type = vm.runtime().bind_type(&old, "Key", &[]).unwrap();
    let value_type = ConversionContext::new(vm.runtime(), &old)
        .unwrap()
        .type_for::<i32>()
        .unwrap();
    let factory = vm
        .runtime()
        .bind_map_constructor::<Object, i32>(&old, key_type.type_argument(), &value_type)
        .unwrap();
    let set_factory = vm
        .runtime()
        .bind_set_constructor::<Object>(&old, key_type.type_argument())
        .unwrap();
    let key = vm
        .runtime()
        .bind_function::<(i32,), Object>(&old, "key")
        .unwrap();
    let first = vm.call(&key, (1,)).unwrap();
    let equal = vm.call(&key, (1,)).unwrap();
    let mut cx = vm.context(&old).unwrap();
    let map = factory.call(&mut cx, ()).unwrap();
    let set = set_factory.call(&mut cx, ()).unwrap();
    let explicit: ScriptMap<Object, i32> = cx
        .create_map_with_types(key_type.type_argument(), &value_type)
        .unwrap();
    let explicit_set: ScriptSet<Object> =
        cx.create_set_with_type(key_type.type_argument()).unwrap();
    explicit.insert(&cx, first.clone(), 40).unwrap();
    assert!(explicit_set.insert(&cx, first.clone()).unwrap());
    assert_eq!(explicit.get(&mut cx, equal.clone()).unwrap(), Some(40));
    assert!(explicit_set.contains(&cx, equal.clone()).unwrap());
    drop((explicit, explicit_set));
    map.insert(&cx, first.clone(), 42).unwrap();
    assert!(set.insert(&cx, first.clone()).unwrap());
    assert_eq!(map.get(&mut cx, equal.clone()).unwrap(), Some(42));
    assert!(!set.insert(&cx, equal.clone()).unwrap());
    let trap = vm.call(&key, (99,)).unwrap();
    assert!(map.get(&mut cx, trap).is_err());
    assert_eq!(map.len(&cx).unwrap(), 1);
    let new_source = CUSTOM.replace("self.value == other.value", "false");
    let new = vm
        .reload_program(
            &old,
            "functions",
            compile_program(&new_source, Some(&module)),
        )
        .unwrap();
    drop(cx);
    let mut cx = vm.context(&new).unwrap();
    let later = factory.call(&mut cx, ()).unwrap();
    later.insert(&cx, first.clone(), 43).unwrap();
    assert_eq!(later.get(&mut cx, equal.clone()).unwrap(), Some(43));
    assert_eq!(map.get(&mut cx, equal.clone()).unwrap(), Some(42));
    let new_key_type = vm.runtime().bind_type(&new, "Key", &[]).unwrap();
    let new_factory = vm
        .runtime()
        .bind_map_constructor::<Object, i32>(&new, new_key_type.type_argument(), &value_type)
        .unwrap();
    let replacement = new_factory.call(&mut cx, ()).unwrap();
    replacement.insert(&cx, first, 44).unwrap();
    assert_eq!(replacement.get(&mut cx, equal).unwrap(), None);
    let (foreign, foreign_owner) = fixture("pub fn main() {}", None);
    assert!(
        factory
            .call(&mut foreign.context(&foreign_owner).unwrap(), ())
            .is_err()
    );
    drop((
        map,
        set,
        later,
        replacement,
        factory,
        set_factory,
        key,
        key_type,
        cx,
    ));
    let collected = vm.runtime().collect_garbage().unwrap();
    assert!(collected.reclaimed_modules.contains(&old.key()));
    assert_eq!(collected.live_objects, 0);
    assert_eq!(vm.runtime().resources().counters().current_call_depth, 0);
}

#[test]
fn typed_native_callbacks_create_collections_with_the_same_checked_factory() {
    type Collections = (ScriptMap<i32, i32>, ScriptSet<i32>);
    let mut module = ModuleBuilder::new(
        "example::factory",
        &StandardDeclarations::default().catalog().unwrap(),
    );
    module
        .add_function(
            FunctionSpec::new("fresh"),
            |cx: &mut NativeContext<'_>, (): ()| -> NativeResult<Collections> {
                let map = cx.create_map()?;
                let set = cx.create_set()?;
                map.insert(cx, 1, 42)?;
                set.insert(cx, 1)?;
                cx.collect_garbage()?;
                Ok((map, set))
            },
        )
        .unwrap();
    let (vm, owner) = fixture(
        r#"
        use std::collections::{HashMap, HashSet};
        use example::factory::fresh;
        pub fn evidence() -> (HashMap<i32, i32>, HashSet<i32>) { (HashMap::new(), HashSet::new()) }
        pub fn run() -> i32 {
            match fresh() { (map, set) => if set.contains(1) { match map.get(1) { Some(value) => value, None => -2 } } else { -1 } }
        }
    "#,
        Some(&module.finish().unwrap()),
    );
    assert_eq!(vm.execute_typed::<(), i32>(&owner, "run", ()).unwrap(), 42);
    assert_eq!(vm.runtime().collect_garbage().unwrap().live_objects, 0);
    assert_eq!(vm.runtime().gc().active_roots(), 0);
}
