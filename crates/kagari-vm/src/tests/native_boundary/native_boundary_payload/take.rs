use super::{Counter, SOURCE, module};
use crate::tests::native_boundary::native_boundary_functions::fixture;
use kagari_runtime::{
    native::{
        collections::vector::ScriptVec,
        conversion::{FromKagari, IntoKagari, context::ConversionContext},
        payload::Opaque,
        value_handle::ScriptValue,
    },
    value::Value,
};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

#[test]
fn take_moves_nonclone_payload_and_invalidates_host_and_script_aliases_across_slot_reuse() {
    let drops = Arc::new(AtomicUsize::new(0));
    let (module, declaration, _, _) = module(drops.clone());
    let source = SOURCE.to_owned()
        + r#"
        pub fn aliases(counter: Counter) -> Vec<Counter> { Vec::from([counter, counter]) }
    "#;
    let (vm, owner) = fixture(&source, Some(&module));
    let _owner_program = owner;
    let owner = vm.runtime().select_program(&_owner_program).unwrap();
    let runtime = vm.runtime();
    let ty = runtime
        .bind_native_type::<Counter>(&_owner_program.root(), &declaration, &[])
        .unwrap();
    let object: Opaque<Counter> = vm.execute_typed(&owner, "make", (42,)).unwrap();
    let alias = object.clone();
    let script: ScriptVec<Opaque<Counter>> = vm
        .execute_typed(&owner, "aliases", (alias.clone(),))
        .unwrap();
    let mut cx = vm.context(&owner).unwrap();
    let mut conversion = ConversionContext::new(runtime, &owner).unwrap();
    let old = alias
        .clone()
        .into_kagari(&mut conversion, ty.type_argument())
        .unwrap();
    let parked = ScriptValue::from_kagari(&mut conversion, ty.type_argument(), &old).unwrap();
    let Value::GcHandle(old_id) = old else {
        panic!("opaque")
    };
    unsafe {
        object.read(&cx, None, |_, _access| {
            assert!(alias.take(&cx).is_err());
            Ok(())
        })
    }
    .unwrap();
    // Retire the temporary source-array before testing immediate native slot reuse.
    assert_eq!(runtime.collect_garbage().unwrap().live_objects, 2);
    let before = runtime.gc().stats().current_heap_units;
    let original = object.take(&cx).unwrap();
    assert_eq!(original.value, 42);
    assert_eq!(drops.load(Ordering::Relaxed), 0);
    assert_eq!(runtime.gc().stats().current_heap_units, before - 1);
    assert!(!runtime.gc().validate_value(&old));
    assert!(runtime.root_value(old).is_none());
    assert!(unsafe { alias.read(&cx, None, |_, _access| Ok(())) }.is_err());
    assert!(object.take(&cx).is_err());
    assert!(script.get(&mut cx, 0).is_err());
    assert!(parked.decode::<Opaque<Counter>>(&mut cx).is_err());
    assert!(
        vm.execute_typed::<_, i32>(&owner, "value", (alias.clone(),))
            .is_err()
    );
    assert_eq!(runtime.collect_garbage().unwrap().live_objects, 1);
    let fresh = ty
        .create(
            &mut cx,
            Counter {
                value: 13,
                dropped: drops.clone(),
            },
            &[],
        )
        .unwrap();
    let fresh_value = fresh
        .clone()
        .into_kagari(&mut conversion, ty.type_argument())
        .unwrap();
    let Value::GcHandle(fresh_id) = fresh_value else {
        panic!("fresh opaque")
    };
    assert_eq!(fresh_id.index(), old_id.index());
    assert_ne!(fresh_id, old_id);
    assert_eq!(
        unsafe { fresh.read(&cx, None, |value, _access| Ok(value.value)) }.unwrap(),
        13
    );
    assert!(unsafe { alias.read(&cx, None, |_, _access| Ok(())) }.is_err());
    assert_eq!(runtime.collect_garbage().unwrap().live_objects, 2);
    drop(fresh);
    runtime.collect_garbage().unwrap();
    assert_eq!(drops.load(Ordering::Relaxed), 1);
    drop(original);
    assert_eq!(drops.load(Ordering::Relaxed), 2);
    drop((object, alias, script, parked, cx, conversion));
    assert_eq!(runtime.collect_garbage().unwrap().live_objects, 0);
    assert_eq!(runtime.gc().stats().current_heap_units, 0);
}

#[test]
fn failed_take_preserves_payload_when_registration_does_not_grant_extraction() {
    let drops = Arc::new(AtomicUsize::new(0));
    let (module, _, other, _) = module(drops.clone());
    let (vm, owner) = fixture(SOURCE, Some(&module));
    let _owner_program = owner;
    let owner = vm.runtime().select_program(&_owner_program).unwrap();
    let ty = vm
        .runtime()
        .bind_native_type::<Counter>(&_owner_program.root(), &other, &[])
        .unwrap();
    let mut cx = vm.context(&owner).unwrap();
    let object = ty
        .create(
            &mut cx,
            Counter {
                value: 7,
                dropped: drops.clone(),
            },
            &[],
        )
        .unwrap();
    let alias = object.clone();
    let before = vm.runtime().gc().stats().current_heap_units;
    assert!(
        object
            .take(&cx)
            .unwrap_err()
            .message()
            .contains("extraction")
    );
    assert_eq!(
        unsafe { alias.read(&cx, None, |value, _access| Ok(value.value)) }.unwrap(),
        7
    );
    assert_eq!(vm.runtime().gc().stats().current_heap_units, before);
    assert_eq!(drops.load(Ordering::Relaxed), 0);
    drop((object, alias));
    assert_eq!(vm.runtime().collect_garbage().unwrap().live_objects, 0);
    assert_eq!(drops.load(Ordering::Relaxed), 1);
}
