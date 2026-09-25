use crate::gc::HeapObjectId;
use crate::host::{FrameHostBorrowToken, HostPathViewHandle, HostRootHandle};

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
    OptionSome,
    OptionNone,
    ResultOk,
    ResultErr,
    Declared(crate::module::EnumVariantRef),
}

impl EnumTag {
    pub fn type_name(&self) -> &str {
        match self {
            Self::OptionSome | Self::OptionNone => "Option",
            Self::ResultOk | Self::ResultErr => "Result",
            Self::Declared(layout) => {
                &layout
                    .layout()
                    .declaration
                    .path
                    .last()
                    .expect("enum declaration")
                    .name
            }
        }
    }
    pub fn variant_name(&self) -> &str {
        match self {
            Self::OptionSome => "Some",
            Self::OptionNone => "None",
            Self::ResultOk => "Ok",
            Self::ResultErr => "Err",
            Self::Declared(layout) => {
                &layout
                    .variant()
                    .declaration
                    .path
                    .last()
                    .expect("variant declaration")
                    .name
            }
        }
    }
    pub(crate) fn accepts_representations(&self, fields: &[Value]) -> bool {
        match self {
            Self::OptionNone => fields.is_empty(),
            Self::OptionSome | Self::ResultOk | Self::ResultErr => fields.len() == 1,
            Self::Declared(layout) => {
                fields.len() == layout.variant().payload.len()
                    && fields
                        .iter()
                        .zip(&layout.variant().payload)
                        .all(|(value, ty)| value.has_representation(ty.representation()))
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
    Enum(HeapObjectId),
    Struct(HeapObjectId),
    GcHandle(HeapObjectId),
    Interface(InterfaceObjectId),
    HostRoot(HostRootHandle),
    HostPathView(HostPathViewHandle),
    Ephemeral(EphemeralValue),
}

impl Value {
    pub fn has_representation(&self, ty: kagari_ir::module::ValueType) -> bool {
        use kagari_ir::module::ValueType as T;
        matches!(
            (self, ty),
            (Self::Unit, T::Unit)
                | (Self::Bool(_), T::Bool)
                | (Self::I32(_), T::I32)
                | (Self::I64(_), T::I64)
                | (Self::F32(_), T::F32)
                | (Self::F64(_), T::F64)
                | (Self::Str(_), T::Str)
                | (
                    Self::HostRoot(_)
                        | Self::HostPathView(_)
                        | Self::Ephemeral(EphemeralValue::HostRef(_) | EphemeralValue::HostMut(_)),
                    T::HostHandle
                )
                | (
                    Self::Tuple(_)
                        | Self::Array(_)
                        | Self::Map(_)
                        | Self::Set(_)
                        | Self::Enum(_)
                        | Self::Struct(_)
                        | Self::GcHandle(_)
                        | Self::Interface(_),
                    T::HeapObject
                )
        )
    }
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
            | Self::Enum(_)
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
            | Self::Enum(_)
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
            HostPathSegmentRegistration, HostRootHandle, HostSchemaEpoch, HostTypeOwnership,
        },
        metadata::{AbiFingerprint, PathAccess, TypeId},
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

    fn path_view_value(object_id: u64) -> Value {
        let result_type = TypeId::new(1);
        let mut runtime = crate::Runtime::default();
        let mut declaration = kagari_common::host_interface::HostTypeDeclaration::new("Player");
        declaration.ownership = HostTypeOwnership::HostRoot;
        declaration.path_access = PathAccess::ReadWrite;
        let mut hp = kagari_common::host_interface::HostFieldDeclaration::new(
            &declaration.id,
            "hp",
            kagari_common::host_interface::HostValueType::I32,
        );
        hp.writable = true;
        hp.path_access = PathAccess::ReadWrite;
        declaration.fields.push(hp);
        let root_type = runtime
            .register_host_type(crate::HostTypeRegistration::new(declaration, "Player"))
            .unwrap();
        let root = runtime
            .register_host_root(HostObjectId(object_id), root_type, HostSchemaEpoch::new(0))
            .unwrap();
        let descriptor = runtime
            .register_host_path_descriptor(HostPathDescriptorRegistration {
                root_type,
                result_type,
                segments: vec![HostPathSegmentRegistration::Field {
                    declaration: runtime
                        .host()
                        .host_type(root_type)
                        .unwrap()
                        .declaration
                        .fields
                        .iter()
                        .find(|field| field.name == "hp")
                        .unwrap()
                        .id
                        .clone(),
                }],
                access: PathAccess::ReadWrite,
                schema_epoch: HostSchemaEpoch::new(0),
                capability_requirements: crate::security::CapabilitySet::default(),
            })
            .unwrap();
        Value::HostPathView(
            runtime
                .host()
                .make_path_view(root, descriptor, DynamicPathArguments::empty())
                .unwrap(),
        )
    }

    fn shared_borrow_value(object_id: u64) -> Value {
        let table = HostBorrowTable::default();
        let guard = table.enter_frame().unwrap();
        Value::host_ref(
            guard
                .borrow_shared(HostObjectId(object_id), TypeId::new(0))
                .unwrap(),
        )
    }

    fn unique_borrow_value(object_id: u64) -> Value {
        let table = HostBorrowTable::default();
        let guard = table.enter_frame().unwrap();
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
            Value::Map(
                crate::gc::GcHeap::new(
                    Default::default(),
                    std::rc::Rc::new(crate::resource::ResourceState::default())
                )
                .alloc_map(Vec::new())
                .unwrap()
            )
            .category(),
            ValueCategory::ScriptOwned
        );
        assert_eq!(
            Value::Set(
                crate::gc::GcHeap::new(
                    Default::default(),
                    std::rc::Rc::new(crate::resource::ResourceState::default())
                )
                .alloc_set(Vec::new())
                .unwrap()
            )
            .category(),
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
        assert!(
            Value::Map(
                crate::gc::GcHeap::new(
                    Default::default(),
                    std::rc::Rc::new(crate::resource::ResourceState::default())
                )
                .alloc_map(Vec::new())
                .unwrap()
            )
            .is_default_heap_payload()
        );
        assert!(
            Value::Set(
                crate::gc::GcHeap::new(
                    Default::default(),
                    std::rc::Rc::new(crate::resource::ResourceState::default())
                )
                .alloc_set(Vec::new())
                .unwrap()
            )
            .is_default_heap_payload()
        );
        let mut runtime = crate::Runtime::default();
        assert!(crate::layout_fixtures::interface_value(&mut runtime).is_default_heap_payload());
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
