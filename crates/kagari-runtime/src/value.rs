pub(crate) mod scalar;

use crate::{
    gc::{GcHeap, HeapObjectId},
    host::{FrameHostBorrowToken, scoped::ScopedHostView},
    module::EnumVariantRef,
};
use kagari_abi::representation::ValueType;
use kagari_contract::representation::semantic_representation;

#[derive(Debug, Clone, PartialEq)]
pub struct StructValueField {
    pub name: String,
    pub value: Value,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EnumValueSnapshot {
    pub tag: EnumTag,
    pub fields: Vec<Value>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum EnumTag {
    Declared(EnumVariantRef),
}

impl EnumTag {
    pub fn type_name(&self) -> &str {
        match self {
            Self::Declared(layout) => layout
                .module()
                .definition_name(layout.layout().declaration)
                .expect("enum declaration"),
        }
    }

    pub fn variant_name(&self) -> &str {
        match self {
            Self::Declared(layout) => layout
                .module()
                .definition_name(layout.variant().declaration)
                .expect("variant declaration"),
        }
    }

    pub(crate) fn accepts_representations(&self, fields: &[Value]) -> bool {
        match self {
            Self::Declared(layout) => {
                fields.len() == layout.variant().payload.len()
                    && fields
                        .iter()
                        .zip(&layout.variant().payload)
                        .all(|(value, ty)| value.has_representation(semantic_representation(ty)))
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct InterfaceObjectId(pub(crate) HeapObjectId);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EphemeralValueId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueCategory {
    Unit,
    Primitive,
    ScriptOwned,
    Interface,
    HostHandle,
    HostPathView,
    Ephemeral,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EphemeralValue {
    HostRef(FrameHostBorrowToken),
    HostMut(FrameHostBorrowToken),
    Runtime(EphemeralValueId),
    HostView(ScopedHostView),
}

/// Compact unrooted transport. Heap-backed payloads are accessed only through
/// their owning heap; copying a value never copies text or establishes a root.
/// Rust equality compares transport tags and IDs. Use `value_semantics::script_equal`
/// for language equality, including string contents and tuple members.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Value {
    Unit,
    Bool(bool),
    Char(char),
    I32(i32),
    I64(i64),
    U64(u64),
    F32(f32),
    F64(f64),
    Str(HeapObjectId),
    Tuple(HeapObjectId),
    Range(HeapObjectId),
    Array(HeapObjectId),
    Enum(HeapObjectId),
    Struct(HeapObjectId),
    GcHandle(HeapObjectId),
    Interface(InterfaceObjectId),
    Closure(HeapObjectId),
    Cell(HeapObjectId),
    HostRoot(HeapObjectId),
    HostPathView(HeapObjectId),
    Ephemeral(HeapObjectId),
    RuntimeEphemeral(EphemeralValueId),
}

impl Value {
    pub(crate) fn object_id(&self) -> Option<HeapObjectId> {
        match self {
            Self::Unit
            | Self::Bool(_)
            | Self::Char(_)
            | Self::I32(_)
            | Self::I64(_)
            | Self::U64(_)
            | Self::F32(_)
            | Self::F64(_)
            | Self::RuntimeEphemeral(_) => None,
            Self::Interface(id) => Some(id.0),
            Self::Str(id)
            | Self::Tuple(id)
            | Self::Range(id)
            | Self::Array(id)
            | Self::Enum(id)
            | Self::Struct(id)
            | Self::GcHandle(id)
            | Self::Closure(id)
            | Self::Cell(id)
            | Self::HostRoot(id)
            | Self::HostPathView(id)
            | Self::Ephemeral(id) => Some(*id),
        }
    }

    pub fn has_representation(&self, ty: ValueType) -> bool {
        if ty == ValueType::Generic {
            return !matches!(self, Self::Ephemeral(_) | Self::RuntimeEphemeral(_));
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
                | (Self::Str(_), ValueType::Str)
                | (
                    Self::HostRoot(_) | Self::HostPathView(_) | Self::Ephemeral(_),
                    ValueType::HostHandle
                )
                | (
                    Self::Range(_)
                        | Self::Tuple(_)
                        | Self::Array(_)
                        | Self::Enum(_)
                        | Self::Struct(_)
                        | Self::GcHandle(_)
                        | Self::Interface(_)
                        | Self::Closure(_)
                        | Self::Cell(_),
                    ValueType::HeapObject
                )
        )
    }

    pub fn category(&self) -> ValueCategory {
        match self {
            Self::Unit => ValueCategory::Unit,
            Self::Bool(_)
            | Self::Char(_)
            | Self::I32(_)
            | Self::I64(_)
            | Self::U64(_)
            | Self::F32(_)
            | Self::F64(_)
            | Self::Str(_) => ValueCategory::Primitive,
            Self::Range(_)
            | Self::Tuple(_)
            | Self::Array(_)
            | Self::Enum(_)
            | Self::Struct(_)
            | Self::GcHandle(_) => ValueCategory::ScriptOwned,
            Self::Closure(_) => ValueCategory::ScriptOwned,
            Self::Cell(_) => ValueCategory::ScriptOwned,
            Self::Interface(_) => ValueCategory::Interface,
            Self::HostRoot(_) => ValueCategory::HostHandle,
            Self::HostPathView(_) => ValueCategory::HostPathView,
            Self::Ephemeral(_) | Self::RuntimeEphemeral(_) => ValueCategory::Ephemeral,
        }
    }

    pub fn is_storable(&self, heap: &GcHeap) -> bool {
        match self {
            Self::Tuple(id) => heap.tuple_properties(*id).is_some_and(|p| p.storable),
            Self::HostRoot(_)
            | Self::HostPathView(_)
            | Self::Ephemeral(_)
            | Self::RuntimeEphemeral(_) => false,
            _ => heap.validate_value(self),
        }
    }

    pub fn is_ephemeral(&self) -> bool {
        matches!(self, Self::Ephemeral(_) | Self::RuntimeEphemeral(_))
    }

    pub fn contains_ephemeral(&self, heap: &GcHeap) -> bool {
        match self {
            Self::Tuple(id) => heap.tuple_properties(*id).is_none_or(|p| p.ephemeral),
            Self::Ephemeral(_) | Self::RuntimeEphemeral(_) => true,
            _ => false,
        }
    }

    pub fn contains_host_borrow(&self, heap: &GcHeap) -> bool {
        match self {
            Self::Tuple(id) => heap.tuple_properties(*id).is_none_or(|p| p.host_borrow),
            Self::Ephemeral(_) => true,
            _ => false,
        }
    }

    pub fn is_default_heap_payload(&self, heap: &GcHeap) -> bool {
        self.is_storable(heap)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        host::{HostBorrowTable, HostObjectId, HostRootHandle, HostSchemaEpoch},
        metadata::{AbiFingerprint, TypeId},
        test_support::layouts::path_view_value,
    };

    fn host_root(object_id: u64) -> HostRootHandle {
        HostRootHandle::new(
            Default::default(),
            HostObjectId(object_id),
            TypeId::new(0),
            HostSchemaEpoch::new(0),
            AbiFingerprint(1),
        )
    }

    fn shared_borrow_value(heap: &GcHeap, object_id: u64) -> Value {
        let table = HostBorrowTable::default();
        let guard = table.enter_frame().unwrap();
        heap.alloc_host_ref(
            guard
                .borrow_shared(HostObjectId(object_id), TypeId::new(0))
                .unwrap(),
        )
        .unwrap()
    }

    fn unique_borrow_value(heap: &GcHeap, object_id: u64) -> Value {
        let table = HostBorrowTable::default();
        let guard = table.enter_frame().unwrap();
        heap.alloc_host_mut(
            guard
                .borrow_unique(HostObjectId(object_id), TypeId::new(0))
                .unwrap(),
        )
        .unwrap()
    }

    #[test]
    fn classifies_storable_and_ephemeral_value_categories() {
        let temporary = Value::RuntimeEphemeral(EphemeralValueId(1));
        assert!(!temporary.has_representation(ValueType::HostHandle));
        assert!(!temporary.has_representation(ValueType::Generic));

        let runtime = crate::Runtime::default();
        let heap = runtime.gc();
        let scalar = Value::I32(1);
        let host_root = heap.alloc_host_root(host_root(7)).unwrap();
        let path_view = path_view_value(heap, 3);
        let host_ref = shared_borrow_value(heap, 9);
        let host_mut = unique_borrow_value(heap, 10);

        assert_eq!(Value::Unit.category(), ValueCategory::Unit);
        assert_eq!(scalar.category(), ValueCategory::Primitive);
        assert_eq!(host_root.category(), ValueCategory::HostHandle);
        assert_eq!(path_view.category(), ValueCategory::HostPathView);
        assert_eq!(host_ref.category(), ValueCategory::Ephemeral);
        assert_eq!(host_mut.category(), ValueCategory::Ephemeral);

        assert!(scalar.is_storable(heap));
        assert!(!host_root.is_storable(heap));
        assert!(!path_view.is_storable(heap));
        assert!(!host_ref.is_storable(heap));
        assert!(!host_mut.is_storable(heap));
        assert!(host_ref.contains_ephemeral(heap));
        assert!(host_mut.contains_host_borrow(heap));
        assert!(!heap.alloc_tuple(vec![host_ref]).unwrap().is_storable(heap));
    }

    #[test]
    fn keeps_host_handles_out_of_default_heap_payloads() {
        let mut runtime = crate::Runtime::default();
        let interface = crate::test_support::layouts::interface_value(&mut runtime);
        let heap = runtime.gc();
        assert!(
            heap.alloc_tuple(vec![Value::Unit])
                .unwrap()
                .is_default_heap_payload(heap)
        );
        assert!(interface.is_default_heap_payload(heap));
        assert!(
            !heap
                .alloc_host_root(host_root(1))
                .unwrap()
                .is_default_heap_payload(heap)
        );
        assert!(!path_view_value(heap, 1).is_default_heap_payload(heap));
        assert!(!shared_borrow_value(heap, 1).is_default_heap_payload(heap));
        assert!(
            !heap
                .alloc_tuple(vec![unique_borrow_value(heap, 1)])
                .unwrap()
                .is_default_heap_payload(heap)
        );
    }
}
