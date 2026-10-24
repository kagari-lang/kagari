//! Candidate-owned proof contracts projected from independently verified bytecode.
use kagari_bytecode::module::BytecodeModule;
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        DefinitionKind,
        mapping::{DefinitionMapper, DefinitionMappingError},
        scoped::{DefinitionReferences, RemapDefinitions},
        table::{DefinitionId, DefinitionTable},
    },
};
use kagari_contract::{
    layout::EnumLayout,
    types::{
        InterfaceTable, PublicItem, TraitContract,
        proofs::{ProofCatalog, implementation::Implementation},
    },
};
use kagari_types::{declaration::NativeDeclaration, ty::substitution::TypeTransformError};

/// Own only portable contract records; proof searches remain in the shared catalog.
#[derive(Debug)]
pub(super) struct ProofFacts {
    interfaces: Vec<InterfaceTable>,
    enumerations: Vec<EnumLayout>,
    traits: Vec<TraitContract>,
    natives: Vec<NativeDeclaration>,
}

impl ProofFacts {
    pub(super) fn from_checked(modules: &[BytecodeModule], definitions: &DefinitionTable) -> Self {
        let mut facts = Self {
            interfaces: Vec::new(),
            enumerations: Vec::new(),
            traits: Vec::new(),
            natives: Vec::new(),
        };
        for module in modules {
            facts
                .enumerations
                .extend(module.enumerations.iter().cloned());
            facts.traits.extend(module.trait_contracts.iter().cloned());
            facts
                .natives
                .extend(module.native_declarations.iter().cloned());
            for item in &module.public_items {
                match item {
                    PublicItem::InterfaceTable(table) => {
                        facts.interfaces.push(table.as_ref().clone())
                    }
                    PublicItem::Trait(abi) => {
                        let root = definitions
                            .lookup_root(&module.identity)
                            .expect("verified public trait module identity");
                        let declaration = definitions
                            .lookup_child(root, DefinitionKind::Trait, &abi.name, 0)
                            .expect("verified public trait declaration identity");
                        facts.traits.push(TraitContract {
                            declaration,
                            abi: abi.clone(),
                        });
                    }
                    _ => {}
                }
            }
        }
        facts
    }

    pub(super) fn catalog(
        &self,
        definitions: &DefinitionTable,
        cancel: &CancellationToken,
    ) -> Result<ProofCatalog<'_>, TypeTransformError> {
        ProofCatalog::new(
            self.interfaces
                .iter()
                .map(Implementation::Interface)
                .collect(),
            &self.enumerations,
            self.traits
                .iter()
                .map(|record| (record.declaration, &record.abi)),
            &self.natives,
            definitions,
            cancel,
        )
    }
}

impl DefinitionReferences for ProofFacts {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        self.interfaces.visit_definition_ids(visit, cancel)?;
        self.enumerations.visit_definition_ids(visit, cancel)?;
        self.traits.visit_definition_ids(visit, cancel)?;
        self.natives.visit_definition_ids(visit, cancel)
    }
}

impl RemapDefinitions for ProofFacts {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_>,
    ) -> Result<Self, DefinitionMappingError> {
        Ok(Self {
            interfaces: self.interfaces.remap_definitions(mapper)?,
            enumerations: self.enumerations.remap_definitions(mapper)?,
            traits: self.traits.remap_definitions(mapper)?,
            natives: self.natives.remap_definitions(mapper)?,
        })
    }
}
