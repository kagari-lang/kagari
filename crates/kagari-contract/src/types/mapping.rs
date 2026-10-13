//! Contextual identity traversal of the owning metadata records.
use crate::types::{
    ConcreteFunctionIdentity, InterfaceTable, ModuleContract, PublicItem, TraitContract,
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        mapping::{DefinitionMapper, DefinitionMappingError, check_cancel, map_sequence},
        scoped::{DefinitionReferences, RemapDefinitions},
        table::DefinitionId,
    },
};

impl RemapDefinitions for ModuleContract {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_>,
    ) -> Result<Self, DefinitionMappingError> {
        mapper.check()?;
        Ok(ModuleContract {
            native_declarations: map_sequence(&self.native_declarations, |value| {
                (value).remap_definitions(mapper)
            })?,
            public_items: map_sequence(&self.public_items, |value| {
                (value).remap_definitions(mapper)
            })?,
            trait_contracts: map_sequence(&self.trait_contracts, |value| {
                (value).remap_definitions(mapper)
            })?,
        })
    }
}

impl DefinitionReferences for ModuleContract {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        for value0 in &self.native_declarations {
            (value0).visit_definition_ids(visit, cancel)?;
        }
        for value0 in &self.public_items {
            (value0).visit_definition_ids(visit, cancel)?;
        }
        for value0 in &self.trait_contracts {
            (value0).visit_definition_ids(visit, cancel)?;
        }
        Ok(())
    }
}

impl RemapDefinitions for PublicItem {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_>,
    ) -> Result<Self, DefinitionMappingError> {
        mapper.check()?;
        Ok(match self {
            Self::Function(field0) => PublicItem::Function((field0).remap_definitions(mapper)?),
            Self::Const(field0) => PublicItem::Const((field0).remap_definitions(mapper)?),
            Self::Type(field0) => PublicItem::Type((field0).remap_definitions(mapper)?),
            Self::Trait(field0) => PublicItem::Trait((field0).remap_definitions(mapper)?),
            Self::InherentTable(table) => {
                PublicItem::InherentTable(Box::new(table.remap_definitions(mapper)?))
            }
            Self::InterfaceTable(field0) => {
                PublicItem::InterfaceTable(Box::new(((field0).as_ref()).remap_definitions(mapper)?))
            }
        })
    }
}

impl DefinitionReferences for PublicItem {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        match self {
            Self::Function(field0) => {
                (field0).visit_definition_ids(visit, cancel)?;
            }
            Self::Const(field0) => {
                (field0).visit_definition_ids(visit, cancel)?;
            }
            Self::Type(field0) => {
                (field0).visit_definition_ids(visit, cancel)?;
            }
            Self::Trait(field0) => {
                (field0).visit_definition_ids(visit, cancel)?;
            }
            Self::InherentTable(table) => table.visit_definition_ids(visit, cancel)?,
            Self::InterfaceTable(field0) => {
                ((field0).as_ref()).visit_definition_ids(visit, cancel)?;
            }
        }
        Ok(())
    }
}

impl RemapDefinitions for InterfaceTable {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_>,
    ) -> Result<Self, DefinitionMappingError> {
        mapper.check()?;
        Ok(InterfaceTable {
            associated_type_families: map_sequence(&self.associated_type_families, |value| {
                (value).remap_definitions(mapper)
            })?,
            associated_consts: map_sequence(&self.associated_consts, |value| {
                (value).remap_definitions(mapper)
            })?,
            bridge: self.bridge,
            declaration: mapper.reference(&self.declaration)?,
            name: self.name.clone(),
            generic_params: map_sequence(&self.generic_params, |value| {
                (value).remap_definitions(mapper)
            })?,
            bounds: map_sequence(&self.bounds, |value| (value).remap_definitions(mapper))?,
            trait_type: self.trait_type.remap_definitions(mapper)?,
            for_type: self.for_type.remap_definitions(mapper)?,
            methods: map_sequence(&self.methods, |value| (value).remap_definitions(mapper))?,
        })
    }
}

impl DefinitionReferences for InterfaceTable {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        for value0 in &self.associated_type_families {
            (value0).visit_definition_ids(visit, cancel)?;
        }
        for value0 in &self.associated_consts {
            (value0).visit_definition_ids(visit, cancel)?;
        }
        check_cancel(cancel)?;
        visit(&self.declaration)?;
        for value0 in &self.generic_params {
            (value0).visit_definition_ids(visit, cancel)?;
        }
        for value0 in &self.bounds {
            (value0).visit_definition_ids(visit, cancel)?;
        }
        self.trait_type.visit_definition_ids(visit, cancel)?;
        self.for_type.visit_definition_ids(visit, cancel)?;
        for value0 in &self.methods {
            (value0).visit_definition_ids(visit, cancel)?;
        }
        Ok(())
    }
}

impl RemapDefinitions for TraitContract {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_>,
    ) -> Result<Self, DefinitionMappingError> {
        mapper.check()?;
        Ok(TraitContract {
            declaration: mapper.reference(&self.declaration)?,
            abi: self.abi.remap_definitions(mapper)?,
        })
    }
}

impl DefinitionReferences for TraitContract {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        visit(&self.declaration)?;
        self.abi.visit_definition_ids(visit, cancel)?;
        Ok(())
    }
}

impl RemapDefinitions for ConcreteFunctionIdentity {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_>,
    ) -> Result<Self, DefinitionMappingError> {
        mapper.check()?;
        Ok(ConcreteFunctionIdentity {
            declaration: mapper.reference(&self.declaration)?,
            arguments: map_sequence(&self.arguments, |value| (value).remap_definitions(mapper))?,
        })
    }
}

impl DefinitionReferences for ConcreteFunctionIdentity {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        visit(&self.declaration)?;
        for value0 in &self.arguments {
            (value0).visit_definition_ids(visit, cancel)?;
        }
        Ok(())
    }
}
