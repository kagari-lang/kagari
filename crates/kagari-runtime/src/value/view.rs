//! Call-lifetime views name protected source locations across short GC scopes.
use crate::{
    execution::roots::{Slots, ValueLocation},
    value::{
        carriers::{Representation, ScopedValue},
        kind::ObjectKind,
    },
};
use kagari_abi::representation::ValueType;
use kagari_gc::error::GcError;
use std::marker::PhantomData;

/// A conversion/call view into already protected storage. Scalars are inline.
/// Managed views never own a payload pointer or a Host root; retaining a value
/// beyond its context requires the context's explicit owning conversion.
#[derive(Debug, Clone, Copy)]
pub struct ValueView<'call> {
    representation: Representation<ValueLocation>,
    lifetime: PhantomData<fn(&'call ()) -> &'call ()>,
}

impl<'call> ValueView<'call> {
    pub const fn unit() -> Self {
        Self {
            representation: Representation::Unit,
            lifetime: PhantomData,
        }
    }

    pub(super) fn from_representation(representation: Representation<ValueLocation>) -> Self {
        Self {
            representation,
            lifetime: PhantomData,
        }
    }

    /// The caller has installed this exact protected value at the location. Only
    /// Runtime's admitted conversion/argument owners choose the resulting lifetime.
    pub(crate) fn protected(
        value: ScopedValue<'_>,
        location: Option<ValueLocation>,
    ) -> Result<Self, GcError> {
        value
            .representation()
            .map_reference(|_| location.ok_or(GcError::Storage("managed view has no source")))
            .map(Self::from_representation)
    }

    pub(crate) fn location(self) -> Option<ValueLocation> {
        match self.representation {
            Representation::Managed { target, .. } => Some(target),
            _ => None,
        }
    }

    pub(crate) fn get<'gc>(self, slots: &mut Slots<'_, 'gc>) -> Result<ScopedValue<'gc>, GcError> {
        self.representation
            .map_reference(|location| {
                let value = slots.get(*location)?;
                if value.kind() != self.kind() {
                    return Err(GcError::Storage(
                        "protected view source changed representation",
                    ));
                }
                value.object().ok_or(GcError::Representation)
            })
            .map(ScopedValue::from_representation)
    }

    pub(crate) fn reborrow<'short>(self) -> ValueView<'short> {
        ValueView::from_representation(self.representation)
    }

    pub fn kind(self) -> Option<ObjectKind> {
        match self.representation {
            Representation::Managed { kind, .. } => Some(kind),
            Representation::RuntimeEphemeral(_) => Some(ObjectKind::Ephemeral),
            _ => None,
        }
    }

    pub fn is_unit(self) -> bool {
        matches!(self.representation, Representation::Unit)
    }

    /// Read only inline scalar bits, without dereferencing a managed source.
    pub(crate) fn scalar_payload(self) -> Option<(ValueType, u64)> {
        self.representation.scalar_payload()
    }

    pub(crate) fn has_representation(self, representation: ValueType) -> bool {
        self.representation.has_representation(representation)
    }
}

macro_rules! scalar_accessors {
    ($($name:ident: $ty:ty => $variant:ident),* $(,)?) => {$(
        impl ValueView<'_> {
            pub fn $name(self) -> Option<$ty> {
                self.representation.$name()
            }
        }
    )*};
}

scalar_accessors!(as_bool: bool => Bool, as_char: char => Char, as_i32: i32 => I32,
    as_i64: i64 => I64, as_u64: u64 => U64, as_f32: f32 => F32, as_f64: f64 => F64);
