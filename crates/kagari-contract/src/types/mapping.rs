//! Contextual identity traversal of the owning metadata records.
use crate::types::{
    ConcreteFunctionIdentity, InterfaceTable, ModuleContract, PublicItem, TraitContract,
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

impl<I: DefinitionReference> DefinitionRecord<I> for ModuleContract<I> {
    type Rebind<J: DefinitionReference> = ModuleContract<J>;

    fn map_identities<J: DefinitionReference>(
        &self,
        mapper: &mut DefinitionMapper<'_, I, J>,
    ) -> Result<Self::Rebind<J>, DefinitionMappingError> {
        mapper.check()?;
        Ok(ModuleContract {
            native_declarations: map_sequence(&self.native_declarations, |value| {
                (value).map_identities(mapper)
            })?,
            public_items: map_sequence(&self.public_items, |value| (value).map_identities(mapper))?,
            trait_contracts: map_sequence(&self.trait_contracts, |value| {
                (value).map_identities(mapper)
            })?,
        })
    }

    fn visit_definitions(
        &self,
        visit: &mut impl FnMut(&I) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        for value0 in &self.native_declarations {
            (value0).visit_definitions(visit, cancel)?;
        }
        for value0 in &self.public_items {
            (value0).visit_definitions(visit, cancel)?;
        }
        for value0 in &self.trait_contracts {
            (value0).visit_definitions(visit, cancel)?;
        }
        Ok(())
    }
}

impl<I: DefinitionReference> DefinitionRecord<I> for PublicItem<I> {
    type Rebind<J: DefinitionReference> = PublicItem<J>;

    fn map_identities<J: DefinitionReference>(
        &self,
        mapper: &mut DefinitionMapper<'_, I, J>,
    ) -> Result<Self::Rebind<J>, DefinitionMappingError> {
        mapper.check()?;
        Ok(match self {
            Self::Function(field0) => PublicItem::Function((field0).map_identities(mapper)?),
            Self::Const(field0) => PublicItem::Const((field0).map_identities(mapper)?),
            Self::Type(field0) => PublicItem::Type((field0).map_identities(mapper)?),
            Self::Trait(field0) => PublicItem::Trait((field0).map_identities(mapper)?),
            Self::InherentTable(table) => {
                PublicItem::InherentTable(Box::new(table.map_identities(mapper)?))
            }
            Self::InterfaceTable(field0) => {
                PublicItem::InterfaceTable(Box::new(((field0).as_ref()).map_identities(mapper)?))
            }
        })
    }

    fn visit_definitions(
        &self,
        visit: &mut impl FnMut(&I) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        match self {
            Self::Function(field0) => {
                (field0).visit_definitions(visit, cancel)?;
            }
            Self::Const(field0) => {
                (field0).visit_definitions(visit, cancel)?;
            }
            Self::Type(field0) => {
                (field0).visit_definitions(visit, cancel)?;
            }
            Self::Trait(field0) => {
                (field0).visit_definitions(visit, cancel)?;
            }
            Self::InherentTable(table) => table.visit_definitions(visit, cancel)?,
            Self::InterfaceTable(field0) => {
                ((field0).as_ref()).visit_definitions(visit, cancel)?;
            }
        }
        Ok(())
    }
}

impl<I: DefinitionReference> DefinitionRecord<I> for InterfaceTable<I> {
    type Rebind<J: DefinitionReference> = InterfaceTable<J>;

    fn map_identities<J: DefinitionReference>(
        &self,
        mapper: &mut DefinitionMapper<'_, I, J>,
    ) -> Result<Self::Rebind<J>, DefinitionMappingError> {
        mapper.check()?;
        Ok(InterfaceTable {
            associated_type_families: map_sequence(&self.associated_type_families, |value| {
                (value).map_identities(mapper)
            })?,
            associated_consts: map_sequence(&self.associated_consts, |value| {
                (value).map_identities(mapper)
            })?,
            host_bridge: self.host_bridge,
            declaration: mapper.reference(&self.declaration)?,
            name: self.name.clone(),
            generic_params: map_sequence(&self.generic_params, |value| {
                (value).map_identities(mapper)
            })?,
            bounds: map_sequence(&self.bounds, |value| (value).map_identities(mapper))?,
            trait_type: self.trait_type.map_identities(mapper)?,
            for_type: self.for_type.map_identities(mapper)?,
            methods: map_sequence(&self.methods, |value| (value).map_identities(mapper))?,
        })
    }

    fn visit_definitions(
        &self,
        visit: &mut impl FnMut(&I) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        for value0 in &self.associated_type_families {
            (value0).visit_definitions(visit, cancel)?;
        }
        for value0 in &self.associated_consts {
            (value0).visit_definitions(visit, cancel)?;
        }
        check_cancel(cancel)?;
        visit(&self.declaration)?;
        for value0 in &self.generic_params {
            (value0).visit_definitions(visit, cancel)?;
        }
        for value0 in &self.bounds {
            (value0).visit_definitions(visit, cancel)?;
        }
        self.trait_type.visit_definitions(visit, cancel)?;
        self.for_type.visit_definitions(visit, cancel)?;
        for value0 in &self.methods {
            (value0).visit_definitions(visit, cancel)?;
        }
        Ok(())
    }
}

impl<I: DefinitionReference> DefinitionRecord<I> for TraitContract<I> {
    type Rebind<J: DefinitionReference> = TraitContract<J>;

    fn map_identities<J: DefinitionReference>(
        &self,
        mapper: &mut DefinitionMapper<'_, I, J>,
    ) -> Result<Self::Rebind<J>, DefinitionMappingError> {
        mapper.check()?;
        Ok(TraitContract {
            declaration: mapper.reference(&self.declaration)?,
            abi: self.abi.map_identities(mapper)?,
        })
    }

    fn visit_definitions(
        &self,
        visit: &mut impl FnMut(&I) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        visit(&self.declaration)?;
        self.abi.visit_definitions(visit, cancel)?;
        Ok(())
    }
}

impl<I: DefinitionReference> DefinitionRecord<I> for ConcreteFunctionIdentity<I> {
    type Rebind<J: DefinitionReference> = ConcreteFunctionIdentity<J>;

    fn map_identities<J: DefinitionReference>(
        &self,
        mapper: &mut DefinitionMapper<'_, I, J>,
    ) -> Result<Self::Rebind<J>, DefinitionMappingError> {
        mapper.check()?;
        Ok(ConcreteFunctionIdentity {
            declaration: mapper.reference(&self.declaration)?,
            arguments: map_sequence(&self.arguments, |value| (value).map_identities(mapper))?,
        })
    }

    fn visit_definitions(
        &self,
        visit: &mut impl FnMut(&I) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        visit(&self.declaration)?;
        for value0 in &self.arguments {
            (value0).visit_definitions(visit, cancel)?;
        }
        Ok(())
    }
}
