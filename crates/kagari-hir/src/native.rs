//! HIR representation hooks supplied only by an installed native declaration.
use crate::{
    host::HostFunctionId,
    types::{NominalType, TypeId},
};
use kagari_common::identity::{DefinitionPath, reference::DefinitionReference};
use kagari_types::{
    callable::NativeDefaultApplication, collection::CollectionAccess,
    declaration::native::NativeStorageLayout, range::RangeKind, scalar::BuiltinType,
};

pub(crate) mod api;
pub(crate) mod paths;
pub mod render;

/// Installed entry identity, checked host catalog index or symbolic default mapping.
/// Selection resolves defaults to ordinary entries; bindings are not function pointers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NativeBinding<I: DefinitionReference = DefinitionPath> {
    Entry(I),
    Host(HostFunctionId),
    /// Symbolic registered template application; selection resolves an Entry.
    Default(NativeDefaultApplication<I>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NativeTypeKind<I: DefinitionReference = DefinitionPath> {
    Storage {
        declaration: I,
        arity: usize,
        layout: NativeStorageLayout,
    },
    String,
    Vec,
    HashMap,
    HashSet,
    Iter,
    Range(RangeKind),
}

impl NativeTypeKind {
    pub fn arity(&self) -> usize {
        match self {
            Self::Storage { arity, .. } => *arity,
            Self::String | Self::Range(RangeKind::Full) => 0,
            Self::HashMap => 2,
            _ => 1,
        }
    }

    pub fn apply(&self, arguments: &[TypeId]) -> Option<TypeId> {
        if arguments.len() != self.arity() {
            return None;
        }
        let first = || Box::new(arguments[0].clone());
        Some(match self {
            Self::Storage { declaration, .. } => TypeId::NativeObject(NominalType {
                declaration: declaration.clone(),
                arguments: arguments.to_vec(),
                associated_types: Default::default(),
            }),
            Self::String => TypeId::Builtin(BuiltinType::String),
            Self::Vec => TypeId::Array(first(), CollectionAccess::Mutable),
            Self::HashMap => TypeId::Map {
                key: first(),
                value: Box::new(arguments[1].clone()),
                access: CollectionAccess::Mutable,
            },
            Self::HashSet => TypeId::Set(first(), CollectionAccess::Mutable),
            Self::Iter => TypeId::Iter(first()),
            Self::Range(RangeKind::Full) => TypeId::Range(
                Box::new(TypeId::Builtin(BuiltinType::Unit)),
                RangeKind::Full,
            ),
            Self::Range(kind) => TypeId::Range(first(), *kind),
        })
    }
}

mod mapping;
