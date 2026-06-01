use crate::gc::HeapObjectId;
use crate::host::{FrameHostBorrowToken, HostPathViewHandle, HostRootHandle};

#[derive(Debug, Clone, PartialEq)]
pub struct StructValueField {
    pub name: String,
    pub value: Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct InterfaceObjectId(pub u64);

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

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum MapKey {
    Bool(bool),
    I32(i32),
    I64(i64),
    Str(String),
}

impl MapKey {
    pub fn from_value(value: &Value) -> Option<Self> {
        match value {
            Value::Bool(value) => Some(Self::Bool(*value)),
            Value::I32(value) => Some(Self::I32(*value)),
            Value::I64(value) => Some(Self::I64(*value)),
            Value::Str(value) => Some(Self::Str(value.clone())),
            _ => None,
        }
    }

    pub fn to_value(&self) -> Value {
        match self {
            Self::Bool(value) => Value::Bool(*value),
            Self::I32(value) => Value::I32(*value),
            Self::I64(value) => Value::I64(*value),
            Self::Str(value) => Value::Str(value.clone()),
        }
    }
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
    Map(HeapObjectId),
    Set(HeapObjectId),
    Struct(HeapObjectId),
    GcHandle(HeapObjectId),
    Interface(InterfaceObjectId),
    HostRoot(HostRootHandle),
    HostPathView(HostPathViewHandle),
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
            Self::Tuple(_)
            | Self::Array(_)
            | Self::Map(_)
            | Self::Set(_)
            | Self::Struct(_)
            | Self::GcHandle(_) => ValueCategory::ScriptOwned,
            Self::Interface(_) => ValueCategory::Interface,
            Self::HostRoot(_) => ValueCategory::HostHandle,
            Self::HostPathView(_) => ValueCategory::HostPathView,
            Self::Ephemeral(_) => ValueCategory::Ephemeral,
        }
    }

    pub fn is_storable(&self) -> bool {
        match self {
            Self::Tuple(elements) => elements.iter().all(Self::is_storable),
            Self::HostRoot(_) | Self::HostPathView(_) | Self::Ephemeral(_) => false,
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
            Self::Array(_)
            | Self::Map(_)
            | Self::Set(_)
            | Self::Struct(_)
            | Self::GcHandle(_)
            | Self::Interface(_) => true,
            Self::HostRoot(_) | Self::HostPathView(_) | Self::Ephemeral(_) => false,
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
        host::{
            DynamicPathArguments, HostBorrowTable, HostObjectId, HostPathDescriptorRegistration,
            HostPathSegment, HostRegistry, HostRootHandle, HostSchemaEpoch, HostTypeInfo,
            HostTypeOwnership,
        },
        metadata::{AbiFingerprint, FieldMetadataId, PathAccess, TypeId},
    };

    fn host_root(object_id: u64) -> HostRootHandle {
        HostRootHandle::new(
            HostObjectId(object_id),
            TypeId::new(0),
            HostSchemaEpoch::new(0),
            AbiFingerprint(1),
        )
    }

    fn path_view_value(object_id: u64) -> Value {
        let root_type = TypeId::new(0);
        let result_type = TypeId::new(1);
        let mut registry = HostRegistry::default();
        registry
            .register_type(HostTypeInfo {
                type_id: root_type,
                script_name: "Player".to_owned(),
                rust_type_name: "Player".to_owned(),
                ownership: HostTypeOwnership::HostRoot,
                fields: Vec::new(),
                methods: Vec::new(),
                traits: Vec::new(),
                path_access: PathAccess::ReadWrite,
                reflection: crate::host::HostReflectionPolicy::Hidden,
                abi_fingerprint: AbiFingerprint(1),
            })
            .unwrap();
        let root = registry
            .register_root(HostObjectId(object_id), root_type, HostSchemaEpoch::new(0))
            .unwrap();
        let descriptor = registry
            .register_path_descriptor(HostPathDescriptorRegistration {
                root_type,
                result_type,
                segments: vec![HostPathSegment::Field {
                    name: "hp".to_owned(),
                    field_id: FieldMetadataId::new(0),
                    owner_type: root_type,
                    result_type,
                    access: PathAccess::ReadWrite,
                    abi_fingerprint: AbiFingerprint(2),
                }],
                access: PathAccess::ReadWrite,
                schema_epoch: HostSchemaEpoch::new(0),
                abi_fingerprint: AbiFingerprint(3),
                capability_requirements: crate::security::CapabilitySet::default(),
            })
            .unwrap();
        Value::HostPathView(
            registry
                .make_path_view(root, descriptor, DynamicPathArguments::empty())
                .unwrap(),
        )
    }

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
        let host_root = Value::HostRoot(host_root(7));
        let path_view = path_view_value(3);
        let host_ref = shared_borrow_value(9);
        let host_mut = unique_borrow_value(10);

        assert_eq!(Value::Unit.category(), ValueCategory::Unit);
        assert_eq!(scalar.category(), ValueCategory::Primitive);
        assert_eq!(
            Value::Map(HeapObjectId::new(1)).category(),
            ValueCategory::ScriptOwned
        );
        assert_eq!(
            Value::Set(HeapObjectId::new(2)).category(),
            ValueCategory::ScriptOwned
        );
        assert_eq!(host_root.category(), ValueCategory::HostHandle);
        assert_eq!(path_view.category(), ValueCategory::HostPathView);
        assert_eq!(host_ref.category(), ValueCategory::Ephemeral);
        assert_eq!(host_mut.category(), ValueCategory::Ephemeral);

        assert!(scalar.is_storable());
        assert!(!host_root.is_storable());
        assert!(!path_view.is_storable());
        assert!(!host_ref.is_storable());
        assert!(!host_mut.is_storable());
        assert!(host_ref.contains_ephemeral());
        assert!(host_mut.contains_host_borrow());
        assert!(!Value::Tuple(vec![host_ref]).is_storable());
    }

    #[test]
    fn keeps_host_handles_out_of_default_heap_payloads() {
        assert!(Value::Tuple(vec![Value::Unit]).is_default_heap_payload());
        assert!(Value::Map(HeapObjectId::new(1)).is_default_heap_payload());
        assert!(Value::Set(HeapObjectId::new(2)).is_default_heap_payload());
        assert!(Value::Interface(InterfaceObjectId(1)).is_default_heap_payload());
        assert!(!Value::HostRoot(host_root(1)).is_default_heap_payload());
        assert!(!path_view_value(1).is_default_heap_payload());
        assert!(!shared_borrow_value(1).is_default_heap_payload());
        assert!(!Value::Tuple(vec![unique_borrow_value(1)]).is_default_heap_payload());
    }

    #[test]
    fn maps_standard_hash_key_values() {
        assert_eq!(
            MapKey::from_value(&Value::Bool(true)),
            Some(MapKey::Bool(true))
        );
        assert_eq!(MapKey::from_value(&Value::I32(7)), Some(MapKey::I32(7)));
        assert_eq!(MapKey::from_value(&Value::I64(9)), Some(MapKey::I64(9)));
        assert_eq!(
            MapKey::from_value(&Value::Str("hp".to_owned())),
            Some(MapKey::Str("hp".to_owned()))
        );
        assert_eq!(
            MapKey::Str("name".to_owned()).to_value(),
            Value::Str("name".to_owned())
        );
        assert!(MapKey::from_value(&Value::F64(1.0)).is_none());
        assert!(MapKey::from_value(&Value::Tuple(vec![])).is_none());
    }
}
