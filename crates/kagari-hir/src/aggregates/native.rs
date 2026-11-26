//! Declared implementation selection uses checked contracts and bounded proofs.

use crate::{
    aggregates::{
        AggregateCatalog, implementations::ImplementationSearchLimits,
        implementations::ImplementationSignature,
    },
    typeck::{GenericBounds, table::match_implementation},
    types::{NominalType, TypeId, TypeSubstitution},
};
use kagari_common::identity::table::DefinitionTable;

#[cfg(test)]
mod tests;

impl AggregateCatalog {
    /// Match the declared receiver without discharging its bounds. Recursive
    /// searches use this projection while retaining their own search budget.
    pub(crate) fn declared_implementation_pattern(
        &self,
        interface: &NominalType,
        receiver: &TypeId,
    ) -> Option<(&ImplementationSignature, TypeSubstitution)> {
        let mut candidates = self.implementations().filter_map(|implementation| {
            let substitution = match_implementation(
                &implementation.trait_type,
                interface,
                &implementation.for_type,
                receiver,
                &implementation.generic_params,
            )?;
            if !implementation.accepts_receiver(receiver, &substitution) {
                return None;
            }
            Some((implementation, substitution))
        });
        let selected = candidates.next()?;
        candidates.next().is_none().then_some(selected)
    }

    /// Selection requires a unique checked
    /// implementation whose obligations hold in the current generic context.
    pub(crate) fn declared_implementation(
        &self,
        definitions: &DefinitionTable,
        interface: &NominalType,
        receiver: &TypeId,
        bounds: &GenericBounds,
    ) -> Option<(&ImplementationSignature, TypeSubstitution)> {
        let (implementation, substitution) =
            self.declared_implementation_pattern(interface, receiver)?;
        let (selected, _) = self
            .concrete_interface_implementation(
                definitions,
                interface,
                receiver,
                bounds,
                ImplementationSearchLimits {
                    max_checks: 4096,
                    max_depth: 64,
                },
                &Default::default(),
            )
            .ok()??;
        (selected == implementation.id).then_some((implementation, substitution))
    }
}

impl ImplementationSignature {
    /// Match the exact declared receiver, including its concrete host access
    /// descriptor. Readonly receivers have distinct read/iteration declarations.
    pub(super) fn accepts_receiver(
        &self,
        receiver: &TypeId,
        substitution: &TypeSubstitution,
    ) -> bool {
        let owner = self.for_type.instantiate(substitution);
        owner == *receiver
    }
}
