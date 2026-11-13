use crate::{
    execution::{
        memory::{GcHeapConfig, ManagedMemory},
        roots::{ExecutionRoots, ValueBank},
    },
    resource::ResourceState,
    value::{carriers::ScopedValue, strings::StringObject},
};
use kagari_gc::error::GcError;

#[test]
fn adopted_call_preparation_retires_with_the_callee_and_preserves_its_caller() -> Result<(), GcError>
{
    let resources = &ResourceState::default();
    let mut memory = ManagedMemory::new(GcHeapConfig::default());
    let execution = memory.enter_execution(resources)?;
    let (caller, callee, caller_value, metadata) = memory.scope(resources, |cx, roots| {
        ExecutionRoots::with(roots, cx, |slots| {
            let caller = slots.enter_frame(execution, 0, 0, || Ok::<_, GcError>(()))?;
            let value = StringObject::alloc(slots.scope(), "caller".into())?;
            let caller_value = slots.push_local(execution, ScopedValue::managed(value))?;
            let mut preparation = Some(slots.mark_locals(execution)?);
            let scratch = StringObject::alloc(slots.scope(), "callee preparation".into())?;
            slots.push_local(execution, ScopedValue::managed(scratch))?;
            let mut conversion = slots.mark_locals(execution)?;
            // A typed source dependency is collector ownership, not a language tag.
            let metadata = StringObject::alloc(slots.scope(), "selected metadata".into())?;
            slots.push_dependency(execution, metadata)?;
            let preserved = slots.preserve_dependency(&mut conversion, metadata)?;
            slots.retire_locals(conversion)?;
            let mut callee = slots.enter_frame(execution, 0, 0, || Ok::<_, GcError>(()))?;
            let mut wrong = Some(slots.mark_locals(execution)?);
            assert!(matches!(
                slots.adopt_frame_preparation(&mut callee, &mut wrong),
                Err(GcError::Storage(_))
            ));
            slots.retire_locals(wrong.take().unwrap())?;
            slots.adopt_frame_preparation(&mut callee, &mut preparation)?;
            assert!(preparation.is_none());
            assert!(matches!(
                slots.adopt_frame_preparation(&mut callee, &mut preparation),
                Err(GcError::Storage(_))
            ));
            Ok((caller, callee, caller_value, preserved))
        })
    })?;
    memory.collect(resources)?;
    assert_eq!(memory.stats().objects, 3);
    assert_eq!(memory.stats().root_registrations, 0);
    memory.scope(resources, |cx, roots| {
        ExecutionRoots::with(roots, cx, |slots| {
            let selected = slots.dependency(metadata)?;
            StringObject::read(slots.scope(), ScopedValue::managed(selected), |text| {
                assert_eq!(text, "selected metadata")
            })?;
            slots.check_frame_exit(&callee)?;
            // The entire adopted prefix dies in the one ordinary frame transition.
            slots.retire_frame(callee)?;
            assert!(matches!(slots.dependency(metadata), Err(GcError::Index)));
            assert_eq!(slots.len(execution, ValueBank::Local)?, 1);
            Ok(())
        })
    })?;
    memory.collect(resources)?;
    assert_eq!(memory.stats().objects, 1);
    memory.scope(resources, |cx, roots| {
        ExecutionRoots::with(roots, cx, |slots| {
            let value = slots.get(caller_value)?;
            StringObject::read(slots.scope(), value, |text| assert_eq!(text, "caller"))?;
            slots.retire_frame(caller)
        })
    })?;
    memory.collect(resources)?;
    assert_eq!(memory.stats().objects, 0);
    assert_eq!(memory.stats().root_registrations, 0);
    memory.retire_execution(resources, execution)
}
