//! Structural builtin value hashing, independent of collection key/table policy.
use crate::{
    gc::GcHeap,
    host::HostRegistryId,
    value::{EnumTag, Value},
};
use kagari_common::identity::table::DefinitionId;
use kagari_types::ty::Ty;
use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
    slice,
};

#[derive(Debug, Clone)]
pub(crate) struct StructuralKey {
    parts: KeyParts,
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
    Char(char),
    I32(i32),
    I64(i64),
    U64(u64),
    Str(String),
    Tuple(usize),
    DeclaredEnum(HostRegistryId, DefinitionId, Vec<Ty>, DefinitionId),
}

impl PartialEq for StructuralKey {
    fn eq(&self, other: &Self) -> bool {
        self.parts.as_slice() == other.parts.as_slice()
    }
}

impl Eq for StructuralKey {}

impl Hash for StructuralKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.parts.as_slice().hash(state);
    }
}

impl StructuralKey {
    pub(crate) fn empty() -> Self {
        Self {
            parts: KeyParts::Empty,
        }
    }

    pub(crate) fn from_value(gc: &GcHeap, value: &Value) -> Option<Self> {
        if !gc.validate_value(value) {
            return None;
        }
        let single = match value {
            Value::Unit => Some(KeyPart::Unit),
            Value::Bool(value) => Some(KeyPart::Bool(*value)),
            Value::Char(value) => Some(KeyPart::Char(*value)),
            Value::I32(value) => Some(KeyPart::I32(*value)),
            Value::I64(value) => Some(KeyPart::I64(*value)),
            Value::U64(value) => Some(KeyPart::U64(*value)),
            Value::Str(value) => Some(KeyPart::Str(gc.string(*value)?.to_owned())),
            _ => None,
        };
        if let Some(part) = single {
            return Some(Self {
                parts: KeyParts::Single(part),
            });
        }
        let mut pending = vec![*value];
        let mut parts = Vec::new();
        while let Some(value) = pending.pop() {
            if parts.len() >= 65536 || !gc.validate_value(&value) {
                return None;
            }
            match value {
                Value::Interface(_) => return None,
                Value::Unit => parts.push(KeyPart::Unit),
                Value::Bool(v) => parts.push(KeyPart::Bool(v)),
                Value::Char(v) => parts.push(KeyPart::Char(v)),
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
                        EnumTag::Declared(r) => {
                            let (nominal, variant) = r.value_identity()?;
                            KeyPart::DeclaredEnum(
                                r.registry_owner(),
                                nominal.declaration,
                                nominal.arguments,
                                variant,
                            )
                        }
                    });
                    parts.push(KeyPart::Tuple(snapshot.fields.len()));
                    pending.extend(snapshot.fields.iter().rev().copied());
                }
                _ => return None,
            }
        }
        Some(Self {
            parts: KeyParts::Aggregate(parts.into_boxed_slice()),
        })
    }
}

pub(crate) fn builtin_hash(gc: &GcHeap, value: &Value) -> Option<i64> {
    let key = StructuralKey::from_value(gc, value)?;
    let mut hasher = DefaultHasher::new();
    key.hash(&mut hasher);
    Some(hasher.finish() as i64)
}
