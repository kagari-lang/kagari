use crate::managed::{
    fold::fold,
    vector::{VectorMut, VectorStorage},
};
use kagari_gc::{
    heap::{HeapConfig, Phase},
    object::Gc,
    scan::WorkBudget,
};
use kagari_runtime::{
    error::{RuntimeError, RuntimeErrorKind},
    native::{
        binding::NativeResult,
        builder::ModuleBuilder,
        catalog::DeclarationCatalog,
        context::CallContext,
        declarations::FunctionDecl,
        managed::{
            call::{CallRegion, Memory},
            entry::ScopedEntry,
            payload::{EdgeFree, Leaf, NativeObject},
            slots::{Object, Optional},
        },
        types::Type,
    },
};
use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

struct Counter {
    value: u64,
    drops: Arc<AtomicUsize>,
}

impl Drop for Counter {
    fn drop(&mut self) {
        self.drops.fetch_add(1, Ordering::Relaxed);
    }
}

// SAFETY: only a scalar and shared host-owned drop counter; no managed references,
// executable dependencies, privately owned buffers or reentrant destruction.
unsafe impl EdgeFree for Counter {}

type CounterObject = NativeObject<Leaf<Counter>>;
type VectorObject = NativeObject<VectorStorage<Object<CounterObject>>>;

fn pop<'gc>(
    region: &mut CallRegion<'_, 'gc>,
    (object,): (Gc<'gc, VectorObject>,),
) -> NativeResult<Option<Gc<'gc, CounterObject>>> {
    VectorMut::from_object(region, object)?.pop(region)
}

#[test]
fn scoped_returns_publish_removed_objects_and_take_preserves_typed_aliases() -> NativeResult<()> {
    let drops = Arc::new(AtomicUsize::new(0));
    let memory = Memory::new(HeapConfig {
        max_roots: 0,
        ..Default::default()
    })?;
    memory.call(|call| {
        let vector = call.local::<Object<VectorObject>>()?;
        let result = call.local::<Optional<Object<CounterObject>>>()?;
        call.region(|r| {
            let values = VectorMut::alloc(r)?;
            let counter = CounterObject::alloc(
                r,
                Counter {
                    value: 41,
                    drops: drops.clone(),
                },
            )?;
            CounterObject::edit(r, counter, |state| state.value += 1)?;
            values.push(r, counter)?;
            r.write(vector, values.object())
        })?;
        let pop =
            ScopedEntry::<(Object<VectorObject>,), Optional<Object<CounterObject>>, _>::new(pop);
        pop.invoke(call, (vector,), result)?;
        call.collect()?;
        assert_eq!(call.stats()?.objects, 2);
        call.region(|r| {
            let counter = r.read(result)?.unwrap();
            assert_eq!(CounterObject::read(r, counter, |state| state.0.value)?, 42);
            let value = CounterObject::take(r, counter)?;
            assert_eq!(value.value, 42);
            assert!(CounterObject::read(r, counter, |_| ()).is_err());
            assert!(CounterObject::take(r, counter).is_err());
            let object = r.read(vector)?;
            let values = VectorMut::from_object(r, object)?;
            assert!(values.push(r, counter).is_err());
            assert_eq!(values.readable().len(r)?, 0);
            drop(value);
            r.write(result, None)
        })?;
        call.collect()?;
        assert_eq!(drops.load(Ordering::Relaxed), 1);
        assert_eq!(call.stats()?.objects, 1);
        pop.invoke(call, (vector,), result)?;
        call.region(|r| {
            assert!(r.read(result)?.is_none());
            Ok(())
        })
    })?;
    memory.collect()?;
    assert_eq!(memory.stats()?.objects, 0);
    assert_eq!(memory.stats()?.root_registrations, 0);
    Ok(())
}

fn pop_then_fail<'gc>(
    r: &mut CallRegion<'_, 'gc>,
    (object,): (Gc<'gc, VectorObject>,),
) -> NativeResult<()> {
    pop(r, (object,))?;
    Err(RuntimeError::new(
        RuntimeErrorKind::ScriptTrap,
        "after removal",
    ))
}

#[test]
fn removal_errors_keep_completed_effects_and_nested_unwind_releases_slots() -> NativeResult<()> {
    let drops = Arc::new(AtomicUsize::new(0));
    let memory = Memory::new(HeapConfig::default())?;
    memory.call(|call| {
        let vector = call.local::<Object<VectorObject>>()?;
        let result = call.local::<()>()?;
        call.region(|r| {
            let values = VectorMut::alloc(r)?;
            let counter = CounterObject::alloc(
                r,
                Counter {
                    value: 7,
                    drops: drops.clone(),
                },
            )?;
            values.push(r, counter)?;
            r.write(vector, values.object())
        })?;
        let entry = ScopedEntry::<(Object<VectorObject>,), (), _>::new(pop_then_fail);
        let error = entry.invoke(call, (vector,), result).unwrap_err();
        assert_eq!(error.message(), "after removal");
        call.collect()?;
        assert_eq!(drops.load(Ordering::Relaxed), 1);
        call.region(|r| {
            let object = r.read(vector)?;
            assert!(VectorMut::from_object(r, object)?.readable().is_empty(r)?);
            Ok(())
        })?;
        let slots = call.working_slots()?;
        assert!(
            catch_unwind(AssertUnwindSafe(|| {
                let _: NativeResult<()> = call.reenter(|inner| {
                    let local = inner.local::<Object<CounterObject>>()?;
                    inner.region(|r| {
                        let counter = CounterObject::alloc(
                            r,
                            Counter {
                                value: 9,
                                drops: drops.clone(),
                            },
                        )?;
                        r.write(local, counter)
                    })?;
                    inner.collect()?;
                    panic!("native unwind")
                });
            }))
            .is_err()
        );
        assert_eq!(call.working_slots()?, slots);
        call.collect()?;
        assert_eq!(drops.load(Ordering::Relaxed), 2);
        Ok(())
    })?;
    memory.collect()?;
    assert_eq!(memory.stats()?.objects, 0);
    Ok(())
}

#[test]
fn vector_tail_edits_repair_incremental_scans_and_capacity_is_charged() -> NativeResult<()> {
    let drops = Arc::new(AtomicUsize::new(0));
    let memory = Memory::new(HeapConfig {
        max_roots: 0,
        ..Default::default()
    })?;
    memory.call(|call| {
        let vector = call.local::<Object<VectorObject>>()?;
        call.region(|r| {
            let values = VectorMut::alloc(r)?;
            for value in 0..64 {
                let counter = CounterObject::alloc(
                    r,
                    Counter {
                        value,
                        drops: drops.clone(),
                    },
                )?;
                values.push(r, counter)?;
            }
            r.write(vector, values.object())
        })?;
        assert!(call.stats()?.indirect_bytes > 0);
        for _ in 0..1000 {
            call.step(WorkBudget::new(8))?;
            if call.stats()?.phase == Phase::Mark {
                break;
            }
        }
        assert_eq!(call.stats()?.phase, Phase::Mark);
        call.step(WorkBudget::new(8))?;
        call.region(|r| {
            let object = r.read(vector)?;
            let values = VectorMut::from_object(r, object)?;
            for _ in 0..32 {
                values.pop(r)?;
            }
            for value in 64..128 {
                let counter = CounterObject::alloc(
                    r,
                    Counter {
                        value,
                        drops: drops.clone(),
                    },
                )?;
                values.push(r, counter)?;
            }
            assert_eq!(values.readable().len(r)?, 96);
            Ok(())
        })?;
        call.collect()?;
        call.collect()?;
        assert_eq!(call.stats()?.objects, 97);
        assert_eq!(drops.load(Ordering::Relaxed), 32);
        assert_eq!(call.stats()?.root_registrations, 0);
        Ok(())
    })?;
    memory.collect()?;
    assert_eq!(drops.load(Ordering::Relaxed), 128);
    assert_eq!(memory.stats()?.indirect_bytes, 0);
    Ok(())
}

#[test]
fn host_snapshots_preserve_scalar_owner_and_object_liveness() -> NativeResult<()> {
    let memory = Memory::new(HeapConfig::default())?;
    let other = Memory::new(HeapConfig::default())?;
    let drops = Arc::new(AtomicUsize::new(0));
    let (object, scalar) = memory.call(|call| {
        let object = call.local::<Object<CounterObject>>()?;
        let scalar = call.local::<u64>()?;
        call.region(|r| {
            let value = CounterObject::alloc(
                r,
                Counter {
                    value: 42,
                    drops: drops.clone(),
                },
            )?;
            r.write(object, value)?;
            r.write(scalar, u64::MAX)?;
            Ok((r.root(object)?, r.root(scalar)?))
        })
    })?;
    let retained = object.clone();
    drop(object);
    memory.collect()?;
    assert_eq!(memory.stats()?.objects, 1);
    other.call(|call| {
        call.region(|r| {
            assert_eq!(
                r.admit_value(&scalar).unwrap_err().kind(),
                RuntimeErrorKind::StaleHandle
            );
            assert_eq!(
                r.admit_value(&retained).unwrap_err().kind(),
                RuntimeErrorKind::StaleHandle
            );
            Ok(())
        })
    })?;
    memory.call(|call| {
        call.region(|r| {
            assert_eq!(r.admit_value(&scalar)?, u64::MAX);
            let counter = r.admit_value(&retained)?;
            assert_eq!(CounterObject::read(r, counter, |value| value.0.value)?, 42);
            Ok(())
        })
    })?;
    drop(retained);
    memory.collect()?;
    assert_eq!(drops.load(Ordering::Relaxed), 1);
    assert_eq!(memory.stats()?.objects, 0);
    Ok(())
}

fn add_one(_: &mut CallRegion<'_, '_>, (value,): (u64,)) -> NativeResult<u64> {
    Ok(value + 1)
}

#[test]
fn scalar_preparation_rejects_wrong_declarations_before_effects_and_reuses_typed_entry()
-> NativeResult<()> {
    let mut builder = ModuleBuilder::new("example::scalar_proof", &DeclarationCatalog::default())?;
    let good = builder.define_function(
        FunctionDecl::new("good")
            .parameter("value", Type::u64())
            .returns(Type::u64()),
    )?;
    builder.bind(
        good,
        |_: &mut CallContext<'_>, value: u64| -> NativeResult<u64> { Ok(value) },
    )?;
    let wrong = builder.define_function(
        FunctionDecl::new("wrong")
            .parameter("value", Type::u64())
            .returns(Type::bool()),
    )?;
    builder.bind(
        wrong,
        |_: &mut CallContext<'_>, _: u64| -> NativeResult<bool> { Ok(false) },
    )?;
    let module = builder.finish()?;
    assert!(
        ScopedEntry::<(u64,), u64, _>::prepare_scalar(&module.declaration().functions[1], add_one)
            .is_err()
    );
    let entry =
        ScopedEntry::<(u64,), u64, _>::prepare_scalar(&module.declaration().functions[0], add_one)?;
    let memory = Memory::new(HeapConfig {
        max_roots: 0,
        ..Default::default()
    })?;
    memory.call(|call| {
        let value = call.local::<u64>()?;
        call.region(|r| r.write(value, 0))?;
        for _ in 0..100 {
            entry.invoke(call, (value,), value)?;
        }
        assert_eq!(call.read_scalar(value)?, 100);
        Ok(())
    })
}

#[test]
fn shared_memory_entry_rejects_live_regions_and_allows_fresh_reentry() -> NativeResult<()> {
    let memory = Memory::new(Default::default())?;
    memory.call(|outer| {
        outer.region(|_| {
            assert_eq!(
                memory.collect().unwrap_err().kind(),
                RuntimeErrorKind::HostBorrowConflict
            );
            assert_eq!(
                memory.call(|_| Ok(())).unwrap_err().kind(),
                RuntimeErrorKind::HostBorrowConflict
            );
            Ok(())
        })?;
        memory.call(|inner| {
            let value = inner.local::<u64>()?;
            inner.region(|r| r.write(value, 42))?;
            inner.collect()?;
            assert_eq!(inner.read_scalar(value)?, 42);
            Ok(())
        })
    })
}

type ScalarVector = NativeObject<VectorStorage<u64>>;

fn push_scalar<'gc>(
    r: &mut CallRegion<'_, 'gc>,
    (values, value): (Gc<'gc, ScalarVector>, u64),
) -> NativeResult<()> {
    VectorMut::from_object(r, values)?.push(r, value)
}

fn pop_scalar<'gc>(
    r: &mut CallRegion<'_, 'gc>,
    (values,): (Gc<'gc, ScalarVector>,),
) -> NativeResult<Option<u64>> {
    VectorMut::from_object(r, values)?.pop(r)
}

#[test]
fn scalar_vectors_use_the_shared_value_family_without_boxing_or_per_item_roots() -> NativeResult<()>
{
    let memory = Memory::new(HeapConfig {
        max_roots: 0,
        ..Default::default()
    })?;
    memory.call(|call| {
        let vector = call.local::<Object<ScalarVector>>()?;
        let input = call.local::<u64>()?;
        let unit = call.local::<()>()?;
        let result = call.local::<Optional<u64>>()?;
        call.region(|r| {
            let values = VectorMut::alloc(r)?;
            r.write(vector, values.object())
        })?;
        let push = ScopedEntry::<(Object<ScalarVector>, u64), (), _>::new(push_scalar);
        let pop = ScopedEntry::<(Object<ScalarVector>,), Optional<u64>, _>::new(pop_scalar);
        for value in 0..128 {
            call.region(|r| r.write(input, value))?;
            push.invoke(call, (vector, input), unit)?;
        }
        call.collect()?;
        assert_eq!(call.stats()?.objects, 1);
        assert_eq!(call.stats()?.root_registrations, 0);
        pop.invoke(call, (vector,), result)?;
        call.region(|r| {
            assert_eq!(r.read(result)?, Some(127));
            Ok(())
        })?;
        Ok(())
    })?;
    memory.collect()?;
    assert_eq!(memory.stats()?.objects, 0);
    Ok(())
}

#[test]
fn fold_reports_alias_mutation_on_the_next_read_even_after_the_last_callback() -> NativeResult<()> {
    let memory = Memory::new(Default::default())?;
    memory.call(|call| {
        let vector = call.local::<Object<ScalarVector>>()?;
        call.region(|r| {
            let values = VectorMut::alloc(r)?;
            values.push(r, 42)?;
            r.write(vector, values.object())
        })?;
        let error = fold(
            call,
            vector,
            0,
            |_, value| Ok(value),
            &mut |call, sum, item| {
                call.region(|r| {
                    let object = r.read(vector)?;
                    VectorMut::from_object(r, object)?.pop(r)?;
                    Ok(())
                })?;
                Ok(sum + item)
            },
        )
        .unwrap_err();
        assert_eq!(error.kind(), RuntimeErrorKind::ScriptTrap);
        call.region(|r| {
            let object = r.read(vector)?;
            assert_eq!(VectorMut::from_object(r, object)?.readable().len(r)?, 0);
            Ok(())
        })
    })
}
