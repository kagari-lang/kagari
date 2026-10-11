use crate::native_boundary_functions::fixture;
use kagari_runtime::{
    error::RuntimeError,
    native::{collections::vector::ScriptVec, objects::Object},
};
use std::panic::{AssertUnwindSafe, catch_unwind};

#[test]
fn typed_bulk_edits_keep_aliases_rooted_and_restore_completed_changes_on_failure() {
    let (vm, owner) = fixture(
        r#"
        pub struct Item { pub var value: i32 }
        pub fn make() -> Vec<Item> {
            Vec::from([Item { value: 1 }, Item { value: 2 }, Item { value: 2 }, Item { value: 3 }])
        }
    "#,
        None,
    );
    let vector: ScriptVec<Object> = vm.execute_typed(&owner, "make", ()).unwrap();
    let ty = vm.runtime().bind_type(&owner, "Item", &[]).unwrap();
    let field = vm.runtime().bind_field::<i32>(&ty, "value").unwrap();
    let mut cx = vm.context(&owner).unwrap();
    let alias = vector.clone();
    let mut calls = 0;
    assert!(
        vector
            .retain(&mut cx, |cx, item| {
                calls += 1;
                cx.collect_garbage()?;
                assert!(alias.push(cx, item.clone()).is_err());
                assert!(alias.get(cx, 0).is_err());
                match item.get(cx, &field)? {
                    1 => Ok(false),
                    2 => Ok(true),
                    _ => Err(RuntimeError::module_validation("predicate failure")),
                }
            })
            .is_err()
    );
    assert_eq!(calls, 4);
    assert_eq!(alias.len(&cx).unwrap(), 3);
    vector
        .dedup_by(&mut cx, |cx, left, right| {
            cx.collect_garbage()?;
            Ok(left.get(cx, &field)? == right.get(cx, &field)?)
        })
        .unwrap();
    assert_eq!(alias.len(&cx).unwrap(), 2);
    let first = alias.get(&mut cx, 0).unwrap().unwrap();
    assert_eq!(first.get(&mut cx, &field).unwrap(), 2);
    let result = catch_unwind(AssertUnwindSafe(|| {
        let _ = vector.retain(&mut cx, |cx, item| {
            if item.get(cx, &field)? == 2 {
                return Ok(false);
            }
            panic!("predicate unwind")
        });
    }));
    assert!(result.is_err());
    assert_eq!(alias.len(&cx).unwrap(), 1);
    first.set(&mut cx, &field, 42).unwrap();
    vector.push(&mut cx, first.clone()).unwrap();
    assert_eq!(
        alias
            .get(&mut cx, 1)
            .unwrap()
            .unwrap()
            .get(&mut cx, &field)
            .unwrap(),
        42
    );
    drop(first);
    drop(alias);
    drop(vector);
    assert_eq!(vm.runtime().collect_garbage().unwrap().live_objects, 0);
    assert_eq!(vm.runtime().gc().active_roots(), 0);
}
