//! Stable tuple publication facts preserve Host value storage boundaries.
use crate::value::{aggregates::tuples::TupleObject, carriers::ScopedValue, kind::ObjectKind};
use kagari_gc::{access::Scope, error::GcError};

/// Tuple membership is immutable, so these facts never need invalidation. Other
/// persistent objects admit only storable fields at their checked write boundary.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ValueProperties {
    pub(crate) storable: bool,
}

impl ValueProperties {
    const ORDINARY: Self = Self { storable: true };

    pub(crate) fn of<'gc>(cx: &Scope<'gc>, value: ScopedValue<'gc>) -> Result<Self, GcError> {
        match value.kind() {
            Some(ObjectKind::Tuple) => {
                let object =
                    cx.downcast::<TupleObject>(value.object().ok_or(GcError::Representation)?)?;
                Ok(cx.read(object, TupleObject::properties))
            }
            Some(ObjectKind::HostRoot | ObjectKind::HostPath) => Ok(Self { storable: false }),
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
        }
        Ok(properties)
    }
}
