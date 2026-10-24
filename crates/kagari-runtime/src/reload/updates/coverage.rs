//! Owned projection of the backend's checked construction catalog, not a resolver.
use crate::reload::updates::proofs::ProofFacts;
use kagari_bytecode::module::BytecodeModule;
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        mapping::{DefinitionMapper, DefinitionMappingError, check_cancel},
        scoped::{DefinitionReferences, RemapDefinitions},
        table::{DefinitionId, DefinitionTable},
    },
};
use kagari_contract::{
    callable::construction::ConstructionMethod,
    native_import::NativeImport,
    types::ConcreteFunctionIdentity,
    updates::demands::{Coverage, NativeClosure},
};
use kagari_types::ty::Ty;
use std::fmt;
pub(crate) struct CoverageFacts {
    pub(super) methods: Vec<ConstructionMethod>,
    pub(super) constructions: Vec<ConcreteFunctionIdentity>,
    pub(super) layouts: Vec<Ty>,
    natives: Vec<NativeImport>,
    pub(super) proofs: ProofFacts,
}

impl fmt::Debug for CoverageFacts {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CoverageFacts")
            .field("methods", &self.methods.len())
            .field("constructions", &self.constructions)
            .field("layouts", &self.layouts)
            .field("natives", &self.natives)
            .field("proofs", &self.proofs)
            .finish()
    }
}

impl CoverageFacts {
    pub(crate) fn from_checked(
        coverage: Coverage<'_>,
        modules: &[BytecodeModule],
        definitions: &DefinitionTable,
    ) -> Self {
        Self {
            methods: coverage
                .methods
                .iter()
                .map(|method| ConstructionMethod {
                    interface: method.interface.clone(),
                    receiver: method.receiver.clone(),
                    slot: method.slot,
                    body: method.body.clone(),
                    parameters: method.parameters.clone(),
                    arguments: method.arguments.clone(),
                })
                .collect(),
            constructions: coverage.constructions.to_vec(),
            layouts: coverage.layouts.to_vec(),
            natives: modules
                .iter()
                .flat_map(|module| &module.native_imports)
                .cloned()
                .collect(),
            proofs: ProofFacts::from_checked(modules, definitions),
        }
    }

    pub(super) fn natives(&self) -> Vec<NativeClosure<'_>> {
        self.natives
            .iter()
            .map(|import| NativeClosure {
                import,
                // Existing effects and callback lists do not prove a closed native
                // boundary. Only future trusted registration evidence can do so.
                callbacks: None,
            })
            .collect()
    }
}

impl DefinitionReferences for CoverageFacts {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        for method in &self.methods {
            check_cancel(cancel)?;
            method.interface.visit_definition_ids(visit, cancel)?;
            method.receiver.visit_definition_ids(visit, cancel)?;
            method.body.visit_definition_ids(visit, cancel)?;
            method.parameters.visit_definition_ids(visit, cancel)?;
            method.arguments.visit_definition_ids(visit, cancel)?;
        }
        self.constructions.visit_definition_ids(visit, cancel)?;
        self.layouts.visit_definition_ids(visit, cancel)?;
        self.natives.visit_definition_ids(visit, cancel)?;
        self.proofs.visit_definition_ids(visit, cancel)
    }
}

impl RemapDefinitions for CoverageFacts {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_>,
    ) -> Result<Self, DefinitionMappingError> {
        let methods = self
            .methods
            .iter()
            .map(|method| {
                mapper.check()?;
                Ok(ConstructionMethod {
                    interface: method.interface.remap_definitions(mapper)?,
                    receiver: method.receiver.remap_definitions(mapper)?,
                    slot: method.slot,
                    body: method.body.remap_definitions(mapper)?,
                    parameters: method.parameters.remap_definitions(mapper)?,
                    arguments: method.arguments.remap_definitions(mapper)?,
                })
            })
            .collect::<Result<_, DefinitionMappingError>>()?;
        Ok(Self {
            methods,
            constructions: self.constructions.remap_definitions(mapper)?,
            layouts: self.layouts.remap_definitions(mapper)?,
            natives: self.natives.remap_definitions(mapper)?,
            proofs: self.proofs.remap_definitions(mapper)?,
        })
    }
}
