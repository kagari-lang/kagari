//! Dynamic views retain explicit outputs and erase a bounded iterator output.

use crate::{
    aggregates::{AggregateCatalog, implementations::ImplementationSearchError},
    types::{NominalType, TypeId, TypeSubstitution},
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{DefinitionKind, table::DefinitionTable},
};
use kagari_types::{
    language::Protocol,
    ty::substitution::{MAX_TYPE_DEPTH, MAX_TYPE_NODES},
};

impl AggregateCatalog {
    /// Canonicalizes value arguments within a requirement or implementation header.
    /// The application's own associated constraints are preserved exactly.
    pub fn canonical_interface_application(
        &self,
        definitions: &DefinitionTable,
        interface: &NominalType,
    ) -> NominalType {
        let TypeId::Trait(interface) = TypeId::Trait(interface.clone())
            .map_children(|ty| self.canonical_value_type(definitions, ty))
        else {
            unreachable!("interface application")
        };
        interface
    }

    /// Canonicalizes executable value types. Unlike an implementation requirement,
    /// an Iterable value with an omitted Iter exposes the bounded iterator view.
    /// Explicit outputs remain exact, including inside nested value types.
    /// Associated projections remain unresolved; invalid interface ancestry or
    /// an exceeded type traversal limit produces an error type.
    pub fn canonical_value_type(&self, definitions: &DefinitionTable, ty: &TypeId) -> TypeId {
        fn walk(
            catalog: &AggregateCatalog,
            definitions: &DefinitionTable,
            ty: &TypeId,
            depth: usize,
            remaining: &mut usize,
        ) -> TypeId {
            if depth > MAX_TYPE_DEPTH || *remaining == 0 {
                return TypeId::Error;
            }
            *remaining -= 1;
            let ty =
                ty.map_children(|child| walk(catalog, definitions, child, depth + 1, remaining));
            if let TypeId::Trait(interface) = &ty {
                return catalog
                    .interface_closure(definitions, interface, &ty, &Default::default())
                    .ok()
                    .and_then(|interfaces| interfaces.into_iter().next())
                    .map(TypeId::Trait)
                    .unwrap_or(TypeId::Error);
            }
            ty
        }
        if !ty.contains_interface() {
            return ty.clone();
        }
        let mut remaining = MAX_TYPE_NODES;
        walk(self, definitions, ty, 0, &mut remaining)
    }

    /// An Iterable value hides its concrete iterator behind the declared
    /// Iterator bound. Concrete receivers and implementation proofs keep the
    /// original output; this closure describes only a dynamic value's surface.
    pub fn interface_closure(
        &self,
        definitions: &DefinitionTable,
        interface: &NominalType,
        receiver: &TypeId,
        cancel: &CancellationToken,
    ) -> Result<Vec<NominalType>, ImplementationSearchError> {
        let mut closure = self.trait_closure(interface, receiver, cancel)?;
        for parent in &mut closure {
            cancel
                .check()
                .map_err(|_| ImplementationSearchError::Cancelled)?;
            if Some(parent.declaration) != Protocol::Iterable.definition(definitions) {
                continue;
            }
            let Some(iter) = definitions.lookup_child(
                parent.declaration,
                DefinitionKind::AssociatedType,
                "Iter",
                0,
            ) else {
                continue;
            };
            if parent.associated_types.contains_key(&iter) {
                continue;
            }
            let Some(contract) = self.trait_(&parent.declaration) else {
                continue;
            };
            let Some([bound]) = contract.associated_types.get(&iter).map(Vec::as_slice) else {
                continue;
            };
            if Some(bound.declaration) != Protocol::Iterator.definition(definitions) {
                continue;
            }
            let substitution: TypeSubstitution = contract
                .generic_params
                .iter()
                .cloned()
                .zip(parent.arguments.iter().cloned())
                .collect();
            let erased = TypeId::Trait(bound.instantiate(&substitution))
                .with_associated_types(parent, &TypeId::SelfType(parent.declaration));
            if erased.contains_self_type() || erased.contains_projection() {
                continue;
            }
            parent.associated_types.insert(iter, erased);
        }
        Ok(closure)
    }
}
