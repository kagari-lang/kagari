//! Contextual identity traversal of the owning metadata records.
use crate::function::{BasicBlock, MirFunction, MirModule};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        mapping::{DefinitionMapper, DefinitionMappingError, check_cancel, map_sequence},
        scoped::{DefinitionReferences, RemapDefinitions},
        table::DefinitionId,
    },
};

impl RemapDefinitions for MirModule {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_>,
    ) -> Result<Self, DefinitionMappingError> {
        mapper.check()?;
        Ok(MirModule {
            native_targets: map_sequence(&self.native_targets, |value| {
                (value).remap_definitions(mapper)
            })?,
            interface_instances: map_sequence(&self.interface_instances, |value| {
                (value).remap_definitions(mapper)
            })?,
            external_types: map_sequence(&self.external_types, |value| {
                value.remap_definitions(mapper)
            })?,
            dependencies: self.dependencies.clone(),
            structures: map_sequence(&self.structures, |value| (value).remap_definitions(mapper))?,
            enumerations: map_sequence(&self.enumerations, |value| {
                (value).remap_definitions(mapper)
            })?,
            identity: self.identity.clone(),
            source_name: self.source_name.clone(),
            module_slots: self.module_slots.clone(),
            abi: self.abi.remap_definitions(mapper)?,
            functions: map_sequence(&self.functions, |value| (value).remap_definitions(mapper))?,
        })
    }
}

impl DefinitionReferences for MirModule {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        for value in &self.external_types {
            value.visit_definition_ids(visit, cancel)?;
        }
        for value0 in &self.native_targets {
            (value0).visit_definition_ids(visit, cancel)?;
        }
        for value0 in &self.interface_instances {
            (value0).visit_definition_ids(visit, cancel)?;
        }

        for value0 in &self.structures {
            (value0).visit_definition_ids(visit, cancel)?;
        }
        for value0 in &self.enumerations {
            (value0).visit_definition_ids(visit, cancel)?;
        }
        self.abi.visit_definition_ids(visit, cancel)?;
        for value0 in &self.functions {
            (value0).visit_definition_ids(visit, cancel)?;
        }
        Ok(())
    }
}

impl RemapDefinitions for MirFunction {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_>,
    ) -> Result<Self, DefinitionMappingError> {
        mapper.check()?;
        Ok(MirFunction {
            semantic: self.semantic.remap_definitions(mapper)?,
            id: self.id,
            instance: self.instance.remap_definitions(mapper)?,
            name: self.name.clone(),
            params: self.params.clone(),
            return_type: self.return_type,
            locals: self.locals.clone(),
            temps: self.temps.clone(),
            blocks: map_sequence(&self.blocks, |value| (value).remap_definitions(mapper))?,
            entry: self.entry,
            effects: self.effects,
            debug: self.debug.clone(),
        })
    }
}

impl DefinitionReferences for MirFunction {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        self.semantic.visit_definition_ids(visit, cancel)?;
        self.instance.visit_definition_ids(visit, cancel)?;
        for value0 in &self.blocks {
            (value0).visit_definition_ids(visit, cancel)?;
        }
        Ok(())
    }
}

impl RemapDefinitions for BasicBlock {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_>,
    ) -> Result<Self, DefinitionMappingError> {
        mapper.check()?;
        Ok(BasicBlock {
            instructions: map_sequence(&self.instructions, |value| {
                (value).remap_definitions(mapper)
            })?,
            instruction_spans: self.instruction_spans.clone(),
            instruction_scopes: self.instruction_scopes.clone(),
            terminator: self.terminator.clone(),
            terminator_span: self.terminator_span,
            terminator_scope: self.terminator_scope,
        })
    }
}

impl DefinitionReferences for BasicBlock {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        for value0 in &self.instructions {
            (value0).visit_definition_ids(visit, cancel)?;
        }
        Ok(())
    }
}
