//! Semantic type expressions and scoped generic constraints; no execution representation.
pub mod access;
pub mod identity;
pub mod inheritance;
mod mapping;
pub mod matching;
pub mod substitution;
mod wire;
use crate::{
    collection::CollectionAccess,
    host_interface::value_type::HostValueType,
    range::RangeKind,
    scalar::BuiltinType,
    surface::{StandardEnum as StandardEnumKind, StandardTypeConstraint},
    ty::substitution::TypeSubstitution,
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{DefinitionPath, reference::DefinitionReference},
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Semantic types preserve nominal identity and container arguments.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(bound(
    serialize = "I: DefinitionReference + Serialize",
    deserialize = "I: DefinitionReference + Deserialize<'de>"
))]
pub struct NominalTy<I = DefinitionPath> {
    pub declaration: I,
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    pub arguments: Vec<Ty<I>>,
    #[serde(deserialize_with = "crate::decode_limits::map")]
    pub associated_types: BTreeMap<I, Ty<I>>,
}

impl<I: DefinitionReference> NominalTy<I> {
    /// A required view can leave outputs unspecified; specified outputs remain
    /// invariant and must equal the concrete implementation's checked outputs.
    pub fn satisfies(&self, required: &Self) -> bool {
        self.declaration == required.declaration
            && self.arguments == required.arguments
            && required
                .associated_types
                .iter()
                .all(|(member, ty)| self.associated_types.get(member) == Some(ty))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Ty<I = DefinitionPath> {
    Projection {
        arguments: Vec<Ty<I>>,
        receiver: Box<Ty<I>>,
        interface: Box<NominalTy<I>>,
        member: I,
    },
    Host(I),
    /// Receiver template in a trait signature, never an executable value layout.
    SelfType(I),
    /// Valid only in a declaration template, never in an executable layout.
    Parameter {
        owner: I,
        position: usize,
    },
    Builtin(BuiltinType),
    Tuple(Vec<Ty<I>>),
    Function {
        params: Vec<Ty<I>>,
        result: Box<Ty<I>>,
    },
    Iter(Box<Ty<I>>),
    Range(Box<Ty<I>>, RangeKind),
    Array(Box<Ty<I>>, CollectionAccess),
    Map {
        key: Box<Ty<I>>,
        value: Box<Ty<I>>,
        access: CollectionAccess,
    },
    Set(Box<Ty<I>>, CollectionAccess),
    Struct(NominalTy<I>),
    /// A nominal script-heap object backed by a registered traced Rust payload.
    NativeObject(NominalTy<I>),
    Enum(NominalTy<I>),
    Trait(NominalTy<I>),
    StandardEnum {
        kind: StandardEnumKind,
        args: Vec<Ty<I>>,
    },
}

impl<I: DefinitionReference> Ty<I> {
    pub fn contains_projection(&self) -> bool {
        let mut pending = vec![self];
        while let Some(ty) = pending.pop() {
            match ty {
                Self::Projection { .. } => return true,
                Self::Struct(ty) | Self::NativeObject(ty) | Self::Enum(ty) | Self::Trait(ty) => {
                    pending.extend(&ty.arguments);
                    pending.extend(ty.associated_types.values());
                }
                Self::Tuple(items) | Self::StandardEnum { args: items, .. } => {
                    pending.extend(items)
                }
                Self::Array(item, _)
                | Self::Set(item, _)
                | Self::Iter(item)
                | Self::Range(item, _) => pending.push(item),
                Self::Map { key, value, .. } => pending.extend([key.as_ref(), value.as_ref()]),
                Self::Function { params, result } => {
                    pending.extend(params);
                    pending.push(result);
                }
                Self::Host(_) | Self::Builtin(_) | Self::SelfType(_) | Self::Parameter { .. } => {}
            }
        }
        false
    }

    pub fn is_concrete(&self) -> bool {
        let mut pending = vec![self];
        while let Some(ty) = pending.pop() {
            match ty {
                Self::Projection { .. } | Self::Parameter { .. } | Self::SelfType(_) => {
                    return false;
                }
                Self::Tuple(types) | Self::StandardEnum { args: types, .. } => {
                    pending.extend(types)
                }
                Self::Function { params, result } => {
                    pending.extend(params);
                    pending.push(result);
                }
                Self::Array(ty, _) | Self::Set(ty, _) | Self::Iter(ty) | Self::Range(ty, _) => {
                    pending.push(ty)
                }
                Self::Map { key, value, .. } => pending.extend([key.as_ref(), value.as_ref()]),
                Self::Struct(ty) | Self::NativeObject(ty) | Self::Enum(ty) | Self::Trait(ty) => {
                    pending.extend(&ty.arguments);
                    pending.extend(ty.associated_types.values());
                }
                Self::Host(_) | Self::Builtin(_) => {}
            }
        }
        true
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GenericParam<I = DefinitionPath> {
    pub owner: I,
    pub position: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(bound(
    serialize = "I: DefinitionReference + Serialize",
    deserialize = "I: DefinitionReference + Deserialize<'de>"
))]
pub struct GenericBound<I = DefinitionPath> {
    pub ty: Ty<I>,
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    pub constraints: Vec<Constraint<I>>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(bound(
    serialize = "I: DefinitionReference + Serialize",
    deserialize = "I: DefinitionReference + Deserialize<'de>"
))]
pub enum Constraint<I = DefinitionPath> {
    Standard(StandardTypeConstraint),
    Trait(NominalTy<I>),
}

impl<I: DefinitionReference> Ty<I> {
    pub fn from_host_type(ty: &HostValueType<I>) -> Self {
        match ty {
            HostValueType::Unit => Self::Builtin(BuiltinType::Unit),
            HostValueType::Bool => Self::Builtin(BuiltinType::Bool),
            HostValueType::I32 => Self::Builtin(BuiltinType::I32),
            HostValueType::I64 => Self::Builtin(BuiltinType::I64),
            HostValueType::F32 => Self::Builtin(BuiltinType::F32),
            HostValueType::F64 => Self::Builtin(BuiltinType::F64),
            HostValueType::String => Self::Builtin(BuiltinType::String),
            HostValueType::Opaque(id) => Self::Host(id.clone()),
            HostValueType::Tuple(types) => {
                Self::Tuple(types.iter().map(Self::from_host_type).collect())
            }
            HostValueType::Array(ty, access) => {
                Self::Array(Box::new(Self::from_host_type(ty)), *access)
            }
            HostValueType::Map { key, value, access } => Self::Map {
                key: Box::new(Self::from_host_type(key)),
                value: Box::new(Self::from_host_type(value)),
                access: *access,
            },
            HostValueType::Set(ty, access) => {
                Self::Set(Box::new(Self::from_host_type(ty)), *access)
            }
            HostValueType::Option(ty) => Self::StandardEnum {
                kind: StandardEnumKind::Option,
                args: vec![Self::from_host_type(ty)],
            },
            HostValueType::Result { ok, error } => Self::StandardEnum {
                kind: StandardEnumKind::Result,
                args: vec![Self::from_host_type(ok), Self::from_host_type(error)],
            },
        }
    }

    pub fn instantiate(&self, owner: &I, arguments: &[Ty<I>]) -> Option<Self> {
        let result = TypeSubstitution::for_owner(owner, arguments)
            .apply(self, &CancellationToken::default())
            .ok()?;
        result.is_concrete().then_some(result)
    }
}
