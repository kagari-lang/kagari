//! Semantic type expressions and scoped generic constraints; no execution representation.
pub mod access;
mod bounds;
pub mod identity;
pub mod inheritance;
mod mapping;
pub mod matching;
mod order;
mod reference;
mod shape;
pub mod substitution;
#[doc(hidden)]
pub mod wire;

use crate::{
    collection::CollectionAccess, range::RangeKind, reference::ReferenceAccess,
    scalar::BuiltinType, ty::substitution::TypeSubstitution,
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
    /// A scope-bound host location, never an ordinary GC value or Rust pointer.
    Reference {
        target: Box<Ty>,
        access: ReferenceAccess,
    },
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
                Self::Reference { target, .. } => pending.push(target),
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
                Self::Reference { target, .. } => pending.push(target),
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
    pub constraints: Vec<NominalTy>,
}

impl Ty {}

impl Ty {
    pub fn instantiate(&self, owner: &DefinitionId, arguments: &[Ty]) -> Option<Self> {
        let result = TypeSubstitution::for_owner(owner, arguments)
            .apply(self, &CancellationToken::default())
            .ok()?;
        result.is_concrete().then_some(result)
    }
}
