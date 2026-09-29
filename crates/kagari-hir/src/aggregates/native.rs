//! Installed implementation selection uses checked source contracts and provenance.

use crate::{
    aggregates::{AggregateCatalog, ImplementationSignature},
    typeck::{GenericBounds, match_implementation},
    types::{NominalType, TypeId, TypeSubstitution},
};
use kagari_abi::standard::traits::StandardTrait;

#[cfg(test)]
mod tests;

impl AggregateCatalog {
    /// Match the installed declaration without discharging its bounds. Recursive
    /// searches use this projection while retaining their own search budget.
    pub(crate) fn engine_implementation_pattern(
        &self,
        interface: &NominalType,
        receiver: &TypeId,
    ) -> Option<(&ImplementationSignature, TypeSubstitution)> {
        let mut candidates = self.implementations().filter_map(|implementation| {
            if !implementation.engine_owned {
                return None;
            }
            let substitution = match_implementation(
                &implementation.trait_type,
                interface,
                &implementation.for_type,
                receiver,
                &implementation.generic_params,
            )?;
            if !implementation.accepts_native_receiver(receiver, &substitution) {
                return None;
            }
            Some((implementation, substitution))
        });
        let selected = candidates.next()?;
        candidates.next().is_none().then_some(selected)
    }

    /// Native dispatch requires both installed provenance and a unique checked
    /// implementation whose obligations hold in the current generic context.
    pub(crate) fn engine_implementation(
        &self,
        interface: &NominalType,
        receiver: &TypeId,
        bounds: &GenericBounds,
    ) -> Option<(&ImplementationSignature, TypeSubstitution)> {
        let (implementation, substitution) =
            self.engine_implementation_pattern(interface, receiver)?;
        let (selected, _) = self
            .concrete_interface_implementation(
                interface,
                receiver,
                bounds,
                4096,
                64,
                &Default::default(),
            )
            .ok()??;
        (selected == implementation.id).then_some((implementation, substitution))
    }
}

impl ImplementationSignature {
    /// Storage access is part of an installed capability contract. A readonly
    /// view can reuse read/iteration implementations, never mutable capabilities.
    pub(super) fn accepts_native_receiver(
        &self,
        receiver: &TypeId,
        substitution: &TypeSubstitution,
    ) -> bool {
        let owner = self.for_type.instantiate(substitution);
        owner == *receiver
            || (matches!(
                StandardTrait::from_id(&self.trait_type.declaration),
                Some(
                    StandardTrait::List
                        | StandardTrait::Map
                        | StandardTrait::Set
                        | StandardTrait::Iterable
                )
            ) && owner.can_weaken_to(receiver))
    }
}
