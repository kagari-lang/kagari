//! Reference coverage for attached registration facts; source arenas remain unchanged.

use crate::{
    hir::{item::function::FunctionImplementation, ty::TypeKind},
    lower::LoweredModule,
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        mapping::{DefinitionMappingError, check_cancel},
        scoped::DefinitionReferences,
        table::DefinitionId,
    },
};

impl DefinitionReferences for LoweredModule {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        for (_, ty) in self.module.body.types.iter() {
            check_cancel(cancel)?;
            if let TypeKind::Definition { declaration, .. } = &ty.kind {
                visit(declaration)?;
            }
        }
        for function in &self.module.functions {
            if let FunctionImplementation::Native(contract) = &function.implementation {
                contract.visit_definition_ids(visit, cancel)?;
            }
        }
        for declaration in self.source_map.structured_definitions() {
            check_cancel(cancel)?;
            visit(declaration)?;
        }
        for (id, value) in self
            .registered_traits
            .iter()
            .chain(&self.native_trait_references)
            .chain(&self.external_traits)
        {
            check_cancel(cancel)?;
            visit(id)?;
            value.visit_definition_ids(visit, cancel)?;
        }
        for value in &self.registered_declarations {
            value.visit_definition_ids(visit, cancel)?;
        }
        for value in self.native_types.values() {
            value.visit_definition_ids(visit, cancel)?;
        }
        for value in self.native_functions.values() {
            value.visit_definition_ids(visit, cancel)?;
        }
        for value in self.native_trait_adapters.values() {
            value.visit_definition_ids(visit, cancel)?;
        }
        Ok(())
    }
}
