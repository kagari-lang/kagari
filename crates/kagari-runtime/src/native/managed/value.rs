//! One bounded representation shared by scoped values, stored edges and host roots.

use crate::{
    native::{
        binding::NativeResult,
        managed::{call::CallRegion, payload::NativeObject, slots::Kind},
    },
    value::scalar::ScalarValue,
};
use kagari_gc::{
    access::Mutation,
    error::GcError,
    object::{AnyObject, Edge, Gc, GcObject},
    root::Root,
    scan::{EdgeVisitor, ScanBudget, ScanStatus},
};
use std::{cell::Cell, marker::PhantomData, sync::Arc};

/// Empty/Absent describe working-slot state, not the language enum ABI.
#[derive(Clone, Copy)]
pub(super) enum Repr<R> {
    Empty,
    Absent,
    Unit,
    Bool(bool),
    Char(char),
    I32(i32),
    I64(i64),
    U64(u64),
    F32(f32),
    F64(f64),
    Object(R),
}

impl<R> Repr<R> {
    /// Codec boundary only: construct a scalar without converting references.
    pub(super) fn from_scalar(value: ScalarValue) -> Self {
        match value {
            ScalarValue::Unit => Self::Unit,
            ScalarValue::Bool(value) => Self::Bool(value),
            ScalarValue::Char(value) => Self::Char(value),
            ScalarValue::I32(value) => Self::I32(value),
            ScalarValue::I64(value) => Self::I64(value),
            ScalarValue::U64(value) => Self::U64(value),
            ScalarValue::F32(value) => Self::F32(value),
            ScalarValue::F64(value) => Self::F64(value),
        }
    }

    /// Codec boundary only: a stored edge cannot escape through this projection.
    pub(super) fn scalar(&self) -> Option<ScalarValue> {
        Some(match self {
            Self::Unit => ScalarValue::Unit,
            Self::Bool(value) => ScalarValue::Bool(*value),
            Self::Char(value) => ScalarValue::Char(*value),
            Self::I32(value) => ScalarValue::I32(*value),
            Self::I64(value) => ScalarValue::I64(*value),
            Self::U64(value) => ScalarValue::U64(*value),
            Self::F32(value) => ScalarValue::F32(*value),
            Self::F64(value) => ScalarValue::F64(*value),
            Self::Empty | Self::Absent | Self::Object(_) => return None,
        })
    }
}

/// An opaque, non-Copy stored value. Only reviewed providers can construct it.
pub struct StoredValue {
    pub(super) repr: Repr<Edge<AnyObject>>,
}

impl StoredValue {
    /// Publish for installation in this mutation's traced payload.
    ///
    /// # Safety
    /// Value must be protected by the same scope/heap as the admitted owner.
    /// Install the result in complete traced storage before the scope ends; never
    /// expose owned unrooted carriers or revive them in another access interval.
    pub unsafe fn publish<P: GcObject>(
        mutation: &mut Mutation<'_, P>,
        value: Value<'_>,
    ) -> Result<Self, GcError> {
        let repr = match value.repr {
            Repr::Empty => return Err(GcError::Storage("cannot publish an uninitialized value")),
            Repr::Absent => Repr::Absent,
            Repr::Unit => Repr::Unit,
            Repr::Bool(value) => Repr::Bool(value),
            Repr::Char(value) => Repr::Char(value),
            Repr::I32(value) => Repr::I32(value),
            Repr::I64(value) => Repr::I64(value),
            Repr::U64(value) => Repr::U64(value),
            Repr::F32(value) => Repr::F32(value),
            Repr::F64(value) => Repr::F64(value),
            // SAFETY: inherited live source and same-owner destination proof.
            Repr::Object(value) => Repr::Object(unsafe { mutation.edge_scoped(value)? }),
        };
        Ok(Self { repr })
    }

    /// Trace while the live provider retains this value; caller charges the slot.
    ///
    /// # Safety
    /// This value is a still-retained, initialized field of the currently scanned
    /// same-heap object. Retry Pending without advancing the provider cursor.
    pub unsafe fn scan(
        &self,
        budget: &mut ScanBudget,
        visitor: &mut EdgeVisitor<'_>,
    ) -> Result<ScanStatus, GcError> {
        match &self.repr {
            // SAFETY: inherited provider source protection.
            Repr::Object(edge) => unsafe { visitor.try_visit_stored(edge, budget) },
            _ => Ok(ScanStatus::Complete),
        }
    }

    /// Select once from a retained payload and obtain a scoped value before edits.
    /// The selector cannot return a detached or borrowed-out payload reference.
    pub fn read_from<'gc, P: GcObject>(
        region: &CallRegion<'_, 'gc>,
        object: Gc<'gc, NativeObject<P>>,
        select: impl for<'p> Fn(&'p P) -> Option<&'p Self>,
    ) -> NativeResult<Option<Value<'gc>>> {
        let scalar = Cell::new(None);
        let target = NativeObject::follow(region, object, |payload| {
            let stored = select(payload)?;
            let repr = match &stored.repr {
                Repr::Object(edge) => return Some(edge),
                Repr::Empty => Repr::Empty,
                Repr::Absent => Repr::Absent,
                Repr::Unit => Repr::Unit,
                Repr::Bool(value) => Repr::Bool(*value),
                Repr::Char(value) => Repr::Char(*value),
                Repr::I32(value) => Repr::I32(*value),
                Repr::I64(value) => Repr::I64(*value),
                Repr::U64(value) => Repr::U64(*value),
                Repr::F32(value) => Repr::F32(*value),
                Repr::F64(value) => Repr::F64(*value),
            };
            scalar.set(Some(Value { repr }));
            None
        })?;
        Ok(target
            .map(|target| Value {
                repr: Repr::Object(target),
            })
            .or_else(|| scalar.get()))
    }
}

/// A scoped transport value; copying it does not retain an object independently.
/// Constructors are private so no detached stored carrier can be revived here.
#[derive(Clone, Copy)]
pub struct Value<'gc> {
    pub(super) repr: Repr<Gc<'gc, AnyObject>>,
}

/// An immutable, typed host snapshot with owner identity, including for scalars.
/// Object clones share the collector's root lease; no mutable slot state is cached.
pub struct RootedValue<T: Kind> {
    pub(super) owner: Arc<()>,
    pub(super) repr: Repr<Root<AnyObject>>,
    pub(super) marker: PhantomData<fn() -> T>,
}

impl<T: Kind> Clone for RootedValue<T> {
    fn clone(&self) -> Self {
        Self {
            owner: self.owner.clone(),
            repr: self.repr.clone(),
            marker: PhantomData,
        }
    }
}
