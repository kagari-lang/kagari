use crate::{
    execution::{
        memory::{GcHeapConfig, ManagedMemory},
        roots::{ExecutionRoots, ValueBank, ValueLocation, queued::QueuedArguments},
    },
    native::conversion::arguments::ArgumentValues,
    resource::ResourceState,
    value::{carriers::ScopedValue, strings::StringObject},
};
use kagari_gc::error::GcError;

#[test]
fn queued_arguments_survive_conversion_and_retire_after_frame_publication() -> Result<(), GcError> {
    let resources = &ResourceState::default();
    let mut memory = ManagedMemory::new(GcHeapConfig::default());
    let execution = memory.enter_execution(resources)?;
    let queued = memory.scope(resources, |cx, roots| {
        ExecutionRoots::with(roots, cx, |slots| {
            let mark = slots.mark_locals(execution)?;
            let text = StringObject::alloc(slots.scope(), "queued value".into())?;
            let local = slots.push_local(execution, ScopedValue::managed(text))?;
            let queued = slots.begin_queue(execution, 1)?;
            let value = slots.get(local)?;
            slots.set_queued(&queued, 0, value)?;
            slots.retire_locals(mark)?;
            Ok(queued)
        })
    })?;
    memory.collect(resources)?;
    assert_eq!(memory.stats().objects, 1);
    assert_eq!(memory.stats().root_registrations, 0);
    let stale = QueuedArguments {
        execution,
        sequence: queued.sequence,
        count: 1,
    };
    let frame = memory.scope(resources, |cx, roots| {
        ExecutionRoots::with(roots, cx, |slots| {
            let arguments = ArgumentValues::queued(&queued);
            arguments.check_bounds(slots)?;
            let frame = slots.enter_frame(execution, 1, 0, || Ok::<_, GcError>(()))?;
            let value = arguments.scoped(slots, 0)?;
            slots.set(
                ValueLocation {
                    execution,
                    bank: ValueBank::Managed,
                    slot: frame.banks().managed,
                },
                value,
            )?;
            slots.retire_queue(queued)?;
            assert!(matches!(
                stale.check_bounds(slots),
                Err(GcError::Storage(_))
            ));
            assert!(matches!(
                slots.begin_queue(execution, 0),
                Err(GcError::Storage(_))
            ));
            Ok(frame)
        })
    })?;
    memory.collect(resources)?;
    assert_eq!(memory.stats().objects, 1);
    memory.scope(resources, |cx, roots| {
        ExecutionRoots::with(roots, cx, |slots| {
            let value = slots.get(ValueLocation {
                execution,
                bank: ValueBank::Managed,
                slot: frame.banks().managed,
            })?;
            StringObject::read(slots.scope(), value, |text| {
                assert_eq!(text, "queued value")
            })?;
            slots.retire_frame(frame)?;
            let next = slots.begin_queue(execution, 1)?;
            assert_ne!(next.sequence, stale.sequence);
            assert!(matches!(stale.get(slots, 0), Err(GcError::Storage(_))));
            slots.retire_queue(next)?;
            let empty = slots.begin_queue(execution, 0)?;
            ArgumentValues::queued(&empty).check_bounds(slots)?;
            assert!(matches!(
                slots.begin_queue(execution, 0),
                Err(GcError::Storage(_))
            ));
            slots.retire_queue(empty)?;
            slots.storage(execution)?.queue_sequence = u64::MAX;
            assert!(matches!(
                slots.begin_queue(execution, 0),
                Err(GcError::Limit("queued argument identity"))
            ));
            assert!(slots.storage(execution)?.queue.is_none());
            Ok(())
        })
    })?;
    assert_eq!(memory.collect(resources)?.retired_owners, 1);
    assert_eq!(memory.stats().objects, 0);
    assert_eq!(memory.stats().root_registrations, 0);
    memory.retire_execution(resources, execution)
}
