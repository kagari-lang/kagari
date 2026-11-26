//! Stable tuple publication facts preserve Host value storage boundaries.
use crate::value::{aggregates::tuples::TupleObject, carriers::ScopedValue, kind::ObjectKind};
use kagari_gc::{access::Scope, error::GcError};

/// Tuple membership is immutable, so these facts never need invalidation. Other
/// aggregate stores keep their own checked write boundaries.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ValueProperties {
    pub(crate) storable: bool,
    /// Direct or tuple-nested path views cannot enter retained computations.
    /// Mutable descendants and active loans require a fresh graph walk.
    pub(crate) retainable: bool,
}

impl ValueProperties {
    const ORDINARY: Self = Self {
        storable: true,
        retainable: true,
    };

    pub(crate) fn of<'gc>(cx: &Scope<'gc>, value: ScopedValue<'gc>) -> Result<Self, GcError> {
        match value.kind() {
            Some(ObjectKind::Tuple) => {
                let object =
                    cx.downcast::<TupleObject>(value.object().ok_or(GcError::Representation)?)?;
                Ok(cx.read(object, TupleObject::properties))
            }
            Some(kind @ (ObjectKind::HostRoot | ObjectKind::HostPath)) => Ok(Self {
                storable: false,
                retainable: kind == ObjectKind::HostRoot,
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
            properties.retainable &= child.retainable;
        }
        Ok(properties)
    }
}
