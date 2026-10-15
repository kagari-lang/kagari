//! HIR representation hooks supplied only by an installed native declaration.
use crate::types::{NominalType, TypeId};
use kagari_common::identity::table::DefinitionId;
use kagari_types::{
    callable::NativeDefaultApplication,
    collection::CollectionAccess,
    declaration::path::HostPathDeclaration,
    declaration::{TypeDeclaration, TypeDef, native::NativeStorageLayout},
    range::RangeKind,
    reference::ReferenceAccess,
    scalar::BuiltinType,
};

use std::sync::Arc;

pub(crate) mod access;
pub(crate) mod adoption;
pub(crate) mod api;
pub(crate) mod paths;
pub mod render;

/// Installed entry identity or symbolic default mapping.
/// Selection resolves defaults to ordinary entries; bindings are not function pointers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NativeBinding {
    /// Canonical identity of an installed native entry, not its machine address.
    Entry(DefinitionId),
    /// Registered recipe for a generated trait forwarding body.
    /// Source lowering also requires [`crate::typeck::table::TypeTable::native_default_call`];
    /// source-free linking independently validates the portable recipe.
    Default(NativeDefaultApplication),
}

/// Installed representation descriptor used to apply native-backed source type declarations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NativeTypeKind {
    /// External object identity; this grants no Rust layout or reference target.
    Host {
        declaration: DefinitionId,
        /// Checked shared schema, retained for field policy and source-free ABI lowering.
        definition: Arc<TypeDef>,
        /// Explicit paths owned by this root's declaring module.
        paths: Arc<[HostPathDeclaration]>,
        /// Checked external schema inputs needed by paths, including unnamed
        /// field and virtual/index targets. This does not import source names.
        schemas: Arc<[TypeDeclaration]>,
    },
    /// A scope-bound host location with a checked target and access capability.
    Reference(ReferenceAccess),
    /// A nominal registered storage type with explicit layout and generic arity.
    Storage {
        /// Canonical nominal type definition.
        declaration: DefinitionId,
        /// Required number of positional type arguments.
        arity: usize,
        /// Registered storage representation descriptor, checked at installation boundaries.
        layout: NativeStorageLayout,
    },
    /// Intrinsic String representation with no generic arguments.
    String,
    /// Existing map storage representation with key and value types.
    HashMap,
    /// Existing set storage representation with one element type.
    HashSet,
    /// Existing iterator representation with one element type.
    Iter,
    /// Range representation; full ranges take no argument and other shapes take an element type.
    Range(RangeKind),
}

impl NativeTypeKind {
    /// Returns the required positional type-argument count for this descriptor.
    pub fn arity(&self) -> usize {
        match self {
            Self::Storage { arity, .. } => *arity,
            Self::Host { .. } | Self::String | Self::Range(RangeKind::Full) => 0,
            Self::HashMap => 2,
            _ => 1,
        }
    }

    /// Builds the semantic representation with supplied arguments; returns `None` on arity mismatch.
    pub fn apply(&self, arguments: &[TypeId]) -> Option<TypeId> {
        if arguments.len() != self.arity() {
            return None;
        }
        let first = || Box::new(arguments[0].clone());
        Some(match self {
            Self::Host { declaration, .. } => TypeId::Host(*declaration),
            Self::Reference(access) => TypeId::Reference {
                target: first(),
                access: *access,
            },
            Self::Storage { declaration, .. } => TypeId::NativeObject(NominalType {
                declaration: *declaration,
                arguments: arguments.to_vec(),
                associated_types: Default::default(),
            }),
            Self::String => TypeId::Builtin(BuiltinType::String),
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
