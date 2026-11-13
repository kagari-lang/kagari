use crate::{
    execution::{
        memory::{GcHeapConfig, ManagedMemory},
        roots::{ExecutionRoots, ValueBank},
    },
    frame::values::operands::{read_operand, write_operand},
    module::execution::{layout::Location, operands::OperandSlot},
    resource::ResourceState,
    value::carriers::ScopedValue,
};
use kagari_abi::representation::ValueType;
use kagari_gc::error::GcError;
use kagari_types::scalar::BuiltinType;

#[test]
fn unit_operands_neither_require_nor_alias_a_payload_cell() -> Result<(), GcError> {
    let resources = &ResourceState::default();
    let mut memory = ManagedMemory::new(GcHeapConfig::default());
    let execution = memory.enter_execution(resources)?;
    let unit = Location {
        operand: OperandSlot::unit(),
        representation: ValueType::Unit,
        semantic: Some(BuiltinType::Unit),
        borrowed_receiver: false,
    };
    let retired_banks = memory.scope(resources, |cx, roots| {
        ExecutionRoots::with(roots, cx, |slots| {
            let empty = slots.enter_frame(execution, 0, 0, || Ok::<_, GcError>(()))?;
            assert!(read_operand(slots, empty.banks(), unit)?.is_unit());
            write_operand(slots, empty.banks(), unit, ScopedValue::unit())?;
            assert!(matches!(
                write_operand(slots, empty.banks(), unit, ScopedValue::from(0_u64)),
                Err(GcError::Representation)
            ));
            assert_eq!(slots.len(execution, ValueBank::Managed)?, 0);
            assert!(slots.storage(execution)?.scalars.is_empty());
            assert!(slots.storage(execution)?.initialized.is_empty());

            let child = slots.enter_frame(execution, 0, 1, || Ok::<_, GcError>(()))?;
            let scalar = Location {
                operand: OperandSlot::new(0, false),
                representation: ValueType::U64,
                semantic: Some(BuiltinType::U64),
                borrowed_receiver: false,
            };
            assert!(read_operand(slots, child.banks(), scalar)?.is_unit());
            write_operand(slots, child.banks(), unit, ScopedValue::unit())?;
            assert!(!slots.storage(execution)?.initialized[0]);
            write_operand(slots, child.banks(), scalar, ScopedValue::from(u64::MAX))?;
            write_operand(slots, child.banks(), unit, ScopedValue::unit())?;
            assert_eq!(
                read_operand(slots, child.banks(), scalar)?.scalar_payload(),
                Some((ValueType::U64, u64::MAX))
            );
            slots.retire_frame(child)?;
            let banks = empty.banks();
            slots.retire_frame(empty)?;
            Ok(banks)
        })
    })?;
    memory.retire_execution(resources, execution)?;
    let replacement = memory.enter_execution(resources)?;
    memory.scope(resources, |cx, roots| {
        ExecutionRoots::with(roots, cx, |slots| {
            // Unit needs no payload, but still requires the complete execution
            // identity. Reusing an empty directory entry cannot admit this old view.
            assert!(matches!(
                read_operand(slots, retired_banks, unit),
                Err(GcError::Index)
            ));
            Ok(())
        })
    })?;
    memory.retire_execution(resources, replacement)?;
    assert_eq!(memory.stats().root_registrations, 0);
    Ok(())
}
