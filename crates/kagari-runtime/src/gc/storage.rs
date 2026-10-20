//! Physical object representations provide tracing and accounting to collection.
use crate::{
    closure::ClosureValueSnapshot,
    error_trace::ErrorTrace,
    execution_metadata::{
        MetadataEdge,
        interfaces::{InterfaceSnapshotId, InterfaceStore},
    },
    gc::{
        HeapObjectId,
        fields::{StructInitializer, StructView, StructViewMut},
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
        structure.description.fields.len()
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
            Self::Range(_) | Self::HostRoot(_) | Self::Ephemeral(_) => 0,
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
    /// Executable state is retained by graph reachability, never by an internal root.
    /// Plain data layouts only retain immutable type metadata.
    pub(super) fn metadata(self) -> Option<MetadataEdge<'a>> {
        match self {
            Self::Interface { snapshot, .. } => Some(MetadataEdge::Interface(snapshot)),
            Self::Closure { snapshot } => Some(MetadataEdge::Closure(snapshot)),
            Self::Native(object) if !object.selected.is_empty() => {
                Some(MetadataEdge::Selections(&object.selected))
            }
            _ => None,
        }
    }

    /// Append stored edges in the existing LIFO traversal order.
    pub(super) fn trace(
        self,
        interfaces: &'a InterfaceStore,
        visit: &mut dyn FnMut(Value),
    ) -> Option<()> {
        match self {
            Self::String(_) | Self::Range(_) | Self::HostRoot(_) | Self::Ephemeral(_) => {}
            Self::Tuple(tuple) => tuple.members.iter().rev().copied().for_each(visit),
            Self::HostPath(path) => path.retained_values().copied().for_each(visit),
            Self::Native(object) => object.trace(&mut |value| visit(*value)),
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
pub(super) fn append_value_edges(values: &mut Vec<Value>, pending: &mut Vec<HeapObjectId>) {
    let start = pending.len();
    while let Some(value) = values.pop() {
        match value {
            Value::Str(id)
            | Value::Tuple(id)
            | Value::Range(id)
            | Value::HostRoot(id)
            | Value::HostPathView(id)
            | Value::Ephemeral(id)
            | Value::Array(id)
            | Value::Map(id)
            | Value::Set(id)
            | Value::Enum(id)
            | Value::Struct(id)
            | Value::GcHandle(id)
            | Value::Closure(id)
            | Value::Cell(id) => pending.push(id),
            Value::Interface(id) => pending.push(id.0),
            Value::Unit
            | Value::Bool(_)
            | Value::I32(_)
            | Value::I64(_)
            | Value::U64(_)
            | Value::F32(_)
            | Value::F64(_)
            | Value::RuntimeEphemeral(_) => {}
        }
    }
    pending[start..].reverse();
}
