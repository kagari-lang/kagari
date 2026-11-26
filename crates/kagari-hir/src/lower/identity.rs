//! Scoped definition references retained by lowered records.

use crate::{
    hir::{
        item::{Module, function::FunctionImplementation},
        origin::NodeOrigin,
        ty::TypeKind,
    },
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

use std::iter::once;

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
        // The module input kind describes its source surface, not the IDs held by its records.
        for origin in self
            .module
            .nodes
            .origins()
            .chain(declaration_origins(&self.module))
        {
            check_cancel(cancel)?;
            if let NodeOrigin::Structured { declaration } = origin {
                visit(declaration)?;
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

fn declaration_origins(module: &Module) -> impl Iterator<Item = &NodeOrigin> {
    module
        .functions
        .iter()
        .flat_map(|item| once(&item.origin).chain(item.params.iter().map(|param| &param.origin)))
        .chain(module.consts.iter().map(|item| &item.origin))
        .chain(module.statics.iter().map(|item| &item.origin))
        .chain(module.modules.iter().map(|item| &item.origin))
        .chain(module.uses.iter().map(|item| &item.origin))
        .chain(module.structs.iter().map(|item| &item.origin))
        .chain(module.enums.iter().map(|item| &item.origin))
        .chain(module.native_types.iter().map(|item| &item.origin))
        .chain(module.type_aliases.iter().map(|item| &item.origin))
        .chain(module.traits.iter().map(|item| &item.origin))
        .chain(module.impls.iter().map(|item| &item.origin))
        .chain(module.generic_params.iter().map(|item| &item.origin))
        .chain(
            module
                .structs
                .iter()
                .flat_map(|item| &item.fields)
                .map(|item| &item.origin),
        )
        .chain(
            module
                .native_types
                .iter()
                .flat_map(|item| &item.fields)
                .map(|item| &item.origin),
        )
        .chain(
            module
                .enums
                .iter()
                .flat_map(|item| &item.variants)
                .map(|item| &item.origin),
        )
        .chain(
            module
                .traits
                .iter()
                .flat_map(|item| &item.methods)
                .map(|item| &item.origin),
        )
        .chain(
            module
                .traits
                .iter()
                .flat_map(|item| &item.associated_consts)
                .map(|item| &item.origin),
        )
        .chain(
            module
                .traits
                .iter()
                .flat_map(|item| &item.associated_types)
                .map(|item| &item.origin),
        )
        .chain(
            module
                .impls
                .iter()
                .flat_map(|item| &item.associated_consts)
                .map(|item| &item.origin),
        )
        .chain(
            module
                .impls
                .iter()
                .flat_map(|item| &item.associated_types)
                .map(|item| &item.origin),
        )
}
