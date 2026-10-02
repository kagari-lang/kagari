//! Dynamic views retain explicit outputs and erase a bounded iterator output.

use crate::{
    aggregates::{AggregateCatalog, implementations::ImplementationSearchError},
    typeck::table::ConstraintTarget,
    types::{NominalType, TypeId, TypeSubstitution},
};
use kagari_abi::language::{Protocol, identity};
use kagari_common::{cancellation::CancellationToken, identity::associated_type_id};

impl AggregateCatalog {
    /// An Iterable value hides its concrete iterator behind the declared
    /// Iterator bound. Concrete receivers and implementation proofs keep the
    /// original output; this closure describes only a dynamic value's surface.
    pub fn interface_closure(
        &self,
        interface: &NominalType,
        receiver: &TypeId,
        cancel: &CancellationToken,
    ) -> Result<Vec<NominalType>, ImplementationSearchError> {
        let mut closure = self.trait_closure(interface, receiver, cancel)?;
        for parent in &mut closure {
            cancel
                .check()
                .map_err(|_| ImplementationSearchError::Cancelled)?;
            if parent.declaration != identity(Protocol::Iterable) {
                continue;
            }
            let iter = associated_type_id(&parent.declaration, "Iter");
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
            if bound.declaration != identity(Protocol::Iterator) {
                continue;
            }
            let substitution: TypeSubstitution = contract
                .generic_params
                .iter()
                .cloned()
                .zip(parent.arguments.iter().cloned())
                .collect();
            let erased =
                TypeId::Trait(bound.instantiate(&substitution)).with_associated_types(parent);
            if erased.contains_self_type() || erased.contains_projection() {
                continue;
            }
            parent.associated_types.insert(iter, erased);
        }
        Ok(closure)
    }
}
