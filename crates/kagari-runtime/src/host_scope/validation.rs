//! Source protection validates revocable Host capabilities without retaining them.
use crate::{
    error::RuntimeError,
    execution::roots::Slots,
    host::{HostBorrowKind, HostBorrowTable, HostRegistry},
    native::binding::NativeResult,
    resource::ResourceState,
    value::{
        EphemeralValue,
        aggregates::tuples::TupleObject,
        carriers::ScopedValue,
        host_values::{EphemeralObject, HostRootObject, paths::HostPathObject},
        kind::ObjectKind,
        properties::ValueProperties,
        view::ValueView,
    },
};
use kagari_gc::{access::Scope, error::GcError};

pub(super) fn values(
    slots: &mut Slots<'_, '_>,
    values: &[ValueView<'_>],
    registry: &HostRegistry,
    borrows: &HostBorrowTable,
    resources: &ResourceState,
) -> Result<NativeResult<()>, GcError> {
    for (index, value) in values.iter().enumerate() {
        if index != 0
            && index.is_multiple_of(1024)
            && let Err(error) = resources.poll_closed()
        {
            return Ok(Err(error));
        }
        let value = value.get(slots)?;
        if let Err(error) = validate(slots.scope(), value, registry, borrows, resources)? {
            return Ok(Err(error));
        }
    }
    Ok(Ok(()))
}

/// Match the existing Host boundary: nested tuple carriers and direct external
/// descriptors. Persistent object stores own their separate admission contracts;
/// async graph safety is a different traversal, never approximated here.
pub(super) fn validate<'gc>(
    cx: &Scope<'gc>,
    value: ScopedValue<'gc>,
    registry: &HostRegistry,
    borrows: &HostBorrowTable,
    resources: &ResourceState,
) -> Result<NativeResult<()>, GcError> {
    let mut current = Some(value);
    let mut pending = Vec::new();
    let mut inspected = 0usize;
    while let Some(value) = current {
        inspected += 1;
        if inspected == 1024 {
            inspected = 0;
            if let Err(error) = resources.poll_closed() {
                return Ok(Err(error));
            }
        }
        match value.kind() {
            Some(ObjectKind::Tuple) if !ValueProperties::of(cx, value)?.storable => {
                let object =
                    cx.downcast::<TupleObject>(value.object().ok_or(GcError::Representation)?)?;
                let count = cx.read(object, TupleObject::len);
                pending.try_reserve(count)?;
                for index in (0..count).rev() {
                    if index.is_multiple_of(1024)
                        && index != 0
                        && let Err(error) = resources.poll_closed()
                    {
                        return Ok(Err(error));
                    }
                    pending.push(TupleObject::get(cx, object, index)?);
                }
            }
            Some(ObjectKind::HostRoot) => {
                let object =
                    cx.downcast::<HostRootObject>(value.object().ok_or(GcError::Representation)?)?;
                if !registry.matches_root(cx.read(object, HostRootObject::handle)) {
                    return Ok(Err(RuntimeError::host_call_failure(
                        "host root belongs to another registry or is not registered",
                    )));
                }
            }
            Some(ObjectKind::HostPath) => {
                let object =
                    cx.downcast::<HostPathObject>(value.object().ok_or(GcError::Representation)?)?;
                if let Err(error) = registry.validate_path_scoped(cx, object, resources)? {
                    return Ok(Err(error));
                }
            }
            Some(ObjectKind::Ephemeral) if value.as_runtime_ephemeral().is_none() => {
                let object =
                    cx.downcast::<EphemeralObject>(value.object().ok_or(GcError::Representation)?)?;
                let result = match cx.read(object, EphemeralObject::value) {
                    EphemeralValue::HostRef(token) => {
                        borrows.validate(token, HostBorrowKind::Shared)
                    }
                    EphemeralValue::HostMut(token) => {
                        borrows.validate(token, HostBorrowKind::Unique)
                    }
                    EphemeralValue::HostView(view) => borrows.validate_scoped(view),
                    EphemeralValue::Runtime(_) => Ok(()),
                };
                if let Err(error) = result {
                    return Ok(Err(error));
                }
            }
            _ => {}
        }
        current = pending.pop();
    }
    Ok(Ok(()))
}

/// A successful scoped return or explicit Host escape cannot retain a borrowed
/// Host/Runtime temporary, including one nested in an immutable tuple.
pub(super) fn result<'gc>(
    cx: &Scope<'gc>,
    value: ScopedValue<'gc>,
    registry: &HostRegistry,
    borrows: &HostBorrowTable,
    resources: &ResourceState,
) -> Result<NativeResult<()>, GcError> {
    let properties = ValueProperties::of(cx, value)?;
    if properties.host_borrow || properties.ephemeral {
        return Ok(Err(RuntimeError::host_borrow_escape(
            "scoped execution result cannot retain an ephemeral value",
        )));
    }
    validate(cx, value, registry, borrows, resources)
}
