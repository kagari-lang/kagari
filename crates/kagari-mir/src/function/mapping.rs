//! Contextual identity traversal of the owning metadata records.
use crate::function::{BasicBlock, MirFunction, MirModule};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        mapping::{
            DefinitionMapper, DefinitionMappingError, DefinitionRecord, check_cancel, map_sequence,
        },
        reference::DefinitionReference,
    },
};

impl<I: DefinitionReference> DefinitionRecord<I> for MirModule<I> {
    type Rebind<J: DefinitionReference> = MirModule<J>;
    fn map_identities<J: DefinitionReference>(
        &self,
        mapper: &mut DefinitionMapper<'_, I, J>,
    ) -> Result<Self::Rebind<J>, DefinitionMappingError> {
        mapper.check()?;
        Ok(MirModule {
            native_targets: map_sequence(&self.native_targets, |value| {
                (value).map_identities(mapper)
            })?,
            interface_instances: map_sequence(&self.interface_instances, |value| {
                (value).map_identities(mapper)
            })?,
            host_types: map_sequence(&self.host_types, |value| (value).map_identities(mapper))?,
            dependencies: self.dependencies.clone(),
            structures: map_sequence(&self.structures, |value| (value).map_identities(mapper))?,
            enumerations: map_sequence(&self.enumerations, |value| (value).map_identities(mapper))?,
            identity: self.identity.clone(),
            source_name: self.source_name.clone(),
            module_slots: self.module_slots.clone(),
            abi: self.abi.map_identities(mapper)?,
            functions: map_sequence(&self.functions, |value| (value).map_identities(mapper))?,
        })
    }
    fn visit_definitions(
        &self,
        visit: &mut impl FnMut(&I) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        for value0 in &self.native_targets {
            (value0).visit_definitions(visit, cancel)?;
        }
        for value0 in &self.interface_instances {
            (value0).visit_definitions(visit, cancel)?;
        }
        for value0 in &self.host_types {
            (value0).visit_definitions(visit, cancel)?;
        }
        for value0 in &self.structures {
            (value0).visit_definitions(visit, cancel)?;
        }
        for value0 in &self.enumerations {
            (value0).visit_definitions(visit, cancel)?;
        }
        self.abi.visit_definitions(visit, cancel)?;
        for value0 in &self.functions {
            (value0).visit_definitions(visit, cancel)?;
        }
        Ok(())
    }
}

impl<I: DefinitionReference> DefinitionRecord<I> for MirFunction<I> {
    type Rebind<J: DefinitionReference> = MirFunction<J>;
    fn map_identities<J: DefinitionReference>(
        &self,
        mapper: &mut DefinitionMapper<'_, I, J>,
    ) -> Result<Self::Rebind<J>, DefinitionMappingError> {
        mapper.check()?;
        Ok(MirFunction {
            semantic: self.semantic.map_identities(mapper)?,
            id: self.id,
            instance: self.instance.map_identities(mapper)?,
            name: self.name.clone(),
            params: self.params.clone(),
            return_type: self.return_type,
            locals: self.locals.clone(),
            temps: self.temps.clone(),
            blocks: map_sequence(&self.blocks, |value| (value).map_identities(mapper))?,
            entry: self.entry,
            effects: self.effects,
            debug: self.debug.clone(),
        })
    }
    fn visit_definitions(
        &self,
        visit: &mut impl FnMut(&I) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        self.semantic.visit_definitions(visit, cancel)?;
        self.instance.visit_definitions(visit, cancel)?;
        for value0 in &self.blocks {
            (value0).visit_definitions(visit, cancel)?;
        }
        Ok(())
    }
}

impl<I: DefinitionReference> DefinitionRecord<I> for BasicBlock<I> {
    type Rebind<J: DefinitionReference> = BasicBlock<J>;
    fn map_identities<J: DefinitionReference>(
        &self,
        mapper: &mut DefinitionMapper<'_, I, J>,
    ) -> Result<Self::Rebind<J>, DefinitionMappingError> {
        mapper.check()?;
        Ok(BasicBlock {
            instructions: map_sequence(&self.instructions, |value| (value).map_identities(mapper))?,
            instruction_spans: self.instruction_spans.clone(),
            instruction_scopes: self.instruction_scopes.clone(),
            terminator: self.terminator.clone(),
            terminator_span: self.terminator_span,
            terminator_scope: self.terminator_scope,
        })
    }
    fn visit_definitions(
        &self,
        visit: &mut impl FnMut(&I) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        for value0 in &self.instructions {
            (value0).visit_definitions(visit, cancel)?;
        }
        Ok(())
    }
}
