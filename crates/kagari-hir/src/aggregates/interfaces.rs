//! Dynamic views retain explicit outputs and erase a bounded iterator output.

use crate::{
    aggregates::{AggregateCatalog, implementations::ImplementationSearchError},
    typeck::table::ConstraintTarget,
    types::{NominalType, TypeId, TypeSubstitution},
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{DefinitionKind, table::DefinitionTable},
};
use kagari_types::language::Protocol;

impl AggregateCatalog {
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
            let Some([ConstraintTarget::Trait(bound)]) =
                contract.associated_types.get(&iter).map(Vec::as_slice)
            else {
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
