//! Contextual identity traversal of the owning metadata records.
use crate::module::{
    BytecodeFunction, BytecodeModule, FunctionMetadata, FunctionRecord, InterfaceMethodSlot,
    InterfaceParentRecord, InterfaceResultAdapter, InterfaceTableRecord, InterfaceViewRecord,
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        mapping::{DefinitionMapper, DefinitionMappingError, check_cancel, map_sequence},
        scoped::{DefinitionReferences, RemapDefinitions},
        table::DefinitionId,
    },
};

impl RemapDefinitions for BytecodeModule {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_>,
    ) -> Result<Self, DefinitionMappingError> {
        mapper.check()?;
        Ok(BytecodeModule {
            dependencies: self.dependencies.clone(),
            external_types: map_sequence(&self.external_types, |value| {
                value.remap_definitions(mapper)
            })?,
            host_interface: self.host_interface.remap_definitions(mapper)?,
            native_imports: map_sequence(&self.native_imports, |value| {
                (value).remap_definitions(mapper)
            })?,
            native_declarations: map_sequence(&self.native_declarations, |value| {
                (value).remap_definitions(mapper)
            })?,
            identity: self.identity.clone(),
            source_name: self.source_name.clone(),
            module_slots: self.module_slots.clone(),
            constants: self.constants.clone(),
            types: self.types.clone(),
            structures: map_sequence(&self.structures, |value| (value).remap_definitions(mapper))?,
            enumerations: map_sequence(&self.enumerations, |value| {
                (value).remap_definitions(mapper)
            })?,
            interface_tables: map_sequence(&self.interface_tables, |value| {
                (value).remap_definitions(mapper)
            })?,
            paths: self.paths.clone(),
            function_table: map_sequence(&self.function_table, |value| {
                (value).remap_definitions(mapper)
            })?,
            public_items: map_sequence(&self.public_items, |value| {
                (value).remap_definitions(mapper)
            })?,
            trait_contracts: map_sequence(&self.trait_contracts, |value| {
                (value).remap_definitions(mapper)
            })?,
            functions: map_sequence(&self.functions, |value| (value).remap_definitions(mapper))?,
        })
    }
}

impl DefinitionReferences for BytecodeModule {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        for value in &self.external_types {
            value.visit_definition_ids(visit, cancel)?;
        }
        self.host_interface.visit_definition_ids(visit, cancel)?;
        for value0 in &self.native_imports {
            (value0).visit_definition_ids(visit, cancel)?;
        }
        for value0 in &self.native_declarations {
            (value0).visit_definition_ids(visit, cancel)?;
        }
        for value0 in &self.structures {
            (value0).visit_definition_ids(visit, cancel)?;
        }
        for value0 in &self.enumerations {
            (value0).visit_definition_ids(visit, cancel)?;
        }
        for value0 in &self.interface_tables {
            (value0).visit_definition_ids(visit, cancel)?;
        }
        for value0 in &self.function_table {
            (value0).visit_definition_ids(visit, cancel)?;
        }
        for value0 in &self.public_items {
            (value0).visit_definition_ids(visit, cancel)?;
        }
        for value0 in &self.trait_contracts {
            (value0).visit_definition_ids(visit, cancel)?;
        }
        for value0 in &self.functions {
            (value0).visit_definition_ids(visit, cancel)?;
        }
        Ok(())
    }
}

impl RemapDefinitions for BytecodeFunction {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_>,
    ) -> Result<Self, DefinitionMappingError> {
        mapper.check()?;
        Ok(BytecodeFunction {
            id: self.id,
            identity: self
                .identity
                .as_ref()
                .map(|value| (value).remap_definitions(mapper))
                .transpose()?,
            name: self.name.clone(),
            parameter_count: self.parameter_count,
            register_count: self.register_count,
            local_count: self.local_count,
            metadata: self.metadata.remap_definitions(mapper)?,
            instructions: map_sequence(&self.instructions, |value| {
                (value).remap_definitions(mapper)
            })?,
        })
    }
}

impl DefinitionReferences for BytecodeFunction {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        if let Some(value0) = self.identity.as_ref() {
            (value0).visit_definition_ids(visit, cancel)?;
        }
        self.metadata.visit_definition_ids(visit, cancel)?;
        for value0 in &self.instructions {
            (value0).visit_definition_ids(visit, cancel)?;
        }
        Ok(())
    }
}

impl RemapDefinitions for FunctionMetadata {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_>,
    ) -> Result<Self, DefinitionMappingError> {
        mapper.check()?;
        Ok(FunctionMetadata {
            semantic: self.semantic.remap_definitions(mapper)?,
            params: self.params.clone(),
            return_type: self.return_type,
            locals: self.locals.clone(),
            registers: self.registers.clone(),
            roots: self.roots.clone(),
            control_flow_targets: self.control_flow_targets.clone(),
            effects: self.effects,
            debug: self.debug.clone(),
        })
    }
}

impl DefinitionReferences for FunctionMetadata {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        self.semantic.visit_definition_ids(visit, cancel)?;
        Ok(())
    }
}

impl RemapDefinitions for FunctionRecord {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_>,
    ) -> Result<Self, DefinitionMappingError> {
        mapper.check()?;
        Ok(FunctionRecord {
            id: self.id,
            identity: self
                .identity
                .as_ref()
                .map(|value| (value).remap_definitions(mapper))
                .transpose()?,
            name: self.name.clone(),
            params: self.params.clone(),
            return_type: self.return_type,
            effects: self.effects,
        })
    }
}

impl DefinitionReferences for FunctionRecord {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        if let Some(value0) = self.identity.as_ref() {
            (value0).visit_definition_ids(visit, cancel)?;
        }
        Ok(())
    }
}

impl RemapDefinitions for InterfaceTableRecord {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_>,
    ) -> Result<Self, DefinitionMappingError> {
        mapper.check()?;
        Ok(InterfaceTableRecord {
            view: self
                .view
                .as_ref()
                .map(|value| (value).remap_definitions(mapper))
                .transpose()?,
            arguments: map_sequence(&self.arguments, |value| (value).remap_definitions(mapper))?,
            declaration: mapper.reference(&self.declaration)?,
            methods: map_sequence(&self.methods, |value| (value).remap_definitions(mapper))?,
            parents: map_sequence(&self.parents, |value| (value).remap_definitions(mapper))?,
        })
    }
}

impl DefinitionReferences for InterfaceTableRecord {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        if let Some(value0) = self.view.as_ref() {
            (value0).visit_definition_ids(visit, cancel)?;
        }
        for value0 in &self.arguments {
            (value0).visit_definition_ids(visit, cancel)?;
        }
        check_cancel(cancel)?;
        visit(&self.declaration)?;
        for value0 in &self.methods {
            (value0).visit_definition_ids(visit, cancel)?;
        }
        for value0 in &self.parents {
            (value0).visit_definition_ids(visit, cancel)?;
        }
        Ok(())
    }
}

impl RemapDefinitions for InterfaceParentRecord {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_>,
    ) -> Result<Self, DefinitionMappingError> {
        mapper.check()?;
        Ok(InterfaceParentRecord {
            interface: self.interface.remap_definitions(mapper)?,
            implementation: self.implementation.remap_definitions(mapper)?,
            view: self.view,
        })
    }
}

impl DefinitionReferences for InterfaceParentRecord {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        self.interface.visit_definition_ids(visit, cancel)?;
        self.implementation.visit_definition_ids(visit, cancel)?;
        Ok(())
    }
}

impl RemapDefinitions for InterfaceViewRecord {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_>,
    ) -> Result<Self, DefinitionMappingError> {
        mapper.check()?;
        Ok(InterfaceViewRecord {
            interface: self.interface.remap_definitions(mapper)?,
            results: map_sequence(&self.results, |value| (value).remap_definitions(mapper))?,
        })
    }
}

impl DefinitionReferences for InterfaceViewRecord {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        self.interface.visit_definition_ids(visit, cancel)?;
        for value0 in &self.results {
            (value0).visit_definition_ids(visit, cancel)?;
        }
        Ok(())
    }
}

impl RemapDefinitions for InterfaceResultAdapter {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_>,
    ) -> Result<Self, DefinitionMappingError> {
        mapper.check()?;
        Ok(InterfaceResultAdapter {
            method: mapper.reference(&self.method)?,
            implementation: self.implementation.remap_definitions(mapper)?,
        })
    }
}

impl DefinitionReferences for InterfaceResultAdapter {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        visit(&self.method)?;
        self.implementation.visit_definition_ids(visit, cancel)?;
        Ok(())
    }
}

impl RemapDefinitions for InterfaceMethodSlot {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_>,
    ) -> Result<Self, DefinitionMappingError> {
        mapper.check()?;
        Ok(InterfaceMethodSlot {
            method: mapper.reference(&self.method)?,
            target: self.target,
            arguments: map_sequence(&self.arguments, |value| (value).remap_definitions(mapper))?,
        })
    }
}

impl DefinitionReferences for InterfaceMethodSlot {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        visit(&self.method)?;
        for value0 in &self.arguments {
            (value0).visit_definition_ids(visit, cancel)?;
        }
        Ok(())
    }
}
