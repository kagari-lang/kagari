//! HIR representation hooks supplied only by an installed native declaration.

use kagari_common::identity::table::DefinitionId;
use kagari_types::{
    callable::NativeDefaultApplication,
    declaration::path::HostPathDeclaration,
    declaration::{TypeDeclaration, TypeDef, native::NativeStorageLayout},
    range::RangeKind,
    reference::ReferenceAccess,
};

use std::sync::Arc;

pub(crate) mod access;
pub(crate) mod adoption;
pub(crate) mod api;
pub(crate) mod paths;
pub mod render;
pub(crate) mod structured;

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

    /// Range representation; full ranges take no argument and other shapes take an element type.
    Range(RangeKind),
}

impl NativeTypeKind {
    /// Returns the required positional type-argument count for this descriptor.
    pub fn arity(&self) -> usize {
        match self {
            Self::Storage { arity, .. } => *arity,
            Self::Host { .. } | Self::String | Self::Range(RangeKind::Full) => 0,

            _ => 1,
        }
    }
}

mod mapping;
