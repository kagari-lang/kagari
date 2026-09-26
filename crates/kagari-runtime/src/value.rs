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
    OrderingLess,
    OrderingEqual,
    OrderingGreater,
    OptionSome,
    OptionNone,
    ResultOk,
    ResultErr,
    Declared(crate::module::EnumVariantRef),
}

impl EnumTag {
    pub fn type_name(&self) -> &str {
        match self {
            Self::OrderingLess | Self::OrderingEqual | Self::OrderingGreater => "Ordering",
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
            Self::OrderingLess => "Less",
            Self::OrderingEqual => "Equal",
            Self::OrderingGreater => "Greater",
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
            Self::OptionNone | Self::OrderingLess | Self::OrderingEqual | Self::OrderingGreater => {
                fields.is_empty()
            }
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

/// Prepared, immutable key. The original value is retained and traced by the heap.
#[derive(Debug, Clone)]
pub struct MapKey {
    parts: Vec<KeyPart>,
    value: Value,
    custom: Option<(i64, i64)>,
}
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum KeyPart {
    Unit,
    Bool(bool),
    I32(i32),
    I64(i64),
    Str(String),
    Tuple(usize),
    StandardEnum(u8),
    DeclaredEnum(
        crate::host::HostRegistryId,
        kagari_common::identity::DefinitionId,
        Vec<kagari_ir::module::abi::AbiType>,
        kagari_common::identity::DefinitionId,
    ),
    Identity(u8, HeapObjectId),
}
impl PartialEq for MapKey {
    fn eq(&self, other: &Self) -> bool {
        self.custom == other.custom && self.parts == other.parts
    }
}
impl Eq for MapKey {}
impl std::hash::Hash for MapKey {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        if let Some((hash, _)) = self.custom {
            std::hash::Hash::hash(&hash, state);
        } else {
            std::hash::Hash::hash(&self.parts, state);
        }
    }
}
impl MapKey {
    pub(crate) fn custom(hash: i64, token: i64, value: Value) -> Self {
        Self {
            parts: vec![],
            value,
            custom: Some((hash, token)),
        }
    }
    pub(crate) fn custom_parts(&self) -> Option<(i64, i64)> {
        self.custom
    }

    pub fn from_value(gc: &crate::gc::GcHeap, value: &Value) -> Option<Self> {
        let mut pending = vec![value.clone()];
        let mut parts = Vec::new();
        while let Some(value) = pending.pop() {
            if parts.len() >= 65536 || !gc.validate_value(&value) {
                return None;
            }
            match value {
                Value::Unit => parts.push(KeyPart::Unit),
                Value::Bool(v) => parts.push(KeyPart::Bool(v)),
                Value::I32(v) => parts.push(KeyPart::I32(v)),
                Value::I64(v) => parts.push(KeyPart::I64(v)),
                Value::Str(v) => parts.push(KeyPart::Str(v)),
                Value::Tuple(values) => {
                    parts.push(KeyPart::Tuple(values.len()));
                    pending.extend(values.into_iter().rev());
                }
                Value::Enum(id) => {
                    let snapshot = gc.enum_snapshot(id)?;
                    parts.push(match snapshot.tag {
                        EnumTag::OrderingLess => KeyPart::StandardEnum(4),
                        EnumTag::OrderingEqual => KeyPart::StandardEnum(5),
                        EnumTag::OrderingGreater => KeyPart::StandardEnum(6),
                        EnumTag::OptionNone => KeyPart::StandardEnum(0),
                        EnumTag::OptionSome => KeyPart::StandardEnum(1),
                        EnumTag::ResultOk => KeyPart::StandardEnum(2),
                        EnumTag::ResultErr => KeyPart::StandardEnum(3),
                        EnumTag::Declared(ref r) => KeyPart::DeclaredEnum(
                            r.registry_owner(),
                            r.layout().declaration.clone(),
                            r.layout().arguments.clone(),
                            r.variant().declaration.clone(),
                        ),
                    });
                    parts.push(KeyPart::Tuple(snapshot.fields.len()));
                    pending.extend(snapshot.fields.into_iter().rev());
                }
                Value::Struct(id) => parts.push(KeyPart::Identity(0, id)),
                Value::Array(id) => parts.push(KeyPart::Identity(1, id)),
                Value::Map(id) => parts.push(KeyPart::Identity(2, id)),
                Value::Set(id) => parts.push(KeyPart::Identity(3, id)),
                _ => return None,
            }
        }
        Some(Self {
            parts,
            custom: None,
            value: value.clone(),
        })
    }
    pub fn to_value(&self) -> Value {
        self.value.clone()
    }
    pub(crate) fn value(&self) -> &Value {
        &self.value
    }
    pub fn script_hash(&self) -> i64 {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        self.hash(&mut hasher);
        hasher.finish() as i64
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
    Closure(HeapObjectId),
    Cell(HeapObjectId),
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
                        | Self::Interface(_)
                        | Self::Closure(_)
                        | Self::Cell(_),
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
            Self::Closure(_) => ValueCategory::ScriptOwned,
            Self::Cell(_) => ValueCategory::ScriptOwned,
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
            Self::Closure(_) => true,
            Self::Cell(_) => true,
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
    fn keys_share_value_equality_and_preserve_original_values() {
        let gc = crate::gc::GcHeap::new(Default::default(), Default::default());
        for value in [
            Value::Unit,
            Value::Bool(true),
            Value::I32(7),
            Value::I64(9),
            Value::Str("hp".into()),
            Value::Tuple(vec![Value::I32(1)]),
        ] {
            let a = MapKey::from_value(&gc, &value).unwrap();
            let b = MapKey::from_value(&gc, &value).unwrap();
            assert_eq!(a, b);
            assert_eq!(a.script_hash(), b.script_hash());
            assert_eq!(a.to_value(), value);
        }
        assert!(MapKey::from_value(&gc, &Value::F64(1.0)).is_none());
    }
}
