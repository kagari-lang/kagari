//! Materialize the concrete iterator witness used by an erased dynamic result.
use crate::source::lower::{MirLoweringError, instances::InstancePlanner};
use kagari_abi::language::{Protocol, identity};
use kagari_common::{
    identity::{DefinitionPath, associated_type_id},
    span::Span,
};
use kagari_hir::types::{TypeId, TypeSubstitution};

impl InstancePlanner<'_> {
    pub(super) fn require_iterator_view(
        &mut self,
        declaration: &DefinitionPath,
        arguments: &[TypeId],
        span: Span,
    ) -> Result<(), MirLoweringError> {
        let Some(contract) = self.catalog.implementation_signature(declaration) else {
            return Ok(());
        };
        if contract.trait_type.declaration != identity(Protocol::Iterable)
            || contract.generic_params.len() != arguments.len()
        {
            return Ok(());
        }
        let substitution: TypeSubstitution = contract
            .generic_params
            .iter()
            .cloned()
            .zip(arguments.iter().cloned())
            .collect();
        let mut interface = contract.trait_type.instantiate(&substitution);
        let receiver = contract.for_type.instantiate(&substitution);
        let output = associated_type_id(&interface.declaration, "Iter");
        let Some(iterator) = interface.associated_types.remove(&output) else {
            return Ok(());
        };
        let invalid = || MirLoweringError::MissingBinding("dynamic iterator result witness");
        let view = self
            .catalog
            .interface_closure(&interface, &receiver, &self.options.cancel)
            .map_err(|_| invalid())?
            .remove(0);
        let Some(TypeId::Trait(bound)) = view.associated_types.get(&output) else {
            return Err(invalid());
        };
        if iterator == TypeId::Trait(bound.clone()) {
            return Ok(());
        }
        let (implementation, arguments) = self
            .catalog
            .concrete_interface_implementation(
                bound,
                &iterator,
                &Default::default(),
                100_000,
                64,
                &self.options.cancel,
            )
            .map_err(|_| invalid())?
            .ok_or_else(invalid)?;
        self.record_interface(&implementation, &arguments, span)?;
        if implementation.module == *self.module.lowered.source.module_identity() {
            let contract = self
                .catalog
                .implementation_signature(&implementation)
                .ok_or_else(invalid)?;
            for method in self.catalog.implementation_methods(contract) {
                self.enqueue_interface_method(&method, &arguments, span)?;
            }
        }
        Ok(())
    }
}
