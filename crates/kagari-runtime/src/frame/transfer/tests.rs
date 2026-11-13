use crate::{
    execution::{
        memory::{GcHeapConfig, ManagedMemory},
        roots::{ExecutionRoots, ValueBank, ValueLocation, pending::PendingSlots},
    },
    frame::{transfer::ReturnValue, values::operands::write_operand},
    module::execution::{layout::Location, operands::OperandSlot},
    resource::ResourceState,
    value::{carriers::ScopedValue, strings::StringObject},
};
use kagari_abi::representation::ValueType;
use kagari_gc::error::GcError;
use std::mem::size_of;

#[test]
fn async_completion_transfers_pending_cells_to_activation_snapshot() -> Result<(), GcError> {
    let resources = &ResourceState::default();
    let mut memory = ManagedMemory::new(GcHeapConfig::default());
    let execution = memory.enter_execution(resources)?;
    let pending = PendingSlots::new(execution);
    let packet = memory.scope(resources, |cx, roots| {
        ExecutionRoots::with(roots, cx, |slots| {
            let awaitable = StringObject::alloc(slots.scope(), "awaitable".into())?;
            let result = StringObject::alloc(slots.scope(), "completed result".into())?;
            slots.set(pending.awaitable(), ScopedValue::managed(awaitable))?;
            slots.set(pending.output(), ScopedValue::managed(result))?;
            let result = slots.get(pending.output())?;
            let packet = ReturnValue::protect(slots, execution, result)?;
            pending.clear(slots)?;
            Ok(packet)
        })
    })?;
    memory.collect(resources)?;
    assert_eq!(memory.stats().objects, 1);
    assert_eq!(memory.stats().root_registrations, 0);
    memory.scope(resources, |cx, roots| {
        ExecutionRoots::with(roots, cx, |slots| {
            assert!(slots.get(pending.awaitable())?.is_unit());
            assert!(slots.get(pending.output())?.is_unit());
            // Reusing a wait cell cannot overwrite an already returned packet.
            let later = StringObject::alloc(slots.scope(), "later wait".into())?;
            slots.set(pending.output(), ScopedValue::managed(later))?;
            let value = packet.read(slots, execution)?;
            StringObject::read(slots.scope(), value, |text| {
                assert_eq!(text, "completed result")
            })?;
            Ok(())
        })
    })?;
    memory.retire_execution(resources, execution)?;
    assert_eq!(memory.collect(resources)?.retired_owners, 2);
    let next = memory.enter_execution(resources)?;
    assert_ne!(next, execution);
    memory.scope(resources, |cx, roots| {
        ExecutionRoots::with(roots, cx, |slots| {
            assert!(matches!(slots.get(pending.output()), Err(GcError::Index)));
            assert!(matches!(
                packet.read(slots, next),
                Err(GcError::Representation)
            ));
            Ok(())
        })
    })?;
    memory.retire_execution(resources, next)?;
    Ok(())
}

#[test]
fn return_snapshots_survive_slot_changes_and_reject_frame_reuse() -> Result<(), GcError> {
    let resources = &ResourceState::default();
    let mut memory = ManagedMemory::new(GcHeapConfig::default());
    let execution = memory.enter_execution(resources)?;
    let managed = Location {
        operand: OperandSlot::new(0, true),
        representation: ValueType::Str,
        semantic: None,
        borrowed_receiver: false,
    };
    let scalar = Location {
        operand: OperandSlot::new(0, false),
        representation: ValueType::U64,
        semantic: None,
        borrowed_receiver: false,
    };
    let (caller, frame, packet) = memory.scope(resources, |cx, roots| {
        ExecutionRoots::with(roots, cx, |slots| {
            let caller = slots.enter_frame(execution, 1, 1, || Ok::<_, GcError>(()))?;
            write_operand(slots, caller.banks(), scalar, ScopedValue::from(42_u64))?;
            let frame = slots.enter_frame(execution, 1, 0, || Ok::<_, GcError>(()))?;
            let scalar = ReturnValue::protect(slots, execution, ScopedValue::from(u64::MAX))?;
            assert_eq!(scalar.payload(), Some((ValueType::U64, u64::MAX)));
            assert_eq!(
                scalar.read(slots, execution)?.scalar_payload(),
                scalar.payload()
            );
            let retained_scalar = scalar.into_retained(slots, execution)?;
            assert_eq!(
                retained_scalar.get(slots.scope())?.scalar_payload(),
                Some((ValueType::U64, u64::MAX))
            );
            assert_eq!(slots.len(execution, ValueBank::Local)?, 0);
            let old = StringObject::alloc(slots.scope(), "returned".into())?;
            let location = ValueLocation {
                execution,
                bank: ValueBank::Managed,
                slot: frame.banks().managed,
            };
            slots.set(location, ScopedValue::managed(old))?;
            let packet = ReturnValue::protect(slots, execution, ScopedValue::managed(old))?;
            let replacement = StringObject::alloc(slots.scope(), "overwritten slot".into())?;
            slots.set(location, ScopedValue::managed(replacement))?;
            Ok((caller, frame, packet))
        })
    })?;
    assert_eq!(memory.stats().root_registrations, 0);
    memory.collect(resources)?;
    assert_eq!(memory.stats().objects, 2);
    let parent_packet = memory.scope(resources, |cx, roots| ExecutionRoots::with(roots, cx, |slots| {
        let value = packet.read(slots, execution)?;
        let object = slots.scope().downcast::<StringObject>(value.object().unwrap())?;
        assert!(slots.scope().read(object, |text| text.as_str() == "returned"));
        let view = packet.view(slots, execution)?;
        assert!(view.get(slots)?.object().unwrap().ptr_eq(object.erase()));
        let independent = slots.enter(0)?;
        let other = slots.enter_frame(independent, 0, 0, || Ok::<_, GcError>(()))?;
        assert!(matches!(packet.read(slots, independent), Err(GcError::Representation)), "matching depths in another execution do not admit this control packet");
        slots.retire_frame(other)?;
        slots.leave(independent)?;
        let child = slots.enter_frame(execution, 0, 0, || Ok::<_, GcError>(()))?;
        assert!(matches!(packet.read(slots, execution), Err(GcError::Storage("return belongs to another current frame"))));
        slots.retire_frame(child)?;
        assert!(packet.read(slots, execution)?.object().unwrap().ptr_eq(object.erase()));
        // Failed destination admission neither changes the caller nor expires
        // the callee snapshot. In particular, a managed return cannot overwrite
        // the caller's scalar bank or publish into a nonadjacent frame suffix.
        assert!(matches!(packet.publish_to_caller(slots, &frame, caller.banks(), Some(scalar)), Err(GcError::Representation)));
        assert_eq!(slots.storage(execution)?.scalars[caller.banks().scalars], 42);
        assert!(slots.storage(execution)?.initialized[caller.banks().scalars]);
        let mut wrong = caller.banks();
        wrong.managed_count = 0;
        assert!(matches!(packet.publish_to_caller(slots, &frame, wrong, Some(managed)), Err(GcError::Storage("return destination is not the preceding caller"))));
        slots.check_frame_exit(&frame)?;
        packet.publish_to_caller(slots, &frame, caller.banks(), Some(managed))?;
        slots.retire_frame(frame)?;
        assert!(matches!(packet.read(slots, execution), Err(GcError::Index)));
        let mut frame = slots.enter_frame(execution, 0, 0, || Ok::<_, GcError>(()))?;
        let new = StringObject::alloc(slots.scope(), "another return".into())?;
        let another = ReturnValue::protect(slots, execution, ScopedValue::managed(new))?;
        assert!(matches!(packet.read(slots, execution), Err(GcError::Index)), "a new return at the same depth/local/record index must not authenticate the old packet");
        assert!(another.read(slots, execution)?.object().unwrap().ptr_eq(new.erase()));
        let parent_packet = another.reparent(slots, &mut frame)?;
        slots.retire_frame(frame)?;
        Ok(parent_packet)
    }))?;
    // An ordinary result now belongs to the caller operand; a local-stack
    // reentry result belongs to its caller's local prefix. Both survive without
    // a Host root after their original callee snapshots and slots disappear.
    assert_eq!(memory.collect(resources)?.retired_owners, 1);
    assert_eq!(memory.stats().objects, 2);
    memory.scope(resources, |cx, roots| {
        ExecutionRoots::with(roots, cx, |slots| {
            let value = slots.get(ValueLocation {
                execution,
                bank: ValueBank::Managed,
                slot: caller.banks().managed,
            })?;
            let text = slots
                .scope()
                .downcast::<StringObject>(value.object().unwrap())?;
            assert!(slots.scope().read(text, |text| text.as_str() == "returned"));
            assert_eq!(
                slots.storage(execution)?.scalars[caller.banks().scalars],
                42
            );
            assert!(slots.storage(execution)?.initialized[caller.banks().scalars]);
            let reentry = parent_packet.read(slots, execution)?;
            let text = slots
                .scope()
                .downcast::<StringObject>(reentry.object().unwrap())?;
            assert!(
                slots
                    .scope()
                    .read(text, |text| text.as_str() == "another return")
            );
            slots.retire_frame(caller)?;
            assert!(matches!(
                parent_packet.read(slots, execution),
                Err(GcError::Index)
            ));
            Ok(())
        })
    })?;
    assert_eq!(memory.collect(resources)?.retired_owners, 2);
    assert_eq!(memory.stats().objects, 0);
    assert_eq!(memory.stats().root_registrations, 0);
    memory.retire_execution(resources, execution)?;
    eprintln!("return packet bytes: {}", size_of::<ReturnValue>());
    Ok(())
}

#[test]
fn explicitly_retained_return_keeps_host_ownership_and_rejects_a_foreign_heap()
-> Result<(), GcError> {
    let resources = &ResourceState::default();
    let mut memory = ManagedMemory::new(GcHeapConfig::default());
    let execution = memory.enter_execution(resources)?;
    let packet = memory.scope(resources, |cx, roots| {
        ExecutionRoots::with(roots, cx, |slots| {
            let mut frame = slots.enter_frame(execution, 0, 0, || Ok::<_, GcError>(()))?;
            let text = StringObject::alloc(slots.scope(), "Host result".into())?;
            let packet = ReturnValue::protect(slots, execution, ScopedValue::managed(text))?;
            let packet = packet.reparent(slots, &mut frame)?;
            slots.retire_frame(frame)?;
            Ok(packet)
        })
    })?;
    assert_eq!(
        memory.stats().root_registrations,
        0,
        "finishing the last script frame does not choose Host ownership"
    );
    memory.collect(resources)?;
    assert_eq!(memory.stats().objects, 1);
    let root = memory.scope(resources, |cx, roots| {
        ExecutionRoots::with(roots, cx, |slots| packet.into_retained(slots, execution))
    })?;
    memory.retire_execution(resources, execution)?;
    memory.collect(resources)?;
    assert_eq!(
        memory.stats().objects,
        1,
        "only the explicit Host lease survives activation retirement"
    );
    let execution = memory.enter_execution(resources)?;
    let packet = ReturnValue::retained(root);
    assert_eq!(memory.stats().root_registrations, 1);
    let foreign_resources = &ResourceState::default();
    let mut foreign = ManagedMemory::new(GcHeapConfig::default());
    assert!(matches!(
        foreign.scope(foreign_resources, |cx, roots| ExecutionRoots::with(
            roots,
            cx,
            |slots| packet.read(slots, execution).map(|_| ())
        )),
        Err(GcError::ForeignHeap)
    ));
    let root = memory.scope(resources, |cx, roots| {
        ExecutionRoots::with(roots, cx, |slots| {
            let frame = slots.enter_frame(execution, 0, 0, || Ok::<_, GcError>(()))?;
            let view = packet.view(slots, execution)?;
            let value = view.get(slots)?;
            let object = slots
                .scope()
                .downcast::<StringObject>(value.object().unwrap())?;
            assert!(
                slots
                    .scope()
                    .read(object, |text| text.as_str() == "Host result")
            );
            slots.retire_frame(frame)?;
            packet.into_retained(slots, execution)
        })
    })?;
    memory.collect(resources)?;
    assert_eq!(memory.stats().objects, 1);
    assert_eq!(
        memory.stats().root_registrations,
        1,
        "moving a retained result must reuse its existing lease"
    );
    assert!(matches!(
        foreign.scope(foreign_resources, |cx, roots| ExecutionRoots::with(
            roots,
            cx,
            |slots| ReturnValue::retained(root.clone()).into_retained(slots, execution)
        )),
        Err(GcError::ForeignHeap)
    ));
    drop(root);
    // The released root entry and its string payload are separate retired owners.
    assert_eq!(memory.collect(resources)?.retired_owners, 2);
    assert_eq!(memory.stats().objects, 0);
    assert_eq!(memory.stats().root_registrations, 0);
    memory.retire_execution(resources, execution)
}

#[test]
fn reentry_returns_expire_with_native_and_conversion_owners() -> Result<(), GcError> {
    let resources = &ResourceState::default();
    let mut memory = ManagedMemory::new(GcHeapConfig::default());
    let execution = memory.enter_execution(resources)?;
    let (native, locals, packet) = memory.scope(resources, |cx, roots| {
        ExecutionRoots::with(roots, cx, |slots| {
            let native = slots.enter_native(execution)?;
            let locals = slots.mark_locals(execution)?;
            let mut frame = slots.enter_frame(execution, 0, 0, || Ok::<_, GcError>(()))?;
            let text = StringObject::alloc(slots.scope(), "Native reentry".into())?;
            let packet = ReturnValue::protect(slots, execution, ScopedValue::managed(text))?;
            let packet = packet.reparent(slots, &mut frame)?;
            slots.retire_frame(frame)?;
            Ok((native, locals, packet))
        })
    })?;
    memory.collect(resources)?;
    assert_eq!(memory.stats().objects, 1);
    assert_eq!(memory.stats().root_registrations, 0);
    let replacement = memory.scope(resources, |cx, roots| {
        ExecutionRoots::with(roots, cx, |slots| {
            let value = packet.read(slots, execution)?;
            let text = slots
                .scope()
                .downcast::<StringObject>(value.object().unwrap())?;
            assert!(
                slots
                    .scope()
                    .read(text, |text| text.as_str() == "Native reentry")
            );
            slots.retire_locals(locals)?;
            assert!(matches!(packet.read(slots, execution), Err(GcError::Index)));
            let mut preservation = slots.mark_locals(execution)?;
            let mut frame = slots.enter_frame(execution, 0, 0, || Ok::<_, GcError>(()))?;
            let text = StringObject::alloc(slots.scope(), "reused local and record".into())?;
            let replacement = ReturnValue::protect(slots, execution, ScopedValue::managed(text))?;
            let replacement = replacement.reparent(slots, &mut frame)?;
            slots.retire_frame(frame)?;
            assert!(matches!(packet.read(slots, execution), Err(GcError::Index)));
            assert!(
                replacement
                    .read(slots, execution)?
                    .object()
                    .unwrap()
                    .ptr_eq(text.erase())
            );
            let value = replacement.read(slots, execution)?;
            let preserved = slots.preserve_local(&mut preservation, value)?;
            assert!(
                matches!(replacement.read(slots, execution), Err(GcError::Index)),
                "preserving a conversion result expires preparation return records"
            );
            slots.retire_locals(preservation)?;
            assert!(slots.get(preserved)?.object().unwrap().ptr_eq(text.erase()));
            slots.retire_native(native)?;
            assert!(matches!(
                replacement.read(slots, execution),
                Err(GcError::Index)
            ));
            // Reusing the exact Native/local/record coordinates cannot revive either
            // packet. The identity counter remains monotonic across suffix cleanup.
            let native = slots.enter_native(execution)?;
            let mut frame = slots.enter_frame(execution, 0, 0, || Ok::<_, GcError>(()))?;
            let next = ReturnValue::protect(slots, execution, ScopedValue::managed(text))?;
            let next = next.reparent(slots, &mut frame)?;
            slots.retire_frame(frame)?;
            assert!(matches!(
                replacement.read(slots, execution),
                Err(GcError::Index)
            ));
            slots.retire_native(native)?;
            assert!(matches!(next.read(slots, execution), Err(GcError::Index)));
            Ok(replacement)
        })
    })?;
    drop(replacement);
    assert_eq!(memory.collect(resources)?.retired_owners, 2);
    assert_eq!(memory.stats().objects, 0);
    assert_eq!(memory.stats().root_registrations, 0);
    memory.retire_execution(resources, execution)
}
