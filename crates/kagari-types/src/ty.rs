//! Semantic type expressions and scoped generic constraints; no execution representation.
pub mod access;
mod bounds;
pub mod identity;
pub mod inheritance;
mod mapping;
pub mod matching;
mod order;
pub mod substitution;
#[doc(hidden)]
pub mod wire;

use crate::{
    collection::CollectionAccess, host_interface::value_type::HostValueType, range::RangeKind,
    scalar::BuiltinType, surface::StandardTypeConstraint, ty::substitution::TypeSubstitution,
};
use kagari_common::{cancellation::CancellationToken, identity::table::DefinitionId};
use std::collections::BTreeMap;

/// Semantic types preserve nominal identity and container arguments.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NominalTy {
    pub declaration: DefinitionId,
    pub arguments: Vec<Ty>,
    pub associated_types: BTreeMap<DefinitionId, Ty>,
}

impl NominalTy {
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
pub enum Ty {
    Projection {
        arguments: Vec<Ty>,
        receiver: Box<Ty>,
        interface: Box<NominalTy>,
        member: DefinitionId,
    },
    Host(DefinitionId),
    /// Receiver template in a trait signature, never an executable value layout.
    SelfType(DefinitionId),
    /// Valid only in a declaration template, never in an executable layout.
    Parameter {
        owner: DefinitionId,
        position: usize,
    },
    Builtin(BuiltinType),
    Tuple(Vec<Ty>),
    Function {
        params: Vec<Ty>,
        result: Box<Ty>,
    },
    Iter(Box<Ty>),
    Range(Box<Ty>, RangeKind),
    Array(Box<Ty>),
    Map {
        key: Box<Ty>,
        value: Box<Ty>,
        access: CollectionAccess,
    },
    Set(Box<Ty>, CollectionAccess),
    Struct(NominalTy),
    /// A nominal script-heap object backed by a registered traced Rust payload.
    NativeObject(NominalTy),
    Enum(NominalTy),
    Trait(NominalTy),
}

impl Ty {
    pub fn contains_projection(&self) -> bool {
        let mut pending = vec![self];
        while let Some(ty) = pending.pop() {
            match ty {
                Self::Projection { .. } => return true,
                Self::Struct(ty) | Self::NativeObject(ty) | Self::Enum(ty) | Self::Trait(ty) => {
                    pending.extend(&ty.arguments);
                    pending.extend(ty.associated_types.values());
                }
                Self::Tuple(items) => pending.extend(items),
                Self::Array(item)
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
                Self::Tuple(types) => pending.extend(types),
                Self::Function { params, result } => {
                    pending.extend(params);
                    pending.push(result);
                }
                Self::Array(ty) | Self::Set(ty, _) | Self::Iter(ty) | Self::Range(ty, _) => {
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GenericParam {
    pub owner: DefinitionId,
    pub position: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct GenericBound {
    pub ty: Ty,
    pub constraints: Vec<Constraint>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Constraint {
    Standard(StandardTypeConstraint),
    Trait(NominalTy),
}

impl Ty {
    pub fn from_host_type(ty: &HostValueType) -> Self {
        match ty {
            HostValueType::Unit => Self::Builtin(BuiltinType::Unit),
            HostValueType::Bool => Self::Builtin(BuiltinType::Bool),
            HostValueType::I32 => Self::Builtin(BuiltinType::I32),
            HostValueType::I64 => Self::Builtin(BuiltinType::I64),
            HostValueType::F32 => Self::Builtin(BuiltinType::F32),
            HostValueType::F64 => Self::Builtin(BuiltinType::F64),
            HostValueType::String => Self::Builtin(BuiltinType::String),
            HostValueType::Opaque(id) => Self::Host(*id),
            HostValueType::Tuple(types) => {
                Self::Tuple(types.iter().map(Self::from_host_type).collect())
            }
            HostValueType::Array(ty) => Self::Array(Box::new(Self::from_host_type(ty))),
            HostValueType::Vec(declaration, item) | HostValueType::List(declaration, item) => {
                let nominal = NominalTy {
                    declaration: *declaration,
                    arguments: vec![Self::from_host_type(item)],
                    associated_types: Default::default(),
                };
                if matches!(ty, HostValueType::Vec(..)) {
                    Self::NativeObject(nominal)
                } else {
                    Self::Trait(nominal)
                }
            }
            HostValueType::Map { key, value, access } => Self::Map {
                key: Box::new(Self::from_host_type(key)),
                value: Box::new(Self::from_host_type(value)),
                access: *access,
            },
            HostValueType::Set(ty, access) => {
                Self::Set(Box::new(Self::from_host_type(ty)), *access)
            }
            HostValueType::Option(declaration, ty) => Self::Enum(NominalTy {
                declaration: *declaration,
                arguments: vec![Self::from_host_type(ty)],
                associated_types: Default::default(),
            }),
            HostValueType::Result {
                declaration,
                ok,
                error,
            } => Self::Enum(NominalTy {
                declaration: *declaration,
                arguments: vec![Self::from_host_type(ok), Self::from_host_type(error)],
                associated_types: Default::default(),
            }),
        }
    }
}

impl Ty {
    pub fn instantiate(&self, owner: &DefinitionId, arguments: &[Ty]) -> Option<Self> {
        let result = TypeSubstitution::for_owner(owner, arguments)
            .apply(self, &CancellationToken::default())
            .ok()?;
        result.is_concrete().then_some(result)
    }
}
