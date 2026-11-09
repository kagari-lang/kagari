//! Reload comparison carries contracts, not ordinary executable images.
use kagari_bytecode::module::{BytecodeModule, PathRecord};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        ModuleIdentity,
        mapping::{DefinitionMapper, DefinitionMappingError},
        scoped::{DefinitionReferences, RemapDefinitions},
        table::DefinitionId,
    },
};
use kagari_contract::{
    statics::StaticContract,
    types::{PublicItem, TraitContract, named::NamedDeclaration},
};
use kagari_types::declaration::{NativeDeclaration, TypeDeclaration};

pub(super) struct ModuleContracts {
    pub(super) identity: ModuleIdentity,
    pub(super) paths: Vec<PathRecord>,
    pub(super) named_declarations: Vec<NamedDeclaration>,
    pub(super) private_types: Vec<TypeDeclaration>,
    pub(super) trait_contracts: Vec<TraitContract>,
    pub(super) native_declarations: Vec<NativeDeclaration>,
    pub(super) public_items: Vec<PublicItem>,
    pub(super) statics: Vec<StaticContract>,
    pub(super) ambiguous: Vec<BytecodeModule>,
}

impl ModuleContracts {
    pub(super) fn prepare(module: &BytecodeModule) -> Self {
        Self {
            identity: module.identity.clone(),
            paths: module.paths.clone(),
            statics: module
                .module_slots
                .iter()
                .map(|slot| slot.contract.clone())
                .collect(),
            // Ambiguous same-name locals have an exact indexed-body contract.
            // Only these modules need their original operand/body evidence. This
            // is temporary validation input, never installed or executable code.
            ambiguous: if module
                .named_declarations
                .iter()
                .any(|record| record.ambiguous_local_body.is_some())
            {
                vec![module.clone()]
            } else {
                Vec::new()
            },
            named_declarations: module.named_declarations.clone(),
            private_types: module.private_types.clone(),
            trait_contracts: module.trait_contracts.clone(),
            native_declarations: module.native_declarations.clone(),
            public_items: module.public_items.clone(),
        }
    }
}

impl DefinitionReferences for ModuleContracts {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        self.named_declarations
            .visit_definition_ids(visit, cancel)?;
        self.private_types.visit_definition_ids(visit, cancel)?;
        self.trait_contracts.visit_definition_ids(visit, cancel)?;
        self.native_declarations
            .visit_definition_ids(visit, cancel)?;
        self.public_items.visit_definition_ids(visit, cancel)?;
        self.statics.visit_definition_ids(visit, cancel)?;
        self.ambiguous.visit_definition_ids(visit, cancel)?;
        Ok(())
    }
}

impl RemapDefinitions for ModuleContracts {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_>,
    ) -> Result<Self, DefinitionMappingError> {
        mapper.check()?;
        Ok(Self {
            identity: self.identity.clone(),
            paths: self.paths.clone(),
            named_declarations: self.named_declarations.remap_definitions(mapper)?,
            private_types: self.private_types.remap_definitions(mapper)?,
            trait_contracts: self.trait_contracts.remap_definitions(mapper)?,
            native_declarations: self.native_declarations.remap_definitions(mapper)?,
            public_items: self.public_items.remap_definitions(mapper)?,
            statics: self.statics.remap_definitions(mapper)?,
            ambiguous: self.ambiguous.remap_definitions(mapper)?,
        })
    }
}
