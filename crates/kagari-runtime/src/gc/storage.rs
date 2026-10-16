//! Physical object representations provide tracing and accounting to collection.
use crate::{
    closure::ClosureValueSnapshot,
    error_trace::ErrorTrace,
    execution_metadata::{
        MetadataEdge,
        interfaces::{InterfaceSnapshotId, InterfaceStore},
    },
    gc::{HeapObjectId, fields::StructStorage, values::TupleData},
    host::{HostPathViewHandle, HostRootHandle},
    native::storage::NativeObject,
    range::RangeValue,
    value::{EnumValueSnapshot, EphemeralValue, Value},
};
use kagari_abi::representation::ValueType;
use std::{fmt::Debug, marker::PhantomData, sync::Arc};

/// One object shape for owned transfer and scoped shared/exclusive views. The
/// heap stores typed payloads; this enum is only a transient access/transfer value.
pub(super) trait ObjectAccess {
    type Field<T: Debug + 'static>: Debug;
}

#[derive(Debug)]
pub(super) struct Owned;

#[derive(Debug)]
pub(super) struct Shared<'a>(PhantomData<&'a ()>);

#[derive(Debug)]
pub(super) struct Exclusive<'a>(PhantomData<&'a mut ()>);

impl ObjectAccess for Owned {
    type Field<T: Debug + 'static> = T;
}

impl<'a> ObjectAccess for Shared<'a> {
    type Field<T: Debug + 'static> = &'a T;
}

impl<'a> ObjectAccess for Exclusive<'a> {
    type Field<T: Debug + 'static> = &'a mut T;
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
        storage: A::Field<StructStorage>,
    },
    Interface {
        snapshot: A::Field<InterfaceSnapshotId>,
        method_count: A::Field<usize>,
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

impl HeapObject {
    pub(super) fn view(&self) -> HeapObject<Shared<'_>> {
        match self {
            Self::String(value) => HeapObject::String(value),
            Self::Tuple(value) => HeapObject::Tuple(value),
            Self::Range(value) => HeapObject::Range(value),
            Self::HostRoot(value) => HeapObject::HostRoot(value),
            Self::HostPath(value) => HeapObject::HostPath(value),
            Self::Ephemeral(value) => HeapObject::Ephemeral(value),
            Self::Native(value) => HeapObject::Native(value),
            Self::Enum(value, trace) => HeapObject::Enum(value, trace),
            Self::Struct { storage } => HeapObject::Struct { storage },
            Self::Interface {
                snapshot,
                method_count,
            } => HeapObject::Interface {
                snapshot,
                method_count,
            },
            Self::Closure { snapshot } => HeapObject::Closure { snapshot },
            Self::Cell { ty, value } => HeapObject::Cell { ty, value },
        }
    }

    pub(super) fn units(&self) -> usize {
        self.view().units()
    }
}

impl HeapObject<Shared<'_>> {
    pub(super) fn units(self) -> usize {
        1 + match self {
            Self::String(text) => text.len(),
            Self::Tuple(tuple) => tuple.members.len(),
            Self::HostPath(path) => path.retained_values().count(),
            Self::Range(_) | Self::HostRoot(_) | Self::Ephemeral(_) => 0,
            Self::Native(object) => object.units(),
            Self::Enum(value, _) => value.fields.len(),
            Self::Struct { storage } => storage.description.fields.len(),
            Self::Interface { method_count, .. } => 1 + *method_count,
            Self::Closure { snapshot, .. } => 1 + snapshot.captures.len(),
            Self::Cell { .. } => 2,
        }
    }
}

impl<'a> HeapObject<Shared<'a>> {
    /// Executable state is retained by graph reachability, never by an internal root.
    /// Plain data layouts only retain immutable type metadata.
    pub(super) fn metadata(self) -> Option<MetadataEdge<'a>> {
        match self {
            Self::Interface { snapshot, .. } => Some(MetadataEdge::Interface(*snapshot)),
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
            Self::Interface { snapshot, .. } => visit(interfaces.get(*snapshot)?.data),
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
