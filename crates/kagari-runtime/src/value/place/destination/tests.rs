use super::Destination;
use crate::{
    execution::{
        memory::{GcHeapConfig, ManagedMemory},
        roots::{ExecutionRoots, ValueBank, ValueLocation},
    },
    module::execution::{layout::Location, operands::OperandSlot},
    resource::ResourceState,
    value::{carriers::ScopedValue, strings::StringObject, view::ValueView},
};
use kagari_abi::representation::ValueType;
use kagari_gc::error::GcError;
use kagari_types::scalar::BuiltinType;

#[test]
fn native_publication_writes_caller_banks_before_suffix_retirement() -> Result<(), GcError> {
    let resources = &ResourceState::default();
    let mut memory = ManagedMemory::new(GcHeapConfig::default());
    let execution = memory.enter_execution(resources)?;
    let scalar = Location {
        operand: OperandSlot::new(0, false),
        representation: ValueType::I32,
        semantic: Some(BuiltinType::I32),
        borrowed_receiver: false,
    };
    let managed = Location {
        operand: OperandSlot::new(0, true),
        representation: ValueType::Str,
        semantic: Some(BuiltinType::String),
        borrowed_receiver: false,
    };
    let frame = memory.scope(resources, |cx, roots| {
        ExecutionRoots::with(roots, cx, |slots| {
            let frame = slots.enter_frame(execution, 1, 1, || Ok::<_, GcError>(()))?;
            let banks = frame.banks();
            let native = slots.enter_native(execution)?;
            let pending = slots.native_result(&native)?;
            let text =
                ScopedValue::managed(StringObject::alloc(slots.scope(), "result 🙂".into())?);
            slots.set(pending, text)?;
            let value = ValueView::protected(text, Some(pending))?;
            let destination = Destination::Frame(&banks, Some(&managed));
            destination.publish(slots, &value)?;
            assert!(destination.admits_frame(&banks, Some(&managed)));
            // Equivalent descriptor copies are not this call's publication proof.
            let copy = banks;
            assert!(!destination.admits_frame(&copy, Some(&managed)));
            assert!(!destination.admits_frame(&banks, None));
            assert!(!destination.admits_source(pending));
            Destination::Frame(&banks, None).publish(slots, &value)?;
            assert_eq!(slots.len(execution, ValueBank::Local)?, 1);
            slots.retire_native(native)?;
            assert_eq!(slots.len(execution, ValueBank::Local)?, 0);
            // Discard still authenticates the original source after retirement.
            assert!(matches!(
                Destination::Frame(&banks, None).publish(slots, &value),
                Err(GcError::Index)
            ));
            let destination = Destination::Frame(&banks, Some(&scalar));
            destination.publish(slots, &ValueView::from(42_i32))?;
            assert!(matches!(
                destination.publish(slots, &ValueView::from(42_i64)),
                Err(GcError::Representation)
            ));
            let replacement = ScopedValue::managed(StringObject::alloc(
                slots.scope(),
                "reflected result".into(),
            )?);
            Destination::Frame(&banks, Some(&managed)).publish_scoped(slots, replacement)?;
            assert_eq!(
                slots.len(execution, ValueBank::Local)?,
                0,
                "closed publication needs no scratch cell"
            );
            assert!(matches!(
                destination.publish_scoped(slots, ScopedValue::from(99i64)),
                Err(GcError::Representation)
            ));
            assert_eq!(slots.storage(execution)?.scalars[0], 42);
            assert!(slots.storage(execution)?.initialized[0]);
            Ok(frame)
        })
    })?;
    memory.collect(resources)?;
    assert_eq!(memory.stats().root_registrations, 0);
    assert_eq!(memory.stats().objects, 1);
    memory.scope(resources, |cx, roots| {
        ExecutionRoots::with(roots, cx, |slots| {
            let value = slots.get(ValueLocation {
                execution,
                bank: ValueBank::Managed,
                slot: frame.banks().managed,
            })?;
            let object = slots
                .scope()
                .downcast::<StringObject>(value.object().unwrap())?;
            assert_eq!(
                slots.scope().read(object, |text| text.as_str().to_owned()),
                "reflected result"
            );
            slots.retire_frame(frame)
        })
    })?;
    memory.collect(resources)?;
    assert_eq!(memory.stats().objects, 0);
    memory.retire_execution(resources, execution)?;
    Ok(())
}
