//! Reference coverage for attached registration facts; source arenas remain unchanged.

use crate::{
    hir::{item::function::FunctionImplementation, origin::NodeOrigin, ty::TypeKind},
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
        for (_, ty) in self.module.nodes.types.iter() {
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
        for origin in self.module.nodes.origins() {
            check_cancel(cancel)?;
            if let NodeOrigin::Structured { declaration } = origin {
                visit(declaration)?;
            }
        }
        for declaration in self.source_map.structured_definitions() {
            check_cancel(cancel)?;
            visit(declaration)?;
        }
        for item in &self.module.native_types {
            if let Some(host) = &item.host {
                host.visit_definition_ids(visit, cancel)?;
            }
        }
        for item in &self.module.traits {
            if let Some(relationship) = &item.conversion_adapter {
                relationship.visit_definition_ids(visit, cancel)?;
            }
        }
        Ok(())
    }
}
