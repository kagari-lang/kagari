//! Contextual identity traversal of the owning metadata records.
use crate::module::{
    BytecodeFunction, BytecodeModule, FunctionMetadata, FunctionRecord, InterfaceMethodSlot,
    InterfaceParentRecord, InterfaceResultAdapter, InterfaceTableRecord, InterfaceViewRecord,
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        mapping::{
            DefinitionMapper, DefinitionMappingError, DefinitionRecord, check_cancel, map_sequence,
        },
        reference::DefinitionReference,
    },
};

impl<I: DefinitionReference> DefinitionRecord<I> for BytecodeModule<I> {
    type Rebind<J: DefinitionReference> = BytecodeModule<J>;

    fn map_identities<J: DefinitionReference>(
        &self,
        mapper: &mut DefinitionMapper<'_, I, J>,
    ) -> Result<Self::Rebind<J>, DefinitionMappingError> {
        mapper.check()?;
        Ok(BytecodeModule {
            dependencies: self.dependencies.clone(),
            host_interface: self.host_interface.map_identities(mapper)?,
            native_imports: map_sequence(&self.native_imports, |value| {
                (value).map_identities(mapper)
            })?,
            native_declarations: map_sequence(&self.native_declarations, |value| {
                (value).map_identities(mapper)
            })?,
            identity: self.identity.clone(),
            source_name: self.source_name.clone(),
            module_slots: self.module_slots.clone(),
            constants: self.constants.clone(),
            types: self.types.clone(),
            structures: map_sequence(&self.structures, |value| (value).map_identities(mapper))?,
            enumerations: map_sequence(&self.enumerations, |value| (value).map_identities(mapper))?,
            interface_tables: map_sequence(&self.interface_tables, |value| {
                (value).map_identities(mapper)
            })?,
            paths: self.paths.clone(),
            function_table: map_sequence(&self.function_table, |value| {
                (value).map_identities(mapper)
            })?,
            public_items: map_sequence(&self.public_items, |value| (value).map_identities(mapper))?,
            trait_contracts: map_sequence(&self.trait_contracts, |value| {
                (value).map_identities(mapper)
            })?,
            functions: map_sequence(&self.functions, |value| (value).map_identities(mapper))?,
        })
    }

    fn visit_definitions(
        &self,
        visit: &mut impl FnMut(&I) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        self.host_interface.visit_definitions(visit, cancel)?;
        for value0 in &self.native_imports {
            (value0).visit_definitions(visit, cancel)?;
        }
        for value0 in &self.native_declarations {
            (value0).visit_definitions(visit, cancel)?;
        }
        for value0 in &self.structures {
            (value0).visit_definitions(visit, cancel)?;
        }
        for value0 in &self.enumerations {
            (value0).visit_definitions(visit, cancel)?;
        }
        for value0 in &self.interface_tables {
            (value0).visit_definitions(visit, cancel)?;
        }
        for value0 in &self.function_table {
            (value0).visit_definitions(visit, cancel)?;
        }
        for value0 in &self.public_items {
            (value0).visit_definitions(visit, cancel)?;
        }
        for value0 in &self.trait_contracts {
            (value0).visit_definitions(visit, cancel)?;
        }
        for value0 in &self.functions {
            (value0).visit_definitions(visit, cancel)?;
        }
        Ok(())
    }
}

impl<I: DefinitionReference> DefinitionRecord<I> for BytecodeFunction<I> {
    type Rebind<J: DefinitionReference> = BytecodeFunction<J>;

    fn map_identities<J: DefinitionReference>(
        &self,
        mapper: &mut DefinitionMapper<'_, I, J>,
    ) -> Result<Self::Rebind<J>, DefinitionMappingError> {
        mapper.check()?;
        Ok(BytecodeFunction {
            id: self.id,
            identity: self
                .identity
                .as_ref()
                .map(|value| (value).map_identities(mapper))
                .transpose()?,
            name: self.name.clone(),
            parameter_count: self.parameter_count,
            register_count: self.register_count,
            local_count: self.local_count,
            metadata: self.metadata.map_identities(mapper)?,
            instructions: map_sequence(&self.instructions, |value| (value).map_identities(mapper))?,
        })
    }

    fn visit_definitions(
        &self,
        visit: &mut impl FnMut(&I) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        if let Some(value0) = self.identity.as_ref() {
            (value0).visit_definitions(visit, cancel)?;
        }
        self.metadata.visit_definitions(visit, cancel)?;
        for value0 in &self.instructions {
            (value0).visit_definitions(visit, cancel)?;
        }
        Ok(())
    }
}

impl<I: DefinitionReference> DefinitionRecord<I> for FunctionMetadata<I> {
    type Rebind<J: DefinitionReference> = FunctionMetadata<J>;

    fn map_identities<J: DefinitionReference>(
        &self,
        mapper: &mut DefinitionMapper<'_, I, J>,
    ) -> Result<Self::Rebind<J>, DefinitionMappingError> {
        mapper.check()?;
        Ok(FunctionMetadata {
            semantic: self.semantic.map_identities(mapper)?,
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

    fn visit_definitions(
        &self,
        visit: &mut impl FnMut(&I) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        self.semantic.visit_definitions(visit, cancel)?;
        Ok(())
    }
}

impl<I: DefinitionReference> DefinitionRecord<I> for FunctionRecord<I> {
    type Rebind<J: DefinitionReference> = FunctionRecord<J>;

    fn map_identities<J: DefinitionReference>(
        &self,
        mapper: &mut DefinitionMapper<'_, I, J>,
    ) -> Result<Self::Rebind<J>, DefinitionMappingError> {
        mapper.check()?;
        Ok(FunctionRecord {
            id: self.id,
            identity: self
                .identity
                .as_ref()
                .map(|value| (value).map_identities(mapper))
                .transpose()?,
            name: self.name.clone(),
            params: self.params.clone(),
            return_type: self.return_type,
            effects: self.effects,
        })
    }

    fn visit_definitions(
        &self,
        visit: &mut impl FnMut(&I) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        if let Some(value0) = self.identity.as_ref() {
            (value0).visit_definitions(visit, cancel)?;
        }
        Ok(())
    }
}

impl<I: DefinitionReference> DefinitionRecord<I> for InterfaceTableRecord<I> {
    type Rebind<J: DefinitionReference> = InterfaceTableRecord<J>;

    fn map_identities<J: DefinitionReference>(
        &self,
        mapper: &mut DefinitionMapper<'_, I, J>,
    ) -> Result<Self::Rebind<J>, DefinitionMappingError> {
        mapper.check()?;
        Ok(InterfaceTableRecord {
            view: self
                .view
                .as_ref()
                .map(|value| (value).map_identities(mapper))
                .transpose()?,
            arguments: map_sequence(&self.arguments, |value| (value).map_identities(mapper))?,
            declaration: mapper.reference(&self.declaration)?,
            methods: map_sequence(&self.methods, |value| (value).map_identities(mapper))?,
            parents: map_sequence(&self.parents, |value| (value).map_identities(mapper))?,
        })
    }

    fn visit_definitions(
        &self,
        visit: &mut impl FnMut(&I) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        if let Some(value0) = self.view.as_ref() {
            (value0).visit_definitions(visit, cancel)?;
        }
        for value0 in &self.arguments {
            (value0).visit_definitions(visit, cancel)?;
        }
        check_cancel(cancel)?;
        visit(&self.declaration)?;
        for value0 in &self.methods {
            (value0).visit_definitions(visit, cancel)?;
        }
        for value0 in &self.parents {
            (value0).visit_definitions(visit, cancel)?;
        }
        Ok(())
    }
}

impl<I: DefinitionReference> DefinitionRecord<I> for InterfaceParentRecord<I> {
    type Rebind<J: DefinitionReference> = InterfaceParentRecord<J>;

    fn map_identities<J: DefinitionReference>(
        &self,
        mapper: &mut DefinitionMapper<'_, I, J>,
    ) -> Result<Self::Rebind<J>, DefinitionMappingError> {
        mapper.check()?;
        Ok(InterfaceParentRecord {
            interface: self.interface.map_identities(mapper)?,
            implementation: self.implementation.map_identities(mapper)?,
            view: self.view,
        })
    }

    fn visit_definitions(
        &self,
        visit: &mut impl FnMut(&I) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        self.interface.visit_definitions(visit, cancel)?;
        self.implementation.visit_definitions(visit, cancel)?;
        Ok(())
    }
}

impl<I: DefinitionReference> DefinitionRecord<I> for InterfaceViewRecord<I> {
    type Rebind<J: DefinitionReference> = InterfaceViewRecord<J>;

    fn map_identities<J: DefinitionReference>(
        &self,
        mapper: &mut DefinitionMapper<'_, I, J>,
    ) -> Result<Self::Rebind<J>, DefinitionMappingError> {
        mapper.check()?;
        Ok(InterfaceViewRecord {
            interface: self.interface.map_identities(mapper)?,
            results: map_sequence(&self.results, |value| (value).map_identities(mapper))?,
        })
    }

    fn visit_definitions(
        &self,
        visit: &mut impl FnMut(&I) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        self.interface.visit_definitions(visit, cancel)?;
        for value0 in &self.results {
            (value0).visit_definitions(visit, cancel)?;
        }
        Ok(())
    }
}

impl<I: DefinitionReference> DefinitionRecord<I> for InterfaceResultAdapter<I> {
    type Rebind<J: DefinitionReference> = InterfaceResultAdapter<J>;

    fn map_identities<J: DefinitionReference>(
        &self,
        mapper: &mut DefinitionMapper<'_, I, J>,
    ) -> Result<Self::Rebind<J>, DefinitionMappingError> {
        mapper.check()?;
        Ok(InterfaceResultAdapter {
            method: mapper.reference(&self.method)?,
            implementation: self.implementation.map_identities(mapper)?,
        })
    }

    fn visit_definitions(
        &self,
        visit: &mut impl FnMut(&I) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        visit(&self.method)?;
        self.implementation.visit_definitions(visit, cancel)?;
        Ok(())
    }
}

impl<I: DefinitionReference> DefinitionRecord<I> for InterfaceMethodSlot<I> {
    type Rebind<J: DefinitionReference> = InterfaceMethodSlot<J>;

    fn map_identities<J: DefinitionReference>(
        &self,
        mapper: &mut DefinitionMapper<'_, I, J>,
    ) -> Result<Self::Rebind<J>, DefinitionMappingError> {
        mapper.check()?;
        Ok(InterfaceMethodSlot {
            method: mapper.reference(&self.method)?,
            target: self.target,
            arguments: map_sequence(&self.arguments, |value| (value).map_identities(mapper))?,
        })
    }

    fn visit_definitions(
        &self,
        visit: &mut impl FnMut(&I) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        visit(&self.method)?;
        for value0 in &self.arguments {
            (value0).visit_definitions(visit, cancel)?;
        }
        Ok(())
    }
}
