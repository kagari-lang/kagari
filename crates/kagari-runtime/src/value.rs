use crate::gc::HeapObjectId;
use crate::host::{FrameHostBorrowToken, HostObjectId};

#[derive(Debug, Clone, PartialEq)]
pub struct StructValueField {
    pub name: String,
    pub value: Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct InterfaceObjectId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct HostPathViewId(pub u64);

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

#[derive(Debug, Clone, PartialEq)]
pub enum EphemeralValue {
    HostRef(FrameHostBorrowToken),
    HostMut(FrameHostBorrowToken),
    Runtime(EphemeralValueId),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Unit,
    Bool(bool),
    I32(i32),
    I64(i64),
    F32(f32),
    F64(f64),
    Str(String),
    Tuple(Vec<Value>),
    Array(HeapObjectId),
    Struct(HeapObjectId),
    GcHandle(HeapObjectId),
    Interface(InterfaceObjectId),
    HostOwned(HostObjectId),
    HostPathView(HostPathViewId),
    Ephemeral(EphemeralValue),
}

impl Value {
    pub fn category(&self) -> ValueCategory {
        match self {
            Self::Unit => ValueCategory::Unit,
            Self::Bool(_)
            | Self::I32(_)
            | Self::I64(_)
            | Self::F32(_)
            | Self::F64(_)
            | Self::Str(_) => ValueCategory::Primitive,
            Self::Tuple(_) | Self::Array(_) | Self::Struct(_) | Self::GcHandle(_) => {
                ValueCategory::ScriptOwned
            }
            Self::Interface(_) => ValueCategory::Interface,
            Self::HostOwned(_) => ValueCategory::HostHandle,
            Self::HostPathView(_) => ValueCategory::HostPathView,
            Self::Ephemeral(_) => ValueCategory::Ephemeral,
        }
    }

    pub fn is_storable(&self) -> bool {
        match self {
            Self::Tuple(elements) => elements.iter().all(Self::is_storable),
            Self::Ephemeral(_) => false,
            _ => true,
        }
    }

    pub fn is_ephemeral(&self) -> bool {
        matches!(self, Self::Ephemeral(_))
    }

    pub fn contains_ephemeral(&self) -> bool {
        match self {
            Self::Tuple(elements) => elements.iter().any(Self::contains_ephemeral),
            Self::Ephemeral(_) => true,
            _ => false,
        }
    }

    pub fn contains_host_borrow(&self) -> bool {
        match self {
            Self::Tuple(elements) => elements.iter().any(Self::contains_host_borrow),
            Self::Ephemeral(EphemeralValue::HostRef(_) | EphemeralValue::HostMut(_)) => true,
            _ => false,
        }
    }

    pub fn is_default_heap_payload(&self) -> bool {
        match self {
            Self::Unit
            | Self::Bool(_)
            | Self::I32(_)
            | Self::I64(_)
            | Self::F32(_)
            | Self::F64(_)
            | Self::Str(_) => true,
            Self::Tuple(elements) => elements.iter().all(Self::is_default_heap_payload),
            Self::Array(_) | Self::Struct(_) | Self::GcHandle(_) | Self::Interface(_) => true,
            Self::HostOwned(_) | Self::HostPathView(_) | Self::Ephemeral(_) => false,
        }
    }

    pub fn host_ref(token: FrameHostBorrowToken) -> Self {
        Self::Ephemeral(EphemeralValue::HostRef(token))
    }

    pub fn host_mut(token: FrameHostBorrowToken) -> Self {
        Self::Ephemeral(EphemeralValue::HostMut(token))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        host::{HostBorrowTable, HostObjectId},
        metadata::TypeId,
    };

    fn shared_borrow_value(object_id: u64) -> Value {
        let table = HostBorrowTable::default();
        let guard = table.enter_frame();
        Value::host_ref(
            guard
                .borrow_shared(HostObjectId(object_id), TypeId::new(0))
                .unwrap(),
        )
    }

    fn unique_borrow_value(object_id: u64) -> Value {
        let table = HostBorrowTable::default();
        let guard = table.enter_frame();
        Value::host_mut(
            guard
                .borrow_unique(HostObjectId(object_id), TypeId::new(0))
                .unwrap(),
        )
    }

    #[test]
    fn classifies_storable_and_ephemeral_value_categories() {
        let scalar = Value::I32(1);
        let host_root = Value::HostOwned(HostObjectId(7));
        let path_view = Value::HostPathView(HostPathViewId(3));
        let host_ref = shared_borrow_value(9);
        let host_mut = unique_borrow_value(10);

        assert_eq!(Value::Unit.category(), ValueCategory::Unit);
        assert_eq!(scalar.category(), ValueCategory::Primitive);
        assert_eq!(host_root.category(), ValueCategory::HostHandle);
        assert_eq!(path_view.category(), ValueCategory::HostPathView);
        assert_eq!(host_ref.category(), ValueCategory::Ephemeral);
        assert_eq!(host_mut.category(), ValueCategory::Ephemeral);

        assert!(scalar.is_storable());
        assert!(host_root.is_storable());
        assert!(path_view.is_storable());
        assert!(!host_ref.is_storable());
        assert!(!host_mut.is_storable());
        assert!(host_ref.contains_ephemeral());
        assert!(host_mut.contains_host_borrow());
        assert!(!Value::Tuple(vec![host_ref]).is_storable());
    }

    #[test]
    fn keeps_host_handles_out_of_default_heap_payloads() {
        assert!(Value::Tuple(vec![Value::Unit]).is_default_heap_payload());
        assert!(Value::Interface(InterfaceObjectId(1)).is_default_heap_payload());
        assert!(!Value::HostOwned(HostObjectId(1)).is_default_heap_payload());
        assert!(!Value::HostPathView(HostPathViewId(1)).is_default_heap_payload());
        assert!(!shared_borrow_value(1).is_default_heap_payload());
        assert!(!Value::Tuple(vec![unique_borrow_value(1)]).is_default_heap_payload());
    }
}
