use crate::{
    Runtime,
    error::RuntimeError,
    gc::inspection::ValueView,
    session::ExecutionOptions,
    test_support::layouts::allocation_owner,
    value::{Value, ValueCategory},
};
use std::{
    cell::Cell,
    panic::{AssertUnwindSafe, catch_unwind},
};

#[test]
fn scalar_inspection_preserves_representation_and_float_bits() {
    let runtime = Runtime::default();
    let integer = runtime.root_value(Value::I64(i64::MIN)).unwrap();
    assert_eq!(
        integer.inspect(&runtime, |view| view.as_i64()).unwrap(),
        i64::MIN
    );
    assert!(integer.inspect(&runtime, |view| view.as_i32()).is_err());
    let bits = 0x7ff8_0000_0000_0042;
    let float = runtime
        .root_value(Value::F64(f64::from_bits(bits)))
        .unwrap();
    assert_eq!(
        float
            .inspect(&runtime, |view| Ok(view.as_f64()?.to_bits()))
            .unwrap(),
        bits
    );
    runtime.resources().close_execution();
    assert_eq!(
        integer.inspect(&runtime, |view| view.as_i64()).unwrap(),
        i64::MIN
    );
}

#[test]
fn string_inspection_keeps_storage_and_root_aliases_stable() {
    let runtime = Runtime::default();
    let text = runtime.gc().alloc_string("hello,雪".to_owned()).unwrap();
    let root = runtime.root_value(text).unwrap();
    let alias = root.clone();
    let unit = runtime.root_value(Value::Unit).unwrap();
    runtime.collect_garbage().unwrap();
    let owned = root
        .inspect(&runtime, |view| {
            assert_eq!(view.category(), ValueCategory::Primitive);
            let text = view.as_str()?;
            assert_eq!(&*text, "hello,雪");
            assert!(runtime.collect_garbage().is_err());
            assert!(runtime.gc().alloc_string("blocked".to_owned()).is_err());
            assert!(alias.replace(runtime.gc(), &unit).is_none());
            assert!(
                alias
                    .inspect(&runtime, |nested| Ok(nested.is_unit()))
                    .is_err()
            );
            assert_eq!(&*text, "hello,雪");
            Ok(text.to_owned())
        })
        .unwrap();
    assert_eq!(owned, "hello,雪");
    let pack = runtime.gc().root_execution_values(vec![text]).unwrap();
    let pack_alias = pack.clone();
    assert!(pack.inspect(&Runtime::default(), 0, |_| Ok(())).is_err());
    assert!(pack.inspect(&runtime, 1, |_| Ok(())).is_err());
    pack.inspect(&runtime, 0, |view| {
        assert!(pack_alias.replace(runtime.gc(), 0, &unit).is_none());
        assert!(runtime.collect_garbage().is_err());
        assert_eq!(&*view.as_str()?, "hello,雪");
        Ok(())
    })
    .unwrap();
    pack_alias.replace(runtime.gc(), 0, &unit).unwrap();
    assert!(
        pack.inspect(&runtime, 0, |view| Ok(view.is_unit()))
            .unwrap()
    );
    alias.replace(runtime.gc(), &unit).unwrap();
    assert!(root.inspect(&runtime, |view| Ok(view.is_unit())).unwrap());
    runtime.collect_garbage().unwrap();
}

#[test]
fn inspection_rejects_script_reentry_and_releases_on_unwind_or_error() {
    let mut runtime = Runtime::default();
    let owner = allocation_owner(&mut runtime);
    let root = runtime.root_value(Value::I32(42)).unwrap();
    root.inspect(&runtime, |view| {
        assert!(runtime.begin_execution(&owner, Default::default()).is_err());
        assert_eq!(view.as_i32()?, 42);
        Ok(())
    })
    .unwrap();
    assert!(
        catch_unwind(AssertUnwindSafe(|| {
            let _: Result<(), _> = root.inspect(&runtime, |_| panic!("inspection unwind"));
        }))
        .is_err()
    );
    let failure: Result<(), _> = root.inspect(&runtime, |_| {
        Err(RuntimeError::module_validation("inspection failure"))
    });
    assert!(failure.is_err());
    let options = ExecutionOptions::default();
    let cancellation = options.cancellation.clone();
    let session = runtime.begin_execution(&owner, options).unwrap();
    cancellation.cancel();
    assert!(runtime.resources().poll_execution().is_err());
    assert_eq!(root.inspect(&runtime, |view| view.as_i32()).unwrap(), 42);
    drop(session);
    runtime.collect_garbage().unwrap();
    assert_eq!(root.inspect(&runtime, |view| view.as_i32()).unwrap(), 42);
}

#[test]
fn foreign_and_disconnected_roots_never_enter_inspection() {
    let first = Runtime::default();
    let second = Runtime::default();
    let root = first.root_value(Value::I32(42)).unwrap();
    let entered = Cell::new(false);
    let inspect = |_: ValueView<'_>| {
        entered.set(true);
        Ok(())
    };
    assert!(root.inspect(&second, inspect).is_err());
    drop(first);
    assert!(root.inspect(&second, inspect).is_err());
    assert!(!entered.get());
}

#[test]
fn collection_callbacks_cannot_enter_user_inspection_even_if_they_ignore_rejection() {
    let runtime = Runtime::default();
    let root = runtime.root_value(Value::I32(42)).unwrap();
    let entered = Cell::new(false);
    assert!(
        runtime
            .resources()
            .collect_operation(|| {
                assert!(
                    root.inspect(&runtime, |_| {
                        entered.set(true);
                        Ok(())
                    })
                    .is_err()
                );
            })
            .is_err()
    );
    assert!(!entered.get());
    assert!(runtime.is_quarantined());
}
