//! Rust-authoritative mandatory library declarations and registration identities.
use crate::namespaces;
use kagari_common::identity::{DefinitionKind, DefinitionPath, DefinitionPathSegment};
use kagari_types::ty::{NominalTy, Ty};

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

/// Apply an explicit registration identity; this does not supply a trait contract.
pub fn applied(name: &str, arguments: Vec<Ty>) -> NominalTy {
    NominalTy {
        declaration: trait_id(name),
        arguments,
        associated_types: Default::default(),
    }
}
