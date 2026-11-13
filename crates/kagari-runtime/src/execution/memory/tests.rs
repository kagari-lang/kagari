use crate::{
    execution::{
        memory::{GcHeapConfig, ManagedMemory},
        roots::{
            DependencyId, ExecutionRoots, ExecutionStorage, NativeSourceMark, SessionId, Slots,
            ValueBank, ValueLocation,
        },
    },
    resource::ResourceState,
    value::{
        aggregates::tuples::TupleObject,
        carriers::{RootedValue, ScopedValue, StoredValue},
        cells::CellObject,
        ephemeral::EphemeralValueId,
        properties::ValueProperties,
        strings::StringObject,
        view::ValueView,
    },
};
use kagari_abi::representation::ValueType;
use kagari_gc::{error::GcError, heap::Phase};

#[test]
fn execution_source_survives_pressure_and_pending_steps_without_new_allocation()
-> Result<(), GcError> {
    let resources = &ResourceState::default();
    let mut memory = ManagedMemory::new(GcHeapConfig {
        collection_threshold: Some(1),
        ..GcHeapConfig::default()
    });
    assert!(!memory.service_requested(false));
    const COUNT: usize = 4096;
    const TEXT: &str = "protected UTF-8 🙂";
    let execution = memory.enter_execution(resources)?;
    let transfer = |slots: &mut Slots<'_, '_>, slot| {
        let source = ValueLocation {
            execution,
            bank: ValueBank::Managed,
            slot,
        };
        let value = slots.get(source)?;
        let text = slots
            .scope()
            .downcast::<StringObject>(value.object().unwrap())?;
        let dependency = slots.push_dependency(execution, text)?;
        slots.set(source, ScopedValue::unit())?;
        Ok::<_, GcError>(dependency)
    };
    let prefix = memory.scope(resources, |cx, roots| {
        ExecutionRoots::with(roots, cx, |slots| {
            slots.grow(execution, ValueBank::Managed, COUNT)?;
            for slot in 0..COUNT {
                let text = StringObject::alloc(slots.scope(), TEXT.to_owned())?;
                slots.set(
                    ValueLocation {
                        execution,
                        bank: ValueBank::Managed,
                        slot,
                    },
                    ScopedValue::managed(text),
                )?;
            }
            transfer(slots, 0)
        })
    })?;
    assert!(memory.service_requested(false));
    memory.safepoint(resources)?;
    assert_eq!(memory.stats().phase, Phase::Roots);
    // Both new banks append behind captured root frontiers. Move still-white
    // targets from the unscanned managed suffix into a Native pending cell and
    // dependency slots, then remove their old roots before collection continues.
    let (native, temporary, late, independent, body, backing) =
        memory.scope(resources, |cx, roots| {
            ExecutionRoots::with(roots, cx, |slots| {
                let native = slots.enter_native(execution)?;
                slots.set_native_origin(&native, prefix)?;
                assert!(matches!(
                    slots.set_native_origin(&native, prefix),
                    Err(GcError::Storage("Native origin already admitted"))
                ));
                let pending = slots.native_result(&native)?;
                let source = ValueLocation {
                    execution,
                    bank: ValueBank::Managed,
                    slot: COUNT - 2,
                };
                let value = slots.get(source)?;
                slots.set(pending, value)?;
                slots.set(source, ScopedValue::unit())?;
                let nested = slots.enter_native(execution)?;
                let nested_result = slots.native_result(&nested)?;
                assert_ne!(pending, nested_result);
                slots.set(nested_result, ScopedValue::from(u64::MAX))?;
                assert_eq!(
                    slots.get(nested_result)?.scalar_payload(),
                    Some((ValueType::U64, u64::MAX))
                );
                slots.retire_native(nested)?;
                assert_eq!(slots.native_result(&native)?, pending);
                let value = slots.get(pending)?;
                let text = slots
                    .scope()
                    .downcast::<StringObject>(value.object().unwrap())?;
                assert!(slots.scope().read(text, |text| text.as_str() == TEXT));
                let temporary = transfer(slots, 1)?;
                let source = ValueLocation {
                    execution,
                    bank: ValueBank::Managed,
                    slot: COUNT - 1,
                };
                let value = slots.get(source)?;
                let text = slots
                    .scope()
                    .downcast::<StringObject>(value.object().unwrap())?;
                let dependency = slots.push_dependency(execution, text)?;
                slots.set(source, ScopedValue::unit())?;
                // A newly admitted execution lies beyond the captured source-row
                // frontier. Resolve from the old protected source before growth,
                // then publish its sole body dependency without a Host root.
                let source = ValueLocation {
                    execution,
                    bank: ValueBank::Managed,
                    slot: COUNT - 3,
                };
                let value = slots.get(source)?;
                let text = slots
                    .scope()
                    .downcast::<StringObject>(value.object().unwrap())?;
                let independent = slots.enter(0)?;
                let body = slots.push_dependency(independent, text)?;
                slots.set(source, ScopedValue::unit())?;
                Ok((
                    native,
                    temporary,
                    dependency,
                    independent,
                    body,
                    slots.storage(execution)?.backing_bytes(),
                ))
            })
        })?;
    // Logical Native origin is source-owned metadata, distinct from its pending
    // result. The innermost record wins, then retirement restores its caller.
    let (inner, inner_body) = memory.scope(resources, |cx, roots| {
        ExecutionRoots::with(roots, cx, |slots| {
            let value = slots.get(native.result_location())?;
            let text = slots
                .scope()
                .downcast::<StringObject>(value.object().unwrap())?;
            let inner = slots.enter_native(execution)?;
            let body = slots.push_dependency(execution, text)?;
            slots.set_native_origin(&inner, body)?;
            Ok((inner, body))
        })
    })?;
    let observed = memory.native_origin::<StringObject>(execution)?.unwrap();
    memory.scope(resources, |cx, roots| {
        ExecutionRoots::with(roots, cx, |slots| {
            let actual = slots.dependency(observed)?;
            assert!(actual.ptr_eq(slots.dependency(inner_body)?));
            assert!(!actual.ptr_eq(slots.dependency(prefix)?));
            slots.retire_native(inner)
        })
    })?;
    let observed = memory.native_origin::<StringObject>(execution)?.unwrap();
    memory.scope(resources, |cx, roots| {
        ExecutionRoots::with(roots, cx, |slots| {
            let actual = slots.dependency(observed)?;
            assert!(actual.ptr_eq(slots.dependency(prefix)?));
            assert!(matches!(slots.dependency(inner_body), Err(GcError::Index)));
            Ok(())
        })
    })?;
    // No allocations or source changes in this loop. Pending collector work,
    // rather than another allocation, must cause every subsequent service.
    let mut steps = 1;
    while memory.service_requested(false) {
        assert!(steps < COUNT, "pending collection stopped making progress");
        memory.safepoint(resources)?;
        steps += 1;
    }
    let stats = memory.stats();
    assert_eq!(stats.phase, Phase::Idle);
    assert_eq!(stats.completed_cycles, 1);
    assert_eq!(stats.objects, COUNT);
    assert_eq!(stats.provider_units, (COUNT * TEXT.len()) as u128);
    assert_eq!(stats.root_registrations, 0);
    assert_eq!(
        memory.retire_execution(resources, execution),
        Err(GcError::Storage("execution has an active Native suffix"))
    );
    memory.scope(resources, |cx, roots| {
        ExecutionRoots::with(roots, cx, |slots| {
            for dependency in [prefix, temporary, late] {
                let text = slots.dependency(dependency)?;
                assert!(slots.scope().read(text, |text| text.as_str() == TEXT));
            }
            slots.retire_native(native)?;
            assert!(matches!(slots.dependency(temporary), Err(GcError::Index)));
            assert!(matches!(slots.dependency(late), Err(GcError::Index)));
            let text = slots.dependency(prefix)?;
            assert!(slots.scope().read(text, |text| text.as_str() == TEXT));
            Ok(())
        })
    })?;
    // Non-owning keys do not retain retired Native dependencies. Both objects
    // die, along with the pending result, while the execution prefix survives.
    assert_eq!(memory.collect(resources)?.retired_owners, 3);
    assert_eq!(memory.stats().objects, COUNT - 3);
    assert_eq!(
        memory.stats().provider_units,
        ((COUNT - 3) * TEXT.len()) as u128
    );
    memory.retire_execution(resources, execution)?;
    assert!(matches!(
        memory.native_origin::<StringObject>(execution),
        Err(GcError::Index)
    ));
    let report = memory.collect(resources)?;
    assert_eq!(report.retired_owners, COUNT - 4);
    assert_eq!(memory.stats().objects, 1);
    memory.scope(resources, |cx, roots| {
        ExecutionRoots::with(roots, cx, |slots| {
            assert!(matches!(slots.dependency(prefix), Err(GcError::Index)));
            let text = slots.dependency(body)?;
            assert!(slots.scope().read(text, |text| text.as_str() == TEXT));
            Ok(())
        })
    })?;
    memory.retire_execution(resources, independent)?;
    assert_eq!(memory.collect(resources)?.retired_owners, 1);
    assert_eq!(memory.stats().objects, 0);
    assert_eq!(memory.stats().provider_units, 0);
    assert_eq!(memory.stats().root_registrations, 0);
    assert!(!memory.service_requested(false));
    println!("managed pressure: {COUNT} protected strings, {steps} service steps");
    println!(
        "value sizes: scoped={} stored={} rooted={} tuple={}",
        size_of::<ScopedValue<'_>>(),
        size_of::<StoredValue>(),
        size_of::<RootedValue>(),
        size_of::<TupleObject>(),
    );
    println!(
        "source sizes: id={} location={} view={} dependency={} native_mark={} storage={} backing={backing}",
        size_of::<SessionId>(),
        size_of::<ValueLocation>(),
        size_of::<ValueView<'_>>(),
        size_of::<DependencyId<StringObject>>(),
        size_of::<NativeSourceMark>(),
        size_of::<ExecutionStorage>(),
    );
    Ok(())
}

/// A child may allocate discarded temporaries before its actual result. The
/// result must move into the retained prefix before the scratch suffix expires.
#[test]
fn conversion_result_survives_child_scratch_and_host_retention() -> Result<(), GcError> {
    let resources = &ResourceState::default();
    let mut memory = ManagedMemory::new(GcHeapConfig::default());
    let execution = memory.enter_execution(resources)?;
    let (parent, result, discarded, prefix) = memory.scope(resources, |cx, roots| {
        ExecutionRoots::with(roots, cx, |slots| {
            let prefix_text = StringObject::alloc(slots.scope(), "prefix dependency".to_owned())?;
            let prefix = slots.push_dependency(execution, prefix_text)?;
            let parent = slots.mark_locals(execution)?;
            let mut child = slots.mark_locals(execution)?;
            let discarded = StringObject::alloc(slots.scope(), "discarded".to_owned())?;
            slots.push_local(execution, ScopedValue::managed(discarded))?;
            // Preparation nodes use the same typed dependency bank as frame
            // metadata. Retiring scratch must clear this edge as well as values.
            let prepared = slots.push_dependency(execution, discarded)?;
            let text = StringObject::alloc(slots.scope(), "returned UTF-8 🙂".to_owned())?;
            let temporary = ScopedValue::runtime_ephemeral(EphemeralValueId(7));
            assert!(temporary.object().is_none());
            assert!(temporary.scalar_payload().is_none());
            let result = TupleObject::alloc(
                slots.scope(),
                &[
                    ScopedValue::managed(text),
                    temporary,
                    ScopedValue::from(u64::MAX),
                ],
            )?;
            let value = ScopedValue::managed(result);
            let old_location = slots.push_local(execution, value)?;
            let old_view = ValueView::protected(value, Some(old_location))?;
            let location = slots.preserve_local(&mut child, value)?;
            let view = ValueView::protected(value, Some(location))?;
            let reopened = view.get(slots)?.object().unwrap();
            assert!(result.ptr_eq(reopened));
            assert!(!discarded.ptr_eq(reopened));
            slots.retire_locals(child)?;
            assert!(matches!(old_view.get(slots), Err(GcError::Index)));
            assert!(matches!(slots.dependency(prepared), Err(GcError::Index)));
            let prefix_text = slots.dependency(prefix)?;
            assert!(
                slots
                    .scope()
                    .read(prefix_text, |text| text.as_str() == "prefix dependency")
            );
            Ok((parent, view, old_view, prefix))
        })
    })?;
    assert_eq!(memory.stats().root_registrations, 0);
    assert_eq!(memory.collect(resources)?.retired_owners, 1);
    assert_eq!(memory.stats().objects, 3);
    let retained = memory.scope(resources, |cx, roots| {
        ExecutionRoots::with(roots, cx, |slots| {
            assert!(matches!(discarded.get(slots), Err(GcError::Index)));
            let value = result.get(slots)?;
            let tuple = slots
                .scope()
                .downcast::<TupleObject>(value.object().unwrap())?;
            assert_eq!(slots.scope().read(tuple, TupleObject::len), 3);
            let facts = ValueProperties::of(slots.scope(), value)?;
            assert!(!facts.storable);
            assert!(facts.ephemeral);
            assert!(!facts.host_borrow);
            let text = TupleObject::get(slots.scope(), tuple, 0)?.object().unwrap();
            let text = slots.scope().downcast::<StringObject>(text)?;
            assert!(
                slots
                    .scope()
                    .read(text, |text| text.as_str() == "returned UTF-8 🙂")
            );
            let temporary = TupleObject::get(slots.scope(), tuple, 1)?;
            assert_eq!(temporary.as_runtime_ephemeral(), Some(EphemeralValueId(7)));
            assert!(temporary.object().is_none());
            let number = TupleObject::get(slots.scope(), tuple, 2)?;
            assert_eq!(number.scalar_payload().unwrap().1, u64::MAX);
            // Nested Native pending cells own disjoint suffixes, including when
            // the outer result is managed and the inner result is a full scalar.
            let top = slots.len(execution, ValueBank::Local)?;
            let outer = slots.enter_native(execution)?;
            let pending = slots.native_result(&outer)?;
            slots.set(pending, value)?;
            let crossing = slots.mark_locals(execution)?;
            let inner = slots.enter_native(execution)?;
            let inner_pending = slots.native_result(&inner)?;
            assert_ne!(pending, inner_pending);
            slots.set(inner_pending, ScopedValue::from(u64::MAX))?;
            assert!(matches!(
                slots.retire_locals(crossing),
                Err(GcError::Storage("conversion suffix crosses a live call"))
            ));
            assert_eq!(
                slots.get(inner_pending)?.scalar_payload(),
                Some((ValueType::U64, u64::MAX))
            );
            slots.retire_native(inner)?;
            assert_eq!(slots.native_result(&outer)?, pending);
            assert!(
                slots
                    .get(pending)?
                    .object()
                    .unwrap()
                    .ptr_eq(value.object().unwrap())
            );
            assert!(matches!(slots.get(inner_pending), Err(GcError::Index)));
            slots.retire_native(outer)?;
            assert_eq!(slots.len(execution, ValueBank::Local)?, top);
            // The argument owner compacts a completed mixed/aliased pack over
            // discarded preparation cells without duplicating roots or losing
            // full-width scalars. Its original source can then be cleared.
            let mut pack = slots.mark_locals(execution)?;
            let pack_start = pack.top();
            slots.push_local(execution, ScopedValue::unit())?;
            slots.push_local(execution, ScopedValue::from(3i32))?;
            slots.reserve_locals(execution, 3)?;
            let first = slots.push_local(execution, value)?;
            slots.push_local(execution, ScopedValue::from(u64::MAX))?;
            slots.push_local(execution, value)?;
            assert!(matches!(
                slots.preserve_local_pack(&mut pack, first.slot, 2),
                Err(GcError::Storage("argument pack is not the current suffix"))
            ));
            slots.preserve_local_pack(&mut pack, first.slot, 3)?;
            let at = |slot| ValueLocation {
                execution,
                bank: ValueBank::Local,
                slot,
            };
            slots.set(result.location().unwrap(), ScopedValue::unit())?;
            let value = slots.get(at(pack_start))?;
            let alias = slots.get(at(pack_start + 2))?;
            assert!(value.object().unwrap().ptr_eq(alias.object().unwrap()));
            assert_eq!(
                slots.get(at(pack_start + 1))?.scalar_payload().unwrap().1,
                u64::MAX
            );
            assert!(matches!(slots.get(at(pack_start + 3)), Err(GcError::Index)));
            slots.retire_locals(pack)?;
            assert_eq!(slots.len(execution, ValueBank::Local)?, pack_start + 3);
            // Only this explicit Host escape registers an independent lease.
            let retained = value.retain(slots.scope())?;
            slots.retire_locals(parent)?;
            assert!(matches!(result.get(slots), Err(GcError::Index)));
            let prefix_text = slots.dependency(prefix)?;
            assert!(
                slots
                    .scope()
                    .read(prefix_text, |text| text.as_str() == "prefix dependency")
            );
            Ok(retained)
        })
    })?;
    memory.retire_execution(resources, execution)?;
    assert_eq!(memory.collect(resources)?.retired_owners, 1);
    assert_eq!(memory.stats().root_registrations, 1);
    memory.scope(resources, |cx, _| {
        let value = retained.get(cx)?;
        let tuple = cx.downcast::<TupleObject>(value.object().unwrap())?;
        let text = TupleObject::get(cx, tuple, 0)?.object().unwrap();
        let text = cx.downcast::<StringObject>(text)?;
        assert!(cx.read(text, |text| text.as_str() == "returned UTF-8 🙂"));
        Ok(())
    })?;
    drop(retained);
    // The Host registration, returned tuple and its String are separate owners.
    assert_eq!(memory.collect(resources)?.retired_owners, 3);
    assert_eq!(memory.stats().root_registrations, 0);
    assert_eq!(memory.stats().objects, 0);
    assert_eq!(memory.stats().provider_units, 0);
    Ok(())
}

#[test]
fn capture_cells_preserve_contracts_and_retire_cycles() -> Result<(), GcError> {
    let resources = &ResourceState::default();
    let mut memory = ManagedMemory::new(GcHeapConfig::default());
    let execution = memory.enter_execution(resources)?;
    let cell = memory.scope(resources, |cx, roots| {
        ExecutionRoots::with(roots, cx, |slots| {
            let cell = CellObject::alloc(
                slots.scope(),
                ValueType::Generic,
                ScopedValue::unit(),
                Default::default(),
            )?;
            let key = slots.push_dependency(execution, cell)?;
            let text = StringObject::alloc(slots.scope(), "shared capture".to_owned())?;
            CellObject::set(
                slots.scope(),
                cell,
                ValueType::Generic,
                ScopedValue::managed(text),
            )?;
            Ok(key)
        })
    })?;
    memory.collect(resources)?;
    assert_eq!(memory.stats().objects, 2);
    assert_eq!(memory.stats().root_registrations, 0);
    memory.scope(resources, |cx, roots| {
        ExecutionRoots::with(roots, cx, |slots| {
            let cell = slots.dependency(cell)?;
            // A wrong requested physical contract and a nested non-storable
            // temporary both fail without overwriting the already committed text.
            assert!(matches!(
                CellObject::set(
                    slots.scope(),
                    cell,
                    ValueType::I64,
                    ScopedValue::from(42i64)
                ),
                Err(GcError::Representation)
            ));
            assert!(matches!(
                CellObject::get(slots.scope(), cell, ValueType::HeapObject),
                Err(GcError::Representation)
            ));
            let temporary = ScopedValue::runtime_ephemeral(EphemeralValueId(1));
            let invalid = TupleObject::alloc(slots.scope(), &[temporary])?;
            assert!(matches!(
                CellObject::set(
                    slots.scope(),
                    cell,
                    ValueType::Generic,
                    ScopedValue::managed(invalid)
                ),
                Err(GcError::Representation)
            ));
            let value = CellObject::get(slots.scope(), cell, ValueType::Generic)?;
            let text = slots
                .scope()
                .downcast::<StringObject>(value.object().unwrap())?;
            assert!(
                slots
                    .scope()
                    .read(text, |text| text.as_str() == "shared capture")
            );
            Ok(())
        })
    })?;
    assert_eq!(memory.collect(resources)?.retired_owners, 1);
    assert_eq!(memory.stats().objects, 2);
    memory.scope(resources, |cx, roots| {
        ExecutionRoots::with(roots, cx, |slots| {
            let cell = slots.dependency(cell)?;
            CellObject::set(
                slots.scope(),
                cell,
                ValueType::Generic,
                ScopedValue::from(u64::MAX),
            )?;
            let value = CellObject::get(slots.scope(), cell, ValueType::Generic)?;
            assert_eq!(value.scalar_payload(), Some((ValueType::U64, u64::MAX)));
            Ok(())
        })
    })?;
    assert_eq!(memory.collect(resources)?.retired_owners, 1);
    assert_eq!(memory.stats().objects, 1);
    assert_eq!(memory.stats().provider_units, 1);
    memory.scope(resources, |cx, roots| {
        ExecutionRoots::with(roots, cx, |slots| {
            let cell = slots.dependency(cell)?;
            CellObject::set(
                slots.scope(),
                cell,
                ValueType::Generic,
                ScopedValue::managed(cell),
            )?;
            let value = CellObject::get(slots.scope(), cell, ValueType::Generic)?;
            assert!(cell.ptr_eq(value.object().unwrap()));
            Ok(())
        })
    })?;
    memory.collect(resources)?;
    assert_eq!(memory.stats().objects, 1);
    memory.retire_execution(resources, execution)?;
    assert_eq!(memory.collect(resources)?.retired_owners, 1);
    assert_eq!(memory.stats().objects, 0);
    assert_eq!(memory.stats().provider_units, 0);
    assert_eq!(memory.stats().root_registrations, 0);
    Ok(())
}

#[test]
fn frame_suffixes_rollback_initialize_and_retire_without_window_roots() -> Result<(), GcError> {
    let resources = &ResourceState::default();
    let mut memory = ManagedMemory::new(GcHeapConfig::default());
    let execution = memory.enter_execution(resources)?;
    let independent = memory.enter_execution(resources)?;
    let (native, parent, child, value, other) = memory.scope(resources, |cx, roots| {
        ExecutionRoots::with(roots, cx, |slots| {
            let native = slots.enter_native(execution)?;
            let parent = slots.enter_frame(execution, 1, 1, || Ok::<_, GcError>(()))?;
            let parent_banks = parent.banks();
            let text = StringObject::alloc(slots.scope(), "frame-owned UTF-8 🙂".into())?;
            let text = ScopedValue::managed(text);
            let parent_value = ValueLocation {
                execution,
                bank: ValueBank::Managed,
                slot: parent_banks.managed,
            };
            slots.set(parent_value, text)?;
            let storage = slots.storage(execution)?;
            storage.scalars[parent_banks.scalars] = u64::MAX;
            storage.initialized[parent_banks.scalars] = true;
            let child = slots.enter_frame(execution, 2, 2, || Ok::<_, GcError>(()))?;
            let child_banks = child.banks();
            let destination = ValueLocation {
                execution,
                bank: ValueBank::Managed,
                slot: child_banks.managed + 1,
            };
            slots.set(destination, text)?;
            slots.set(parent_value, ScopedValue::unit())?;
            let inner = slots.enter_native(execution)?;
            assert!(matches!(
                slots.check_frame_exit(&child),
                Err(GcError::Storage("frame retirement is out of order"))
            ));
            slots.retire_native(inner)?;
            assert!(matches!(
                slots.check_frame_exit(&parent),
                Err(GcError::Storage("frame retirement is out of order"))
            ));
            assert!(matches!(
                slots.native_result(&native),
                Err(GcError::Storage("Native suffix is not current"))
            ));
            // Reject after managed initialization and part of scalar/flag
            // initialization. No partial frame or changed parent may remain.
            let mut polls = 0;
            assert!(matches!(
                slots.enter_frame(execution, 2048, 2048, || {
                    polls += 1;
                    if polls == 2 {
                        Err(GcError::Limit("initialization stop"))
                    } else {
                        Ok(())
                    }
                }),
                Err(GcError::Limit("initialization stop"))
            ));
            assert_eq!(polls, 2);
            slots.check_frame_exit(&child)?;
            assert_eq!(
                slots.len(execution, ValueBank::Managed)?,
                child_banks.managed + 2
            );
            let storage = slots.storage(execution)?;
            assert_eq!(storage.scalars.len(), child_banks.scalars + 2);
            assert_eq!(storage.initialized.len(), storage.scalars.len());
            assert_eq!(storage.scalars[parent_banks.scalars], u64::MAX);
            assert!(storage.initialized[parent_banks.scalars]);
            let other = slots.enter_frame(independent, 0, 1, || Ok::<_, GcError>(()))?;
            let storage = slots.storage(independent)?;
            storage.scalars[other.banks().scalars] = 777;
            storage.initialized[other.banks().scalars] = true;
            Ok((native, parent, child, destination, other))
        })
    })?;
    let parent_banks = parent.banks();
    let child_banks = child.banks();
    let other_banks = other.banks();
    assert!(memory.contains_frame_banks(parent_banks));
    assert!(memory.contains_frame_banks(child_banks));
    // Cold queries copy scalar/tag/coordinate metadata from the installed source,
    // without opening a mutable heap scope or retaining the observed object.
    let value_location = value;
    let value = memory.inspect_value(value_location)?;
    assert!(value.has_representation(ValueType::Str));
    assert_eq!(
        memory.inspect_scalar(execution, parent_banks.scalars)?,
        Some(u64::MAX)
    );
    assert_eq!(memory.inspect_scalar(execution, child_banks.scalars)?, None);
    let foreign = ManagedMemory::new(GcHeapConfig::default());
    assert!(matches!(
        foreign.inspect_value(value_location),
        Err(GcError::ForeignHeap)
    ));
    assert!(matches!(
        foreign.inspect_scalar(execution, parent_banks.scalars),
        Err(GcError::ForeignHeap)
    ));
    assert_eq!(memory.stats().root_registrations, 0);
    memory.collect(resources)?;
    assert_eq!(memory.stats().objects, 1);
    memory.scope(resources, |cx, roots| {
        ExecutionRoots::with(roots, cx, |slots| {
            let text = value.get(slots)?.object().unwrap();
            let text = slots.scope().downcast::<StringObject>(text)?;
            assert!(
                slots
                    .scope()
                    .read(text, |text| text.as_str() == "frame-owned UTF-8 🙂")
            );
            slots.retire_frame(child)?;
            assert!(matches!(value.get(slots), Err(GcError::Index)));
            let reused = slots.enter_frame(execution, 2, 2, || Ok::<_, GcError>(()))?;
            assert_eq!(reused.banks(), child_banks);
            let storage = slots.storage(execution)?;
            assert_eq!(storage.scalars[parent_banks.scalars], u64::MAX);
            assert!(storage.initialized[parent_banks.scalars]);
            assert!(
                storage.scalars[child_banks.scalars..]
                    .iter()
                    .all(|bits| *bits == 0)
            );
            assert!(
                storage.initialized[child_banks.scalars..]
                    .iter()
                    .all(|initialized| !initialized)
            );
            assert!(
                slots
                    .get(ValueLocation {
                        execution,
                        bank: ValueBank::Managed,
                        slot: child_banks.managed
                    })?
                    .is_unit()
            );
            slots.retire_frame(reused)?;
            slots.retire_frame(parent)?;
            let result = slots.native_result(&native)?;
            assert!(slots.get(result)?.is_unit());
            slots.retire_native(native)?;
            Ok(())
        })
    })?;
    assert!(!memory.contains_frame_banks(parent_banks));
    assert!(memory.contains_frame_banks(other_banks));
    assert!(matches!(
        memory.inspect_value(value_location),
        Err(GcError::Index)
    ));
    assert_eq!(memory.collect(resources)?.retired_owners, 1);
    memory.retire_execution(resources, execution)?;
    assert!(matches!(
        memory.inspect_scalar(execution, parent_banks.scalars),
        Err(GcError::Index)
    ));
    assert_eq!(
        memory.inspect_scalar(independent, other_banks.scalars)?,
        Some(777)
    );
    memory.scope(resources, |cx, roots| {
        ExecutionRoots::with(roots, cx, |slots| {
            assert_eq!(
                slots.storage(independent)?.scalars[other_banks.scalars],
                777
            );
            Ok(())
        })
    })?;
    // Whole-execution retirement owns even parked frame suffixes. The inert
    // control token can then disappear without touching a peer or source again.
    memory.retire_execution(resources, independent)?;
    assert!(!memory.contains_frame_banks(other_banks));
    let _retired = other;
    assert_eq!(memory.stats().root_registrations, 0);
    assert_eq!(memory.stats().objects, 0);
    Ok(())
}
