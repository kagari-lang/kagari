//! One physical field operation serves ready and newly prepared layouts.
use crate::{
    error::{RuntimeError, fault::EngineFault, trap::TrapReason},
    frame::{
        cursor::kernel::RegionError, types::compatibility::TypeView,
        values::operands::OperandWindow,
    },
    host::{HostBorrowTable, HostRegistry},
    module::{
        StructLayoutRef,
        execution::fields::{FieldAccess, PreparedFieldOperation},
    },
    resource::ResourceState,
    value::{
        aggregates::structures::StructObject,
        carriers::ScopedValue,
        kind::ObjectKind,
        properties::ValueProperties,
        scalar::{self, domain},
    },
    value_check::{data::candidate_access, scoped},
};
use kagari_contract::representation::builtin_representation;
use kagari_gc::error::GcError;
use kagari_types::{scalar::BuiltinType, ty::Ty};

pub(in crate::frame) fn execute(
    values: &mut OperandWindow<'_, '_, '_>,
    operation: &PreparedFieldOperation,
    layout: &StructLayoutRef,
    host: &HostRegistry,
    host_borrows: &HostBorrowTable,
    resources: &ResourceState,
) -> Result<(), RegionError> {
    let input = admit_input(values, operation)?;
    let base = values
        .read_location(operation.base)
        .map_err(RegionError::storage)?;
    if base.kind() != Some(ObjectKind::Struct) {
        return Err(RegionError::TypeMismatch(match operation.access {
            FieldAccess::Read { .. } => "read_field expects struct value",
            FieldAccess::Write { .. } => "write_field expects struct value",
        }));
    }
    let cx = values.scope();
    if let Some(value) = input {
        candidate_access(cx, value, resources).map_err(RegionError::from)?;
    }
    candidate_access(cx, base, resources).map_err(RegionError::from)?;
    let object = cx
        .downcast::<StructObject>(
            base.object()
                .ok_or(GcError::Representation)
                .map_err(RegionError::storage)?,
        )
        .map_err(RegionError::storage)?;
    let slot = operation.slot as usize;
    if let Some(ty) = operation.scalar {
        match operation.access {
            FieldAccess::Read { dst } => {
                let bits =
                    StructObject::scalar(cx, object, layout, slot, ty).map_err(field_read_error)?;
                let value = scalar::decode(dst.representation, bits).ok_or_else(invalid)?;
                values
                    .write_location(dst, value)
                    .map_err(RegionError::storage)?;
            }
            FieldAccess::Write { .. } => {
                let bits = input
                    .and_then(|value| value.scalar_payload())
                    .expect("admitted scalar field input")
                    .1;
                let field = layout
                    .layout()
                    .fields
                    .get(slot)
                    .ok_or_else(|| RuntimeError::from(TrapReason::InvalidFieldSlot))?;
                if !field.mutable || field.ty != Ty::Builtin(ty) {
                    return Err(RuntimeError::from(
                        TrapReason::FieldIsReadOnlyOrValueHasTheWrongConcreteType,
                    )
                    .into());
                }
                if !cx.read(object, |object| object.layout().matches(layout)) {
                    return Err(RuntimeError::from(TrapReason::StructLayoutMismatch).into());
                }
                StructObject::set_scalar(cx, object, layout, slot, ty, bits)
                    .map_err(field_write_error)?;
            }
        }
        return Ok(());
    }
    match operation.access {
        FieldAccess::Read { dst } => {
            if !cx.read(object, |object| object.layout().matches(layout)) {
                return Err(RegionError::TypeMismatch("struct layout or field mismatch"));
            }
            let value = StructObject::get(cx, object, slot).map_err(field_read_error)?;
            values
                .write_location(dst, value)
                .map_err(RegionError::storage)?;
        }
        FieldAccess::Write { .. } => {
            let value = input.ok_or_else(invalid)?;
            let field = layout
                .layout()
                .fields
                .get(slot)
                .ok_or_else(|| RuntimeError::from(TrapReason::InvalidFieldSlot))?;
            let (ty, environment) = layout.field_type(slot).ok_or_else(invalid)?;
            if !field.mutable
                || !scoped::matches(
                    cx,
                    value,
                    TypeView::new(ty, layout.module(), environment),
                    host,
                    host_borrows,
                    resources,
                )
                .map_err(RegionError::from)?
            {
                return Err(RuntimeError::from(
                    TrapReason::FieldIsReadOnlyOrValueHasTheWrongConcreteType,
                )
                .into());
            }
            if !cx.read(object, |object| object.layout().matches(layout)) {
                return Err(RuntimeError::from(TrapReason::StructLayoutMismatch).into());
            }
            StructObject::set(cx, object, layout, slot, value).map_err(field_write_error)?;
        }
    }
    Ok(())
}

/// Admit the original write input before fallible layout/receiver work. Cold
/// preparation repeats only this closed input read after resolving the layout.
pub(in crate::frame) fn admit_input<'gc>(
    values: &mut OperandWindow<'_, '_, 'gc>,
    operation: &PreparedFieldOperation,
) -> Result<Option<ScopedValue<'gc>>, RegionError> {
    let input = match operation.access {
        FieldAccess::Read { .. } => None,
        FieldAccess::Write { value } => {
            Some(values.read_location(value).map_err(RegionError::storage)?)
        }
    };
    if operation.scalar.is_none()
        && let Some(value) = input
        && !ValueProperties::of(values.scope(), value)
            .map_err(RegionError::storage)?
            .storable
    {
        return Err(RegionError::TypeMismatch(
            "write_field expects default-storable value",
        ));
    }
    if let (Some(value), Some(ty)) = (input, operation.scalar) {
        scalar_input(value, ty)?;
    }
    Ok(input)
}

fn scalar_input(value: ScopedValue<'_>, ty: BuiltinType) -> Result<u64, RegionError> {
    let invalid = || RuntimeError::from(TrapReason::InvalidHeapPayload);
    let (representation, bits) = value.scalar_payload().ok_or_else(invalid)?;
    if representation != builtin_representation(ty)
        || !domain::admits_payload(representation, Some(ty), bits)
    {
        return Err(invalid().into());
    }
    Ok(bits)
}

fn invalid() -> RegionError {
    RuntimeError::from(EngineFault::InvalidExecutionOperandSlot).into()
}

fn field_read_error(error: GcError) -> RegionError {
    match error {
        GcError::Index | GcError::Representation => {
            RegionError::TypeMismatch("struct layout or field mismatch")
        }
        error => RegionError::storage(error),
    }
}

fn field_write_error(error: GcError) -> RegionError {
    match error {
        GcError::Index | GcError::Representation => {
            RuntimeError::from(EngineFault::StructStorageDoesNotMatchItsLayout).into()
        }
        error => RegionError::storage(error),
    }
}

#[cfg(test)]
mod tests {
    use super::scalar_input;
    use crate::{
        error::{RuntimeError, trap::TrapReason},
        frame::cursor::kernel::RegionError,
        value::carriers::ScopedValue,
    };
    use kagari_types::scalar::BuiltinType;

    #[test]
    fn scalar_field_inputs_preserve_carrier_and_domain_before_erasing_tags() {
        let expected = RuntimeError::from(TrapReason::InvalidHeapPayload);
        for input in [
            ScopedValue::unit(),
            ScopedValue::from(false),
            ScopedValue::from(0i64),
            ScopedValue::from(128i32),
        ] {
            let error = scalar_input(input, BuiltinType::I8).unwrap_err();
            assert!(matches!(error, RegionError::Runtime(error) if *error == expected));
        }
        assert_eq!(
            scalar_input(ScopedValue::from(-128i32), BuiltinType::I8).unwrap(),
            (-128i64) as u64
        );
        assert_eq!(
            scalar_input(ScopedValue::from(u64::MAX), BuiltinType::U64).unwrap(),
            u64::MAX
        );
    }
}
