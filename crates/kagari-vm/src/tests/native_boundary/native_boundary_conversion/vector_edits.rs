use crate::tests::native_boundary::native_boundary_functions::fixture;
use kagari_runtime::{error::RuntimeError, native::objects::Object};
use kagari_stdlib::collections::vector::ScriptVec;
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
    let _owner_program = owner;
    let owner = vm.runtime().select_program(&_owner_program).unwrap();
    let vector: ScriptVec<Object> = vm.execute_typed(&owner, "make", ()).unwrap();
    let ty = vm
        .runtime()
        .bind_type(&_owner_program.root(), "Item", &[])
        .unwrap();
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

#[test]
fn collecting_sort_preserves_stability_and_recovers_every_comparator_boundary() {
    let (vm, owner) = fixture(
        r#"
        pub struct Item { pub var key: i32, pub var serial: i32 }
        pub fn make() -> Vec<Item> {
            Vec::from([
                Item { key: 3, serial: 0 }, Item { key: 1, serial: 1 },
                Item { key: 2, serial: 2 }, Item { key: 1, serial: 3 },
                Item { key: 3, serial: 4 }, Item { key: 2, serial: 5 },
                Item { key: 0, serial: 6 }
            ])
        }
    "#,
        None,
    );
    let program = owner;
    let owner = vm.runtime().select_program(&program).unwrap();
    let ty = vm
        .runtime()
        .bind_type(&program.root(), "Item", &[])
        .unwrap();
    let key = vm.runtime().bind_field::<i32>(&ty, "key").unwrap();
    let serial = vm.runtime().bind_field::<i32>(&ty, "serial").unwrap();
    let mut cx = vm.context(&owner).unwrap();
    let vector: ScriptVec<Object> = vm.execute_typed(&owner, "make", ()).unwrap();
    let mut comparisons = 0;
    vector
        .sort_by(&mut cx, |cx, left, right| {
            comparisons += 1;
            cx.collect_garbage()?;
            Ok(left.get(cx, &key)?.cmp(&right.get(cx, &key)?))
        })
        .unwrap();
    let mut ordered = Vec::new();
    vector
        .for_each(&mut cx, |cx, item| {
            ordered.push(item.get(cx, &serial)?);
            Ok(())
        })
        .unwrap();
    assert_eq!(ordered, [6, 1, 3, 2, 5, 0, 4]);
    drop(vector);
    for unwind in [false, true] {
        for fail_at in 1..=comparisons {
            let vector: ScriptVec<Object> = vm.execute_typed(&owner, "make", ()).unwrap();
            let alias = vector.clone();
            let mut calls = 0;
            let outcome = catch_unwind(AssertUnwindSafe(|| {
                vector.sort_by(&mut cx, |cx, left, right| {
                    calls += 1;
                    cx.collect_garbage()?;
                    assert!(alias.get(cx, 0).is_err());
                    assert!(alias.push(cx, left.clone()).is_err());
                    if calls == fail_at {
                        if unwind {
                            panic!("sort comparison unwind");
                        }
                        return Err(RuntimeError::module_validation("sort comparison failure"));
                    }
                    Ok(left.get(cx, &key)?.cmp(&right.get(cx, &key)?))
                })
            }));
            assert_eq!(calls, fail_at, "no callbacks after failure");
            assert!(if unwind {
                outcome.is_err()
            } else {
                outcome.unwrap().is_err()
            });
            vm.runtime().collect_garbage().unwrap();
            let mut surviving = Vec::new();
            vector
                .for_each(&mut cx, |cx, item| {
                    surviving.push(item.get(cx, &serial)?);
                    Ok(())
                })
                .unwrap();
            surviving.sort_unstable();
            assert_eq!(surviving, [0, 1, 2, 3, 4, 5, 6]);
        }
    }
    assert_eq!(vm.runtime().collect_garbage().unwrap().live_objects, 0);
}
