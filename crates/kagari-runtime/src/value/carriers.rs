//! Scoped access, traced storage and escaping Host retention are distinct roles.
#[cfg(test)]
mod tests;

use crate::{
    execution::roots::ValueLocation,
    value::{ephemeral::EphemeralValueId, kind::ObjectKind, view::ValueView},
};
use kagari_abi::representation::ValueType;
use kagari_gc::{
    access::{Init, Mutation, Scope},
    error::GcError,
    object::{AnyObject, Edge, Gc, GcObject},
    root::Root,
    scan::{EdgeVisitor, ScanBudget, ScanStatus},
};

/// Runtime providers assign the physical category of their concrete allocation.
/// This is not language nominal/type authority; declared layouts are checked by
/// the provider's ordinary object operations.
pub(crate) trait ManagedRepresentation: GcObject {
    const KIND: ObjectKind;
}

/// Generic cells preserve the same scalar representation in each lifetime role.
/// R is inline: Gc/Edge/Root never introduces another payload box here.
#[derive(Debug, Clone, Copy)]
pub(super) enum Representation<R> {
    Unit,
    Bool(bool),
    Char(char),
    I32(i32),
    I64(i64),
    U64(u64),
    F32(f32),
    F64(f64),
    RuntimeEphemeral(EphemeralValueId),
    Managed { kind: ObjectKind, target: R },
}

impl<R> Representation<R> {
    pub(super) fn scalar_payload(&self) -> Option<(ValueType, u64)> {
        Some(match self {
            Self::Unit => (ValueType::Unit, 0),
            Self::Bool(value) => (ValueType::Bool, u64::from(*value)),
            Self::Char(value) => (ValueType::Char, u64::from(u32::from(*value))),
            Self::I32(value) => (ValueType::I32, *value as i64 as u64),
            Self::I64(value) => (ValueType::I64, *value as u64),
            Self::U64(value) => (ValueType::U64, *value),
            Self::F32(value) => (ValueType::F32, u64::from(value.to_bits())),
            Self::F64(value) => (ValueType::F64, value.to_bits()),
            Self::Managed { .. } | Self::RuntimeEphemeral(_) => return None,
        })
    }

    pub(super) fn has_representation(&self, ty: ValueType) -> bool {
        if ty == ValueType::Generic {
            return !matches!(
                self,
                Self::RuntimeEphemeral(_)
                    | Self::Managed {
                        kind: ObjectKind::Ephemeral,
                        ..
                    }
            );
        }
        matches!(
            (self, ty),
            (Self::Unit, ValueType::Unit)
                | (Self::Bool(_), ValueType::Bool)
                | (Self::Char(_), ValueType::Char)
                | (Self::I32(_), ValueType::I32)
                | (Self::I64(_), ValueType::I64)
                | (Self::U64(_), ValueType::U64)
                | (Self::F32(_), ValueType::F32)
                | (Self::F64(_), ValueType::F64)
                | (
                    Self::Managed {
                        kind: ObjectKind::String,
                        ..
                    },
                    ValueType::Str
                )
                | (
                    Self::Managed {
                        kind: ObjectKind::HostRoot | ObjectKind::HostPath | ObjectKind::Ephemeral,
                        ..
                    },
                    ValueType::HostHandle
                )
                | (
                    Self::Managed {
                        kind: ObjectKind::Range
                            | ObjectKind::Tuple
                            | ObjectKind::Array
                            | ObjectKind::Enum
                            | ObjectKind::Struct
                            | ObjectKind::Native
                            | ObjectKind::Interface
                            | ObjectKind::Closure
                            | ObjectKind::Cell,
                        ..
                    },
                    ValueType::HeapObject
                )
        )
    }

    pub(super) fn map_reference<S>(
        &self,
        map: impl FnOnce(&R) -> Result<S, GcError>,
    ) -> Result<Representation<S>, GcError> {
        Ok(match self {
            Self::Unit => Representation::Unit,
            Self::Bool(value) => Representation::Bool(*value),
            Self::Char(value) => Representation::Char(*value),
            Self::I32(value) => Representation::I32(*value),
            Self::I64(value) => Representation::I64(*value),
            Self::U64(value) => Representation::U64(*value),
            Self::F32(value) => Representation::F32(*value),
            Self::F64(value) => Representation::F64(*value),
            Self::RuntimeEphemeral(value) => Representation::RuntimeEphemeral(*value),
            Self::Managed { kind, target } => Representation::Managed {
                kind: *kind,
                target: map(target)?,
            },
        })
    }
}

/// A transient protected value; its brand cannot leave a RootDriver scope.
#[derive(Debug, Clone, Copy)]
pub struct ScopedValue<'gc>(Representation<Gc<'gc, AnyObject>>);

/// An initialized cell in traced source/object storage. Never independently
/// retained, copied or exposed to a Native callback across a scope boundary.
#[derive(Debug)]
pub struct StoredValue(Representation<Edge<AnyObject>>);

/// An escaping Host value. Scalar retention is inline; managed clones share one
/// collector lease and cannot expose a tag or reference independently.
#[must_use = "retain the handle while the Host needs the value"]
#[derive(Debug, Clone)]
pub struct RootedValue(Representation<Root<AnyObject>>);

impl<'gc> ScopedValue<'gc> {
    pub(super) fn from_representation(value: Representation<Gc<'gc, AnyObject>>) -> Self {
        Self(value)
    }

    pub(super) fn representation(self) -> Representation<Gc<'gc, AnyObject>> {
        self.0
    }

    pub(crate) const fn unit() -> Self {
        Self(Representation::Unit)
    }

    pub(crate) fn has_representation(self, ty: ValueType) -> bool {
        self.0.has_representation(ty)
    }

    pub(crate) fn runtime_ephemeral(id: EphemeralValueId) -> Self {
        Self(Representation::RuntimeEphemeral(id))
    }

    pub(crate) fn as_runtime_ephemeral(self) -> Option<EphemeralValueId> {
        match self.0 {
            Representation::RuntimeEphemeral(id) => Some(id),
            _ => None,
        }
    }

    pub(crate) fn managed<T: ManagedRepresentation>(target: Gc<'gc, T>) -> Self {
        Self(Representation::Managed {
            kind: T::KIND,
            target: target.erase(),
        })
    }

    pub(crate) fn object(self) -> Option<Gc<'gc, AnyObject>> {
        match self.0 {
            Representation::Managed { target, .. } => Some(target),
            _ => None,
        }
    }

    pub(crate) fn kind(self) -> Option<ObjectKind> {
        match self.0 {
            Representation::Managed { kind, .. } => Some(kind),
            Representation::RuntimeEphemeral(_) => Some(ObjectKind::Ephemeral),
            _ => None,
        }
    }

    pub(crate) fn is_unit(self) -> bool {
        matches!(self.0, Representation::Unit)
    }

    /// Canonical scalar tags and complete bits; references never become integers.
    pub(crate) fn scalar_payload(self) -> Option<(ValueType, u64)> {
        self.0.scalar_payload()
    }

    pub(crate) fn retain(self, cx: &mut Scope<'gc>) -> Result<RootedValue, GcError> {
        self.0
            .map_reference(|target| cx.retain(*target))
            .map(RootedValue)
    }
}

impl StoredValue {
    /// Inspect inline data without borrowing or exposing a stored edge.
    pub(crate) fn is_inline_scalar(&self) -> bool {
        self.0.scalar_payload().is_some()
    }

    pub(crate) const fn unit() -> Self {
        Self(Representation::Unit)
    }

    /// Copy only the scalar/tag/coordinate description of an admitted source
    /// cell. The source owner chooses the nonescaping view lifetime; this does
    /// not import, copy or dereference the stored edge.
    pub(crate) fn source_view<'view>(
        &self,
        location: ValueLocation,
    ) -> Result<ValueView<'view>, GcError> {
        self.0
            .map_reference(|_| Ok(location))
            .map(ValueView::from_representation)
    }

    /// Initialize an ordinary traced object field from a protected value.
    ///
    /// # Safety
    /// Store the result only in this initializer's fully traced returned object.
    /// Never use source publication as a substitute for object initialization.
    pub unsafe fn initialize<'gc>(
        init: &mut Init<'_>,
        value: ScopedValue<'gc>,
    ) -> Result<Self, GcError> {
        value
            .0
            .map_reference(|target| {
                // SAFETY: the caller installs only in this initialized object's fields.
                unsafe { init.edge_scoped(*target) }
            })
            .map(Self)
    }

    /// Prepare a coupled value for one admitted provider's traced field.
    ///
    /// # Safety
    /// The incoming handle belongs to this mutation's heap. Install the result
    /// only into this owner's fully traced storage before the operation ends.
    pub(crate) unsafe fn publish_field<T: GcObject>(
        mutation: &mut Mutation<'_, T>,
        value: ScopedValue<'_>,
    ) -> Result<Self, GcError> {
        value
            .0
            .map_reference(|target| {
                // SAFETY: the provider establishes same-heap protection and commits
                // the coupled carrier into this mutation's scanned destination.
                unsafe { mutation.edge_scoped(*target) }
            })
            .map(Self)
    }

    /// Read one selected field and its tag under the same protected object loan.
    /// The selector runs once; only a scoped handle leaves the payload borrow.
    pub fn get_field<'gc, T: GcObject>(
        cx: &Scope<'gc>,
        owner: Gc<'gc, T>,
        select: impl for<'field> FnOnce(&'field T) -> Option<&'field Self>,
    ) -> Result<ScopedValue<'gc>, GcError> {
        let mut snapshot = None;
        let target = cx.follow(owner, |owner| {
            let field = select(owner)?;
            snapshot = Some(field.0.map_reference(|_| Ok(())));
            match &field.0 {
                Representation::Managed { target, .. } => Some(target),
                _ => None,
            }
        });
        snapshot
            .ok_or(GcError::Index)??
            .map_reference(|()| target.ok_or(GcError::Representation))
            .map(ScopedValue)
    }

    /// Publish before committing a coupled tag/reference into the paired source.
    ///
    /// # Safety
    /// Install the returned value only in this Scope's paired RootAccess source
    /// before the scope ends. It must remain traced until replaced or removed.
    /// Do not use this operation to initialize or mutate an object's fields.
    pub(crate) unsafe fn publish_source<'gc>(
        cx: &mut Scope<'gc>,
        value: ScopedValue<'gc>,
    ) -> Result<Self, GcError> {
        value
            .0
            .map_reference(|target| {
                // SAFETY: the caller commits only into the paired traced source.
                unsafe { cx.source_edge(*target) }
            })
            .map(Self)
    }

    /// Import a still-retained source cell into its collection-free interval.
    ///
    /// # Safety
    /// This cell must be an initialized slot in the RootAccess paired with cx.
    /// Its source remains installed for admission; no detached carrier is valid.
    pub(crate) unsafe fn get_source<'gc>(
        &self,
        cx: &mut Scope<'gc>,
    ) -> Result<ScopedValue<'gc>, GcError> {
        self.0
            .map_reference(|target| {
                // SAFETY: the caller supplies the still-retained paired source.
                unsafe { cx.source_get(target) }
            })
            .map(ScopedValue)
    }

    /// Visit a cell already charged by its owning scanner. A pending visit leaves
    /// the owner's coordinate unchanged, so it is retried before advancement.
    ///
    /// # Safety
    /// The admitted source/object retains this initialized cell in the visitor's
    /// heap for the whole scan call. No carrier may be extracted or dereferenced.
    pub unsafe fn scan(
        &self,
        budget: &mut ScanBudget,
        visitor: &mut EdgeVisitor<'_>,
    ) -> Result<ScanStatus, GcError> {
        match &self.0 {
            Representation::Managed { target, .. } => {
                // SAFETY: the owning scanner retains this same-heap source cell.
                unsafe { visitor.try_visit_stored(target, budget) }
            }
            _ => Ok(ScanStatus::Complete),
        }
    }
}

impl RootedValue {
    /// Copy inline bits only; managed roots still require physical admission.
    pub(crate) fn scalar_payload(&self) -> Option<(ValueType, u64)> {
        self.0.scalar_payload()
    }

    /// An inline Unit value. Constructing or cloning it registers no GC root.
    pub const fn unit() -> Self {
        Self(Representation::Unit)
    }

    pub(crate) fn get<'gc>(&self, cx: &Scope<'gc>) -> Result<ScopedValue<'gc>, GcError> {
        self.0
            .map_reference(|target| cx.get(target))
            .map(ScopedValue)
    }
}

macro_rules! scalar_conversions {
    ($($ty:ty => $variant:ident: $accessor:ident),* $(,)?) => {$(
        impl<R> Representation<R> {
            pub(super) fn $accessor(&self) -> Option<$ty> {
                match self { Self::$variant(value) => Some(*value), _ => None }
            }
        }

        impl RootedValue {
            /// Read an inline scalar without dereferencing a managed object.
            pub fn $accessor(&self) -> Option<$ty> {
                self.0.$accessor()
            }
        }

        impl From<$ty> for ScopedValue<'_> {
            fn from(value: $ty) -> Self {
                Self(Representation::$variant(value))
            }
        }

        impl From<$ty> for ValueView<'_> {
            fn from(value: $ty) -> Self {
                Self::from_representation(Representation::$variant(value))
            }
        }

        impl From<$ty> for RootedValue {
            fn from(value: $ty) -> Self {
                Self(Representation::$variant(value))
            }
        }
    )*};
}

scalar_conversions!(bool => Bool: as_bool, char => Char: as_char, i32 => I32: as_i32,
    i64 => I64: as_i64, u64 => U64: as_u64, f32 => F32: as_f32, f64 => F64: as_f64);
