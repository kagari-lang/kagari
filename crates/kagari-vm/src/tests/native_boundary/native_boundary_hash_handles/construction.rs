use super::{CUSTOM, compile_program, fixture};
use kagari_runtime::native::{
    binding::NativeResult,
    builder::ModuleBuilder,
    collections::{map::ScriptMap, set::ScriptSet},
    conversion::KagariType,
    conversion::context::ConversionContext,
    declarations::FunctionDecl,
    objects::Object,
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
    let _owner_program = owner;
    let owner = vm.runtime().select_program(&_owner_program).unwrap();
    let runtime = vm.runtime();
    let conversion = ConversionContext::new(runtime, &owner).unwrap();
    let key = conversion.type_for::<String>().unwrap();
    let value = conversion.type_for::<i32>().unwrap();
    let factory = runtime
        .bind_map_constructor::<String, i32>(&_owner_program.root(), &key, &value)
        .unwrap();
    let again = runtime
        .bind_map_constructor::<String, i32>(&_owner_program.root(), &key, &value)
        .unwrap();
    let set_factory = runtime
        .bind_set_constructor::<i32>(&_owner_program.root(), &value)
        .unwrap();
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
            .bind_map_constructor::<bool, i32>(&_owner_program.root(), &key, &value)
            .is_err()
    );
    let missing = conversion.type_for::<i64>().unwrap();
    assert!(
        runtime
            .bind_set_constructor::<i64>(&_owner_program.root(), &missing)
            .is_err()
    );
    drop((map, other, set, convenience, convenience_set));
    assert_eq!(runtime.collect_garbage().unwrap().live_objects, 0);
    assert_eq!(runtime.gc().active_roots(), 0);
}

#[test]
fn custom_key_factories_use_current_bodies_and_helpers_through_reentry_and_reload() {
    let mut module = ModuleBuilder::new(
        "example::keys",
        &StandardDeclarations::default().catalog().unwrap(),
    )
    .unwrap();
    {
        let registered_function = module.define_function(FunctionDecl::new("probe")).unwrap();
        module
            .bind(
                registered_function.clone(),
                |cx: &mut NativeContext<'_>, (): ()| -> NativeResult<()> {
                    cx.collect_garbage()?;
                    Ok(())
                },
            )
            .map(|()| registered_function)
    }
    .unwrap();
    let module = module.finish().unwrap();
    let source = format!(
        "{}\n{}",
        CUSTOM
            .replace(
                "probe();\n            if",
                "probe();\n            EQ_BODY = 1; EQ_HELPER = eq_marker();\n            if",
            )
            .replace(
                "probe(); 7",
                "probe(); HASH_BODY = 1; HASH_HELPER = hash_marker(); 7",
            ),
        r#"
        static var HASH_BODY: i32 = 0;
        static var HASH_HELPER: i32 = 0;
        static var EQ_BODY: i32 = 0;
        static var EQ_HELPER: i32 = 0;
        fn hash_marker() -> i32 { 10 }
        fn eq_marker() -> i32 { 10 }
        pub fn reset() { HASH_BODY = 0; HASH_HELPER = 0; EQ_BODY = 0; EQ_HELPER = 0; }
        pub fn markers() -> (i32, i32, i32, i32) { (HASH_BODY, HASH_HELPER, EQ_BODY, EQ_HELPER) }
        "#,
    );
    let (vm, old) = fixture(&source, Some(&module));
    let _old_program = old;
    let old = vm.runtime().select_program(&_old_program).unwrap();
    let key_type = vm
        .runtime()
        .bind_type(&_old_program.root(), "Key", &[])
        .unwrap();
    let value_type = ConversionContext::new(vm.runtime(), &old)
        .unwrap()
        .type_for::<i32>()
        .unwrap();
    let factory = vm
        .runtime()
        .bind_map_constructor::<Object, i32>(
            &_old_program.root(),
            key_type.type_argument(),
            &value_type,
        )
        .unwrap();
    let set_factory = vm
        .runtime()
        .bind_set_constructor::<Object>(&_old_program.root(), key_type.type_argument())
        .unwrap();
    let key = vm
        .runtime()
        .bind_function::<(i32,), Object>(&_old_program.root(), "key")
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
    assert_eq!(
        vm.execute_typed::<(), (i32, i32, i32, i32)>(&old, "markers", ())
            .unwrap(),
        (1, 10, 1, 10)
    );
    drop(cx);
    let bodies = source
        .replace("HASH_BODY = 1;", "HASH_BODY = 2;")
        .replace("EQ_BODY = 1;", "EQ_BODY = 2;");
    let helpers = bodies.replace("-> i32 { 10 }", "-> i32 { 20 }");
    for (updated, expected) in [(&bodies, (2, 10, 2, 10)), (&helpers, (2, 20, 2, 20))] {
        vm.reload_program(
            &_old_program,
            Some("functions".into()),
            compile_program(updated, Some(&module)),
        )
        .unwrap();
        let current = vm.runtime().select_program(&_old_program).unwrap();
        let mut cx = vm.context(&current).unwrap();
        vm.execute_typed::<(), ()>(&current, "reset", ()).unwrap();
        assert_eq!(map.get(&mut cx, equal.clone()).unwrap(), Some(42));
        assert_eq!(
            vm.execute_typed::<(), (i32, i32, i32, i32)>(&current, "markers", ())
                .unwrap(),
            expected
        );
        vm.execute_typed::<(), ()>(&current, "reset", ()).unwrap();
        assert!(set.contains(&cx, equal.clone()).unwrap());
        assert_eq!(
            vm.execute_typed::<(), (i32, i32, i32, i32)>(&current, "markers", ())
                .unwrap(),
            expected
        );
        let trap = vm.call(&key, (99,)).unwrap();
        assert!(map.get(&mut cx, trap.clone()).is_err());
        assert!(set.contains(&cx, trap).is_err());
        assert_eq!(map.len(&cx).unwrap(), 1);
        assert_eq!(set.len(&cx).unwrap(), 1);
        assert_eq!(vm.runtime().resources().counters().current_call_depth, 0);
    }
    let _new_program = _old_program.clone();
    let new = vm.runtime().select_program(&_new_program).unwrap();
    let mut cx = vm.context(&new).unwrap();
    let later = factory.call(&mut cx, ()).unwrap();
    later.insert(&cx, first.clone(), 43).unwrap();
    assert_eq!(later.get(&mut cx, equal.clone()).unwrap(), Some(43));
    assert_eq!(map.get(&mut cx, equal.clone()).unwrap(), Some(42));
    let later_set = set_factory.call(&mut cx, ()).unwrap();
    assert!(later_set.insert(&cx, first.clone()).unwrap());
    assert!(later_set.contains(&cx, equal.clone()).unwrap());
    let new_key_type = vm
        .runtime()
        .bind_type(&_new_program.root(), "Key", &[])
        .unwrap();
    let new_factory = vm
        .runtime()
        .bind_map_constructor::<Object, i32>(
            &_new_program.root(),
            new_key_type.type_argument(),
            &value_type,
        )
        .unwrap();
    let replacement = new_factory.call(&mut cx, ()).unwrap();
    replacement.insert(&cx, first, 44).unwrap();
    assert_eq!(replacement.get(&mut cx, equal).unwrap(), Some(44));
    let (foreign, foreign_owner) = fixture("pub fn main() {}", None);
    let _foreign_owner_program = foreign_owner;
    let foreign_owner = foreign
        .runtime()
        .select_program(&_foreign_owner_program)
        .unwrap();
    let mut foreign_cx = foreign.context(&foreign_owner).unwrap();
    assert!(factory.call(&mut foreign_cx, ()).is_err());
    assert!(map.len(&foreign_cx).is_err());
    assert!(set.len(&foreign_cx).is_err());
    drop((
        map,
        set,
        later,
        later_set,
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
    )
    .unwrap();
    {
        let registered_function = module
            .define_function(
                FunctionDecl::new("fresh")
                    .returns(<Collections>::kagari_type(module.declarations()).unwrap()),
            )
            .unwrap();
        module
            .bind(
                registered_function.clone(),
                |cx: &mut NativeContext<'_>, (): ()| -> NativeResult<Collections> {
                    let map = cx.create_map()?;
                    let set = cx.create_set()?;
                    map.insert(cx, 1, 42)?;
                    set.insert(cx, 1)?;
                    cx.collect_garbage()?;
                    Ok((map, set))
                },
            )
            .map(|()| registered_function)
    }
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
    let _owner_program = owner;
    let owner = vm.runtime().select_program(&_owner_program).unwrap();
    assert_eq!(vm.execute_typed::<(), i32>(&owner, "run", ()).unwrap(), 42);
    assert_eq!(vm.runtime().collect_garbage().unwrap().live_objects, 0);
    assert_eq!(vm.runtime().gc().active_roots(), 0);
}
