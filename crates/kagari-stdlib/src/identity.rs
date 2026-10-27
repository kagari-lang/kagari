//! Rust-authoritative mandatory library declarations and registration identities.
use crate::namespaces;
use kagari_common::identity::{DefinitionKind, DefinitionPath, DefinitionPathSegment};
use std::sync::LazyLock;

/// Build an explicit foundation registration identity. This creates no declaration
/// and grants no language semantics; consumers must resolve its checked record.
pub fn trait_id(name: &str) -> DefinitionPath {
    DefinitionPath {
        module: namespaces::trait_owner(name),
        path: vec![DefinitionPathSegment {
            kind: DefinitionKind::Trait,
            name: name.into(),
            occurrence: 0,
        }],
    }
}

/// Build an exact external query for a canonical bundled type declaration.
/// Callers still resolve its registered contract in an explicit scope.
pub fn type_id(name: &str, kind: DefinitionKind) -> DefinitionPath {
    DefinitionPath {
        module: namespaces::type_owner(name),
        path: vec![DefinitionPathSegment {
            kind,
            name: name.into(),
            occurrence: 0,
        }],
    }
}

/// Immutable library symbol shared by declaration preparation and native traversal.
/// It owns no runtime IDs, checked layouts, call authority or heap references.
pub(crate) fn vector_id() -> &'static DefinitionPath {
    static VECTOR: LazyLock<DefinitionPath> =
        LazyLock::new(|| type_id("Vec", DefinitionKind::AssociatedType));
    &VECTOR
}
