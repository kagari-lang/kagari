//! Stable tuple publication facts preserve ephemeral and Host-loan boundaries.
use crate::value::{aggregates::tuples::TupleObject, carriers::ScopedValue, kind::ObjectKind};
use kagari_gc::{access::Scope, error::GcError};

/// Tuple membership is immutable, so these facts never need invalidation. Other
/// persistent objects admit only storable fields at their checked write boundary.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ValueProperties {
    pub(crate) storable: bool,
    pub(crate) ephemeral: bool,
    pub(crate) host_borrow: bool,
}

impl ValueProperties {
    const ORDINARY: Self = Self {
        storable: true,
        ephemeral: false,
        host_borrow: false,
    };

    pub(crate) fn of<'gc>(cx: &Scope<'gc>, value: ScopedValue<'gc>) -> Result<Self, GcError> {
        if value.as_runtime_ephemeral().is_some() {
            return Ok(Self {
                storable: false,
                ephemeral: true,
                host_borrow: false,
            });
        }
        match value.kind() {
            Some(ObjectKind::Tuple) => {
                let object =
                    cx.downcast::<TupleObject>(value.object().ok_or(GcError::Representation)?)?;
                Ok(cx.read(object, TupleObject::properties))
            }
            Some(ObjectKind::Ephemeral) => Ok(Self {
                storable: false,
                ephemeral: true,
                // Managed ephemeral descriptors are exclusively Host loans or
                // scoped Host projections. Runtime-only IDs stay inline above.
                host_borrow: true,
            }),
            Some(ObjectKind::HostRoot | ObjectKind::HostPath) => Ok(Self {
                storable: false,
                ..Self::ORDINARY
            }),
            _ => Ok(Self::ORDINARY),
        }
    }

    pub(crate) fn tuple<'gc>(
        cx: &Scope<'gc>,
        values: &[ScopedValue<'gc>],
    ) -> Result<Self, GcError> {
        let mut properties = Self::ORDINARY;
        for value in values {
            let child = Self::of(cx, *value)?;
            properties.storable &= child.storable;
            properties.ephemeral |= child.ephemeral;
            properties.host_borrow |= child.host_borrow;
        }
        Ok(properties)
    }
}
