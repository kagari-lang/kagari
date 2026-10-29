//! Physical object representations provide tracing and accounting to collection.
use crate::{
    closure::ClosureValueSnapshot,
    error_trace::ErrorTrace,
    execution_metadata::interfaces::{InterfaceSnapshotId, InterfaceStore},
    gc::{
        GcHeap,
        directory::ObjectStorage,
        fields::{StructInitializer, StructView, StructViewMut},
        reference::GcRef,
        values::TupleData,
    },
    host::{HostPathViewHandle, HostRootHandle},
    native::storage::NativeObject,
    range::RangeValue,
    resource::HeapCharge,
    value::{EnumValueSnapshot, EphemeralValue, Value},
};
use kagari_abi::representation::ValueType;
use std::{borrow::Borrow, cell::RefCell, fmt::Debug, marker::PhantomData, sync::Arc};

/// One object shape for owned transfer and scoped shared/exclusive views. The
/// heap stores typed payloads; this enum is only a transient access/transfer value.
pub(super) trait ObjectAccess {
    type Field<T: Debug + 'static>: Debug + Borrow<T>;
    type Structure: Debug;

    fn field_count(structure: &Self::Structure) -> usize;
}

#[derive(Debug)]
pub(super) struct Owned;

#[derive(Debug)]
pub(super) struct Shared<'a>(PhantomData<&'a ()>);

#[derive(Debug)]
pub(super) struct Exclusive<'a>(PhantomData<&'a mut ()>);

impl ObjectAccess for Owned {
    type Field<T: Debug + 'static> = T;
    type Structure = StructInitializer;

    fn field_count(structure: &Self::Structure) -> usize {
        structure.descriptor.structure_layout().fields.len()
    }
}

impl<'a> ObjectAccess for Shared<'a> {
    type Field<T: Debug + 'static> = &'a T;
    type Structure = StructView<'a>;

    fn field_count(structure: &Self::Structure) -> usize {
        structure.description.fields.len()
    }
}

impl<'a> ObjectAccess for Exclusive<'a> {
    type Field<T: Debug + 'static> = &'a mut T;
    type Structure = StructViewMut<'a>;

    fn field_count(structure: &Self::Structure) -> usize {
        structure.description.fields.len()
    }
}

#[derive(Debug)]
pub(super) enum HeapObject<A: ObjectAccess = Owned> {
    String(A::Field<String>),
    Tuple(A::Field<TupleData>),
    Range(A::Field<RangeValue>),
    HostRoot(A::Field<HostRootHandle>),
    HostPath(A::Field<Arc<HostPathViewHandle>>),
    Ephemeral(A::Field<EphemeralValue>),
    Native(A::Field<NativeObject>),
    /// Only the header remains initialized; no Rust payload or metadata is owned.
    Taken,
    Enum(
        A::Field<EnumValueSnapshot>,
        A::Field<Option<Arc<ErrorTrace>>>,
    ),
    Struct {
        storage: A::Structure,
    },
    /// Checked identity copied from the separately borrowed metadata owner. Even
    /// exclusive heap access cannot replace a snapshot or its receiver here.
    Interface {
        snapshot: InterfaceSnapshotId,
    },
    Closure {
        snapshot: A::Field<ClosureValueSnapshot>,
    },
    Cell {
        ty: A::Field<ValueType>,
        value: A::Field<Value>,
    },
}

impl Copy for HeapObject<Shared<'_>> {}

impl Clone for HeapObject<Shared<'_>> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<A: ObjectAccess> HeapObject<A> {
    pub(super) fn units(&self, interfaces: &RefCell<InterfaceStore>) -> Option<usize> {
        let payload = match self {
            Self::String(text) => text.borrow().len(),
            Self::Tuple(tuple) => tuple.borrow().members.len(),
            Self::HostPath(path) => path.borrow().retained_values().count(),
            Self::Range(_) | Self::HostRoot(_) | Self::Ephemeral(_) | Self::Taken => 0,
            Self::Native(object) => object.borrow().charge.logical_units,
            Self::Enum(value, _) => value.borrow().fields.len(),
            Self::Struct { storage } => A::field_count(storage),
            Self::Interface { snapshot } => {
                let interfaces = interfaces.try_borrow().ok()?;
                1 + interfaces.get(*snapshot)?.description.methods.len()
            }
            Self::Closure { snapshot, .. } => 1 + snapshot.borrow().captures.len(),
            Self::Cell { .. } => 2,
        };
        payload.checked_add(1)
    }
}

impl<A: ObjectAccess> HeapObject<A> {
    pub(super) fn charge(&self, interfaces: &RefCell<InterfaceStore>) -> Option<HeapCharge> {
        let retained_bytes = match self {
            Self::String(text) => text.borrow().capacity(),
            Self::Native(object) => object.borrow().charge.retained_bytes,
            _ => 0,
        };
        Some(HeapCharge::new(self.units(interfaces)?, retained_bytes))
    }
}

impl<'a> HeapObject<Shared<'a>> {
    /// Inspection uses the provider's scoped state, independent of a suspended
    /// collector cursor. Representation edge semantics remain shared with tracing.
    pub(super) fn inspect(
        self,
        interfaces: &'a InterfaceStore,
        visit: &mut dyn FnMut(Value),
    ) -> Option<()> {
        self.visit_edges(interfaces, visit, |object, visit| {
            object.inspect(&mut |value| visit(*value)).ok()
        })
    }

    fn visit_edges(
        self,
        interfaces: &'a InterfaceStore,
        visit: &mut dyn FnMut(Value),
        native: impl FnOnce(&'a NativeObject, &mut dyn FnMut(Value)) -> Option<()>,
    ) -> Option<()> {
        match self {
            Self::String(_)
            | Self::Range(_)
            | Self::HostRoot(_)
            | Self::Ephemeral(_)
            | Self::Taken => {}
            Self::Tuple(tuple) => tuple.members.iter().rev().copied().for_each(visit),
            Self::HostPath(path) => path.retained_values().copied().for_each(visit),
            Self::Native(object) => native(object, visit)?,
            Self::Enum(snapshot, _) => snapshot.fields.iter().rev().copied().for_each(visit),
            Self::Struct { storage } => storage.trace(visit)?,
            Self::Interface { snapshot, .. } => visit(interfaces.get(snapshot)?.data),
            Self::Closure { snapshot, .. } => {
                snapshot.captures.iter().rev().copied().for_each(visit)
            }
            Self::Cell { value, .. } => visit(*value),
        }
        Some(())
    }
}

/// Append checked identities without exposing value kinds to the graph marker.
/// Immutable aggregates own their edges in heap storage like other objects.
pub(super) fn append_value_edges(
    heap: &GcHeap,
    objects: &ObjectStorage,
    values: &mut Vec<Value>,
    pending: &mut Vec<GcRef>,
) -> Option<()> {
    let start = pending.len();
    while let Some(value) = values.pop() {
        if value.is_managed() {
            pending.push(heap.value_identity(objects, &value)?);
        }
    }
    pending[start..].reverse();
    Some(())
}
