//! Authoritative typed working slots, scanned directly by RootDriver.

use crate::native::{
    managed::{
        payload::NativeObject,
        value::{Repr, StoredValue, Value},
    },
    scalar::{NativeScalar, ScalarCodec},
};
use kagari_gc::{
    access::Scope,
    error::GcError,
    object::{Gc, GcObject},
    root_source::{RootAccess, RootSource},
    scan::{EdgeVisitor, ScanBudget, ScanStatus},
};
use kagari_types::scalar::BuiltinType;
use std::{marker::PhantomData, sync::Arc};

/// A kind admitted by the bounded Native prototype, not a script nominal type.
pub trait Kind: private::Sealed + 'static {
    type View<'gc>: Copy;

    /// Exact scalar mapping available to the bounded pre-cutover adapter.
    /// Native objects deliberately return None: physical type is not nominal proof.
    fn scalar_type() -> Option<BuiltinType> {
        None
    }

    #[doc(hidden)]
    fn validate<'gc>(_: &Scope<'gc>, _: Self::View<'gc>) -> Result<(), GcError> {
        Ok(())
    }

    #[doc(hidden)]
    fn lower<'gc>(value: Self::View<'gc>) -> Value<'gc>;

    #[doc(hidden)]
    fn admit<'gc>(scope: &Scope<'gc>, value: Value<'gc>) -> Result<Self::View<'gc>, GcError>;
}

mod private {
    pub trait Sealed {}
}

/// One optional working-slot value. This is not the language Option encoding.
/// Nested options are intentionally not part of this bounded family.
pub struct Optional<T: Plain>(PhantomData<T>);

/// Non-optional kinds supported by the optional working-slot projection.
pub trait Plain: Kind {}

impl<P: GcObject> Plain for Object<NativeObject<P>> {}

impl<T: Plain> private::Sealed for Optional<T> {}

impl<T: Plain> Kind for Optional<T> {
    type View<'gc> = Option<T::View<'gc>>;

    fn validate<'gc>(scope: &Scope<'gc>, value: Self::View<'gc>) -> Result<(), GcError> {
        value.map_or(Ok(()), |value| T::validate(scope, value))
    }

    fn lower<'gc>(value: Self::View<'gc>) -> Value<'gc> {
        value.map_or(Value { repr: Repr::Absent }, T::lower)
    }

    fn admit<'gc>(scope: &Scope<'gc>, value: Value<'gc>) -> Result<Self::View<'gc>, GcError> {
        if matches!(value.repr, Repr::Absent) {
            Ok(None)
        } else {
            T::admit(scope, value).map(Some)
        }
    }
}

/// A native object's physical representation; nominal mappings remain link-owned.
pub struct Object<T: GcObject>(PhantomData<T>);

impl<T: GcObject> private::Sealed for Object<T> {}

/// Protected slot coordinates. They contain no managed pointer or root lease.
pub struct Local<'call, T: Kind> {
    pub(super) owner: &'call Arc<()>,
    pub(super) index: usize,
    pub(super) marker: PhantomData<T>,
}

impl<T: Kind> Copy for Local<'_, T> {}

impl<T: Kind> Clone for Local<'_, T> {
    fn clone(&self) -> Self {
        *self
    }
}

/// Complete direct-source storage for this proof's active nested calls.
pub(super) struct Slots {
    pub(super) owner: Arc<()>,
    pub(super) values: Vec<StoredValue>,
}

/// Exclusive disjoint access to the source and its paired collection-free scope.
pub(super) struct SlotRegion<'a, 'gc> {
    pub(super) scope: &'a mut Scope<'gc>,
    pub(super) slots: &'a mut Slots,
}

impl Slots {
    pub(super) fn region<'a, 'gc>(
        roots: &'a mut RootAccess<'_, Self>,
        scope: &'a mut Scope<'gc>,
    ) -> Result<SlotRegion<'a, 'gc>, GcError> {
        // SAFETY: only this private facade mutates the paired source; installs
        // publish first and retained carriers are never exposed to safe clients.
        let slots = unsafe { roots.source_mut(scope)? };
        Ok(SlotRegion { scope, slots })
    }
}

impl<'gc> SlotRegion<'_, 'gc> {
    pub(super) fn check<T: Kind>(&self, local: Local<'_, T>) -> Result<usize, GcError> {
        if !Arc::ptr_eq(local.owner, &self.slots.owner) {
            return Err(GcError::ForeignHeap);
        }
        if local.index >= self.slots.values.len() {
            return Err(GcError::Index);
        }
        Ok(local.index)
    }

    pub fn read<T: Kind>(&mut self, local: Local<'_, T>) -> Result<T::View<'gc>, GcError> {
        let index = self.check(local)?;
        let repr = match &self.slots.values[index].repr {
            Repr::Empty => return Err(GcError::Storage("uninitialized slot")),
            Repr::Absent => Repr::Absent,
            Repr::Unit => Repr::Unit,
            Repr::Bool(value) => Repr::Bool(*value),
            Repr::Char(value) => Repr::Char(*value),
            Repr::I32(value) => Repr::I32(*value),
            Repr::I64(value) => Repr::I64(*value),
            Repr::U64(value) => Repr::U64(*value),
            Repr::F32(value) => Repr::F32(*value),
            Repr::F64(value) => Repr::F64(*value),
            Repr::Object(edge) => {
                // SAFETY: the paired authoritative source retains this edge.
                Repr::Object(unsafe { self.scope.source_get(edge)? })
            }
        };
        T::admit(self.scope, Value { repr })
    }

    pub fn write<T: Kind>(
        &mut self,
        local: Local<'_, T>,
        value: T::View<'gc>,
    ) -> Result<(), GcError> {
        let index = self.check(local)?;
        T::validate(self.scope, value)?;
        let stored = match T::lower(value).repr {
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
            Repr::Object(value) => {
                // SAFETY: immediately commit the published edge to this source.
                Repr::Object(unsafe { self.scope.source_edge(value)? })
            }
        };
        self.slots.values[index] = StoredValue { repr: stored };
        Ok(())
    }
}

macro_rules! scalar_kinds {
    ($($rust:ty),+ $(,)?) => { $(
        impl private::Sealed for $rust {}

        impl Plain for $rust {}

        impl Kind for $rust {
            type View<'gc> = Self;

            fn scalar_type() -> Option<BuiltinType> {
                Some(Self::abi_type())
            }

            #[inline]
            fn lower<'gc>(value: Self) -> Value<'gc> {
                Value { repr: Repr::from_scalar(value.encode_scalar()) }
            }

            #[inline]
            fn admit<'gc>(_: &Scope<'gc>, value: Value<'gc>) -> Result<Self, GcError> {
                let value = value.repr.scalar().ok_or(GcError::Representation)?;
                Self::decode_scalar(value).map_err(|_| GcError::Representation)
            }
        }
    )+ };
}

scalar_kinds!(
    (),
    bool,
    char,
    i8,
    i16,
    i32,
    i64,
    u8,
    u16,
    u32,
    u64,
    f32,
    f64
);

impl<P: GcObject> Kind for Object<NativeObject<P>> {
    type View<'gc> = Gc<'gc, NativeObject<P>>;

    fn validate<'gc>(scope: &Scope<'gc>, value: Self::View<'gc>) -> Result<(), GcError> {
        scope.read(value, |shell| {
            if shell.payload.is_some() {
                Ok(())
            } else {
                Err(GcError::Storage("native payload was taken"))
            }
        })
    }

    fn lower<'gc>(value: Self::View<'gc>) -> Value<'gc> {
        Value {
            repr: Repr::Object(value.erase()),
        }
    }

    fn admit<'gc>(scope: &Scope<'gc>, value: Value<'gc>) -> Result<Self::View<'gc>, GcError> {
        match value.repr {
            Repr::Object(value) => {
                let value = scope.downcast(value)?;
                Self::validate(scope, value)?;
                Ok(value)
            }
            _ => Err(GcError::Representation),
        }
    }
}

// SAFETY: private source writes publish first; truncation only removes roots.
// The cursor is a numeric frontier, reborrowed on each slice. New slots publish.
unsafe impl RootSource for Slots {
    type Cursor = (usize, usize);

    fn empty() -> Self {
        Self {
            owner: Arc::new(()),
            values: Vec::new(),
        }
    }

    fn begin_scan(&self) -> Result<Self::Cursor, GcError> {
        Ok((0, self.values.len()))
    }

    fn scan(
        &self,
        cursor: &mut Self::Cursor,
        budget: &mut ScanBudget,
        visitor: &mut EdgeVisitor<'_>,
    ) -> Result<ScanStatus, GcError> {
        while cursor.0 < cursor.1.min(self.values.len()) {
            if !budget.take(1) {
                return Ok(ScanStatus::Pending);
            }
            if let Repr::Object(edge) = &self.values[cursor.0].repr {
                // SAFETY: the source still retains this same-heap edge. Pending
                // retries the same position, including marker bitmap setup.
                if unsafe { visitor.try_visit_stored(edge, budget)? } == ScanStatus::Pending {
                    return Ok(ScanStatus::Pending);
                }
            }
            cursor.0 += 1;
        }
        Ok(ScanStatus::Complete)
    }
}
