//! Materialize the concrete iterator witness used by an erased dynamic result.
use crate::source::lower::{MirLoweringError, instances::InstancePlanner};
use kagari_common::{
    identity::{DefinitionKind, table::DefinitionId},
    span::Span,
};
use kagari_hir::{
    aggregates::implementations::ImplementationSearchLimits,
    types::{TypeId, TypeSubstitution},
};
use kagari_types::language::Protocol;

impl InstancePlanner<'_> {
    pub(super) fn require_iterator_view(
        &mut self,
        declaration: &DefinitionId,
        arguments: &[TypeId],
        span: Span,
    ) -> Result<(), MirLoweringError> {
        let Some(contract) = self.catalog.implementation_signature(declaration) else {
            return Ok(());
        };
        if Some(contract.trait_type.declaration)
            != Protocol::Iterable.definition(&self.definitions())
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
        let output = self
            .definitions()
            .lookup_child(
                interface.declaration,
                DefinitionKind::AssociatedType,
                "Iter",
                0,
            )
            .expect("registered associated member");
        let Some(iterator) = interface.associated_types.remove(&output) else {
            return Ok(());
        };
        let invalid = || MirLoweringError::MissingBinding("dynamic iterator result witness");
        let view = self
            .catalog
            .interface_closure(
                &self.definitions(),
                &interface,
                &receiver,
                &self.options.cancel,
            )
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
                &self.definitions(),
                bound,
                &iterator,
                &Default::default(),
                ImplementationSearchLimits {
                    max_checks: 100_000,
                    max_depth: 64,
                },
                &self.options.cancel,
            )
            .map_err(|_| invalid())?
            .ok_or_else(invalid)?;
        self.record_interface(&implementation, &arguments, span)?;
        if self.module_handle(implementation) == self.owner_handle() {
            let contract = self
                .catalog
                .implementation_signature(&implementation)
                .ok_or_else(invalid)?;
            for method in self
                .catalog
                .implementation_methods(&self.definitions(), contract)
            {
                self.enqueue_interface_method(&method, &arguments, span)?;
            }
        }
        Ok(())
    }
}
