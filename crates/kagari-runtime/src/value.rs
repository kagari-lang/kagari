use crate::gc::HeapObjectId;
use crate::host::HostObjectId;

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
    HostRef(HostObjectId),
    HostMut(HostObjectId),
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
        !self.is_ephemeral()
    }

    pub fn is_ephemeral(&self) -> bool {
        matches!(self, Self::Ephemeral(_))
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

    pub fn host_ref(id: HostObjectId) -> Self {
        Self::Ephemeral(EphemeralValue::HostRef(id))
    }

    pub fn host_mut(id: HostObjectId) -> Self {
        Self::Ephemeral(EphemeralValue::HostMut(id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_storable_and_ephemeral_value_categories() {
        let scalar = Value::I32(1);
        let host_root = Value::HostOwned(HostObjectId(7));
        let path_view = Value::HostPathView(HostPathViewId(3));
        let host_ref = Value::host_ref(HostObjectId(9));
        let host_mut = Value::host_mut(HostObjectId(10));

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
    }

    #[test]
    fn keeps_host_handles_out_of_default_heap_payloads() {
        assert!(Value::Tuple(vec![Value::Unit]).is_default_heap_payload());
        assert!(Value::Interface(InterfaceObjectId(1)).is_default_heap_payload());
        assert!(!Value::HostOwned(HostObjectId(1)).is_default_heap_payload());
        assert!(!Value::HostPathView(HostPathViewId(1)).is_default_heap_payload());
        assert!(!Value::host_ref(HostObjectId(1)).is_default_heap_payload());
        assert!(!Value::Tuple(vec![Value::host_mut(HostObjectId(1))]).is_default_heap_payload());
    }
}
