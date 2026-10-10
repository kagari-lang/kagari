use crate::{
    gc::{GcHeap, GcObjectKind, HeapObjectId},
    host::{FrameHostBorrowToken, HostRegistryId},
    module::EnumVariantRef,
    value_semantics,
};
use kagari_abi::representation::ValueType;
use kagari_common::identity::table::DefinitionId;
use kagari_contract::representation::semantic_representation;
use kagari_types::ty::Ty;
use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
    slice,
};

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
}

/// Prepared, immutable key. The original value is retained and traced by the heap.
#[derive(Debug, Clone)]
pub struct MapKey {
    parts: KeyParts,
    value: Value,
    custom: Option<(i64, i64)>,
}

#[derive(Debug, Clone)]
enum KeyParts {
    Empty,
    Single(KeyPart),
    Aggregate(Box<[KeyPart]>),
}

impl KeyParts {
    fn as_slice(&self) -> &[KeyPart] {
        match self {
            Self::Empty => &[],
            Self::Single(part) => slice::from_ref(part),
            Self::Aggregate(parts) => parts,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum KeyPart {
    Unit,
    Bool(bool),
    I32(i32),
    I64(i64),
    U64(u64),
    Str(String),
    Tuple(usize),
    DeclaredEnum(
        HostRegistryId,
        DefinitionId,
        Vec<Ty<DefinitionId>>,
        DefinitionId,
    ),
    Identity(u8, HeapObjectId),
}

impl PartialEq for MapKey {
    fn eq(&self, other: &Self) -> bool {
        self.custom == other.custom && self.parts.as_slice() == other.parts.as_slice()
    }
}

impl Eq for MapKey {}

impl Hash for MapKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        if let Some((hash, _)) = self.custom {
            Hash::hash(&hash, state);
        } else {
            Hash::hash(self.parts.as_slice(), state);
        }
    }
}

impl MapKey {
    pub(crate) fn custom(hash: i64, token: i64, value: Value) -> Self {
        Self {
            parts: KeyParts::Empty,
            value,
            custom: Some((hash, token)),
        }
    }

    pub(crate) fn custom_parts(&self) -> Option<(i64, i64)> {
        self.custom
    }

    pub fn from_value(gc: &GcHeap, value: &Value) -> Option<Self> {
        if !gc.validate_value(value) {
            return None;
        }
        let single = match value {
            Value::Unit => Some(KeyPart::Unit),
            Value::Bool(value) => Some(KeyPart::Bool(*value)),
            Value::I32(value) => Some(KeyPart::I32(*value)),
            Value::I64(value) => Some(KeyPart::I64(*value)),
            Value::U64(value) => Some(KeyPart::U64(*value)),
            Value::Str(value) => Some(KeyPart::Str(gc.string(*value)?.to_owned())),
            Value::Struct(id) => Some(KeyPart::Identity(0, *id)),
            Value::Array(id) => Some(KeyPart::Identity(1, *id)),
            Value::Map(id) => Some(KeyPart::Identity(2, *id)),
            Value::Set(id) => Some(KeyPart::Identity(3, *id)),
            Value::GcHandle(id) if gc.object_kind(*id) == Some(GcObjectKind::Native) => {
                Some(KeyPart::Identity(4, *id))
            }
            _ => None,
        };
        if let Some(part) = single {
            return Some(Self {
                parts: KeyParts::Single(part),
                custom: None,
                value: *value,
            });
        }
        let mut pending = vec![*value];
        let mut parts = Vec::new();
        while let Some(value) = pending.pop() {
            if parts.len() >= 65536 || !gc.validate_value(&value) {
                return None;
            }
            match value {
                Value::Interface(_) => pending.push(value_semantics::collection_data(gc, &value)?),
                Value::Unit => parts.push(KeyPart::Unit),
                Value::Bool(v) => parts.push(KeyPart::Bool(v)),
                Value::I32(v) => parts.push(KeyPart::I32(v)),
                Value::I64(v) => parts.push(KeyPart::I64(v)),
                Value::U64(v) => parts.push(KeyPart::U64(v)),
                Value::Str(v) => parts.push(KeyPart::Str(gc.string(v)?.to_owned())),
                Value::Tuple(id) => {
                    let values = gc.tuple(id)?;
                    parts.push(KeyPart::Tuple(values.len()));
                    pending.extend(values.iter().rev().copied());
                }
                Value::Enum(id) => {
                    let snapshot = gc.enum_view(id)?;
                    parts.push(match &snapshot.tag {
                        EnumTag::Declared(r) => KeyPart::DeclaredEnum(
                            r.registry_owner(),
                            r.layout().declaration,
                            r.layout().arguments.clone(),
                            r.variant().declaration,
                        ),
                    });
                    parts.push(KeyPart::Tuple(snapshot.fields.len()));
                    pending.extend(snapshot.fields.iter().rev().copied());
                }
                Value::Struct(id) => parts.push(KeyPart::Identity(0, id)),
                Value::Array(id) => parts.push(KeyPart::Identity(1, id)),
                Value::Map(id) => parts.push(KeyPart::Identity(2, id)),
                Value::Set(id) => parts.push(KeyPart::Identity(3, id)),
                Value::GcHandle(id) if gc.object_kind(id) == Some(GcObjectKind::Native) => {
                    parts.push(KeyPart::Identity(4, id))
                }
                _ => return None,
            }
        }
        Some(Self {
            parts: KeyParts::Aggregate(parts.into_boxed_slice()),
            custom: None,
            value: *value,
        })
    }

    pub fn to_value(&self) -> Value {
        self.value
    }

    pub(crate) fn value(&self) -> &Value {
        &self.value
    }

    pub fn script_hash(&self) -> i64 {
        let mut hasher = DefaultHasher::new();
        self.hash(&mut hasher);
        hasher.finish() as i64
    }
}

/// Compact unrooted transport. Heap-backed payloads are accessed only through
/// their owning heap; copying a value never copies text or establishes a root.
/// Rust equality compares transport tags and IDs. Use `value_semantics::script_equal`
/// for language equality, including string contents and tuple members.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Value {
    Unit,
    Bool(bool),
    I32(i32),
    I64(i64),
    U64(u64),
    F32(f32),
    F64(f64),
    Str(HeapObjectId),
    Tuple(HeapObjectId),
    Range(HeapObjectId),
    Array(HeapObjectId),
    Map(HeapObjectId),
    Set(HeapObjectId),
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
            | Self::Map(id)
            | Self::Set(id)
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
                        | Self::Map(_)
                        | Self::Set(_)
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
            | Self::I32(_)
            | Self::I64(_)
            | Self::U64(_)
            | Self::F32(_)
            | Self::F64(_)
            | Self::Str(_) => ValueCategory::Primitive,
            Self::Range(_)
            | Self::Tuple(_)
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
        host::{
            DynamicPathArguments, HostBorrowTable, HostObjectId, HostPathDescriptorRegistration,
            HostPathSegmentRegistration, HostRootHandle, HostSchemaEpoch, HostTypeRegistration,
        },
        metadata::{AbiFingerprint, TypeId},
    };
    use kagari_types::host_interface::{
        type_declaration::{
            HostFieldDeclaration, HostTypeDeclaration, HostTypeOwnership, PathAccess,
        },
        value_type::HostValueType,
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

    fn path_view_value(heap: &GcHeap, object_id: u64) -> Value {
        let result_type = TypeId::new(1);
        let mut runtime = crate::Runtime::default();
        let mut declaration = HostTypeDeclaration::new("Player");
        declaration.ownership = HostTypeOwnership::HostRoot;
        declaration.path_access = PathAccess::ReadWrite;
        let mut hp = HostFieldDeclaration::new(&declaration.id, "hp", HostValueType::I32);
        hp.writable = true;
        hp.path_access = PathAccess::ReadWrite;
        declaration.fields.push(hp);
        let root_type = runtime
            .register_host_type(HostTypeRegistration::new(declaration, "Player"))
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
            })
            .unwrap();
        heap.alloc_host_path(
            runtime
                .host()
                .make_path_view(
                    runtime.gc(),
                    root,
                    descriptor,
                    DynamicPathArguments::empty(),
                )
                .unwrap(),
        )
        .unwrap()
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
        let interface = crate::layout_fixtures::interface_value(&mut runtime);
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

    #[test]
    fn keys_share_value_equality_and_preserve_original_values() {
        let gc = crate::gc::GcHeap::new(Default::default(), Default::default());
        for value in [
            Value::Unit,
            Value::Bool(true),
            Value::I32(7),
            Value::I64(9),
            gc.alloc_string("hp".into()).unwrap(),
            gc.alloc_tuple(vec![Value::I32(1)]).unwrap(),
        ] {
            let a = MapKey::from_value(&gc, &value).unwrap();
            let b = MapKey::from_value(&gc, &value).unwrap();
            assert_eq!(a, b);
            assert_eq!(a.script_hash(), b.script_hash());
            assert_eq!(a.to_value(), value);
        }
        let first = gc.alloc_string("é🙂".into()).unwrap();
        let second = gc.alloc_string("é🙂".into()).unwrap();
        assert_ne!(first, second);
        let first_key = MapKey::from_value(&gc, &first).unwrap();
        let second_key = MapKey::from_value(&gc, &second).unwrap();
        assert_eq!(first_key, second_key);
        assert_eq!(first_key.script_hash(), second_key.script_hash());
        assert!(MapKey::from_value(&gc, &Value::F64(1.0)).is_none());
    }
}
