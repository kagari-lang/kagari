//! Physical object representations provide tracing and accounting to collection.
use crate::{
    closure::ClosureValueSnapshot,
    error_trace::ErrorTrace,
    execution_metadata::{
        MetadataEdge,
        interfaces::{InterfaceSnapshotId, InterfaceStore},
    },
    gc::HeapObjectId,
    module::StructLayoutRef,
    native::storage::NativeObject,
    value::{EnumValueSnapshot, Value},
};
use kagari_abi::representation::ValueType;
use std::sync::Arc;

#[derive(Debug)]
pub(super) enum HeapObject {
    Native(NativeObject),
    Enum(EnumValueSnapshot, Option<Arc<ErrorTrace>>),
    Struct {
        layout: StructLayoutRef,
        fields: Vec<Value>,
    },
    Interface {
        snapshot: InterfaceSnapshotId,
        method_count: usize,
    },
    Closure {
        snapshot: ClosureValueSnapshot,
    },
    Cell {
        ty: ValueType,
        value: Value,
    },
}

impl HeapObject {
    pub(super) fn units(&self) -> usize {
        1 + match self {
            Self::Native(object) => object.units(),
            Self::Enum(value, _) => value.fields.len(),
            Self::Struct { fields, .. } => fields.len(),
            Self::Interface { method_count, .. } => 1 + method_count,
            Self::Closure { snapshot, .. } => 1 + snapshot.captures.len(),
            Self::Cell { .. } => 2,
        }
    }
}

impl HeapObject {
    /// Executable state is retained by graph reachability, never by an internal root.
    /// Plain data layouts only retain immutable type metadata.
    pub(super) fn metadata(&self) -> Option<MetadataEdge<'_>> {
        match self {
            Self::Interface { snapshot, .. } => Some(MetadataEdge::Interface(*snapshot)),
            Self::Closure { snapshot } => Some(MetadataEdge::Closure(snapshot)),
            _ => None,
        }
    }

    /// Append stored edges in the existing LIFO traversal order.
    pub(super) fn trace<'a>(
        &'a self,
        interfaces: &'a InterfaceStore,
        visit: &mut dyn FnMut(&'a Value),
    ) -> Option<()> {
        match self {
            Self::Native(object) => object.trace(visit),
            Self::Enum(snapshot, _) => snapshot.fields.iter().rev().for_each(visit),
            Self::Struct { fields, .. } => fields.iter().rev().for_each(visit),
            Self::Interface { snapshot, .. } => visit(&interfaces.get(*snapshot)?.data),
            Self::Closure { snapshot, .. } => snapshot.captures.iter().rev().for_each(visit),
            Self::Cell { value, .. } => visit(value),
        }
        Some(())
    }
}

/// Flatten inline values into checked object identities without exposing value
/// kinds to the graph marker. Preserve the caller's pending-stack traversal order.
pub(super) fn append_value_edges(values: &mut Vec<&Value>, pending: &mut Vec<HeapObjectId>) {
    let start = pending.len();
    while let Some(value) = values.pop() {
        match value {
            Value::HostPathView(view) => values.extend(
                view.dynamic_args()
                    .as_slice()
                    .iter()
                    .rev()
                    .map(|arg| &arg.value),
            ),
            Value::Tuple(elements) => values.extend(elements.iter().rev()),
            Value::Array(id)
            | Value::Map(id)
            | Value::Set(id)
            | Value::Enum(id)
            | Value::Struct(id)
            | Value::GcHandle(id)
            | Value::Closure(id)
            | Value::Cell(id) => pending.push(*id),
            Value::Interface(id) => pending.push(id.0),
            Value::Unit
            | Value::Bool(_)
            | Value::I32(_)
            | Value::I64(_)
            | Value::U64(_)
            | Value::F32(_)
            | Value::F64(_)
            | Value::Str(_)
            | Value::Range(_)
            | Value::HostRoot(_)
            | Value::Ephemeral(_) => {}
        }
    }
    pending[start..].reverse();
}
