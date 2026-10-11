//! Normalize offline Host contracts into the analysis scope without changing revisions or slots.

use crate::host::{HostDeclarations, callable::HostSignature};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        map::DefinitionContext,
        mapping::{DefinitionMapper, DefinitionMappingError},
        scoped::{DefinitionReferences, RemapDefinitions},
        table::{DefinitionId, DefinitionTable},
    },
};
use std::{collections::HashSet, sync::Arc};

impl HostDeclarations {
    /// Borrows the scope of these immutable offline contracts and semantic signatures.
    pub fn definitions(&self) -> &DefinitionTable {
        &self.definitions
    }

    /// Imports the complete interface once, then remaps its indexes and origin keys.
    /// Host revision/slot identities remain tied to the original immutable input.
    pub(crate) fn adopt(
        &self,
        context: &DefinitionContext,
        cancel: &CancellationToken,
    ) -> Result<Arc<Self>, DefinitionMappingError> {
        let mut references = HashSet::new();
        self.interface.visit_definition_ids(
            &mut |id| {
                references.insert(*id);
                Ok(())
            },
            cancel,
        )?;
        let remap = context.import(&self.definitions, references)?;
        let mut map = |id: &_| remap.map(*id).map_err(Into::into);
        let mut mapper = DefinitionMapper::new(&mut map, cancel);
        let interface = self.interface.remap_definitions(&mut mapper)?;
        let origins = self
            .origins
            .iter()
            .map(|(id, origin)| Ok((mapper.reference(id)?, origin.clone())))
            .collect::<Result<_, DefinitionMappingError>>()?;
        let methods = self
            .methods
            .iter()
            .map(|((owner, name), slot)| Ok(((mapper.reference(owner)?, name.clone()), *slot)))
            .collect::<Result<_, DefinitionMappingError>>()?;
        let type_identities = self
            .type_identities
            .iter()
            .map(|(id, slot)| Ok((mapper.reference(id)?, *slot)))
            .collect::<Result<_, DefinitionMappingError>>()?;
        cancel
            .check()
            .map_err(|_| DefinitionMappingError::Cancelled)?;
        Ok(Arc::new(Self {
            revision: self.revision,
            signatures: interface.functions.iter().map(HostSignature::new).collect(),
            interface,
            definitions: context.snapshot(),
            origins,
            methods,
            type_identities,
            paths: self.paths.clone(),
            type_paths: self.type_paths.clone(),
            modules: self.modules.clone(),
        }))
    }
}

impl DefinitionReferences for HostDeclarations {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        cancel
            .check()
            .map_err(|_| DefinitionMappingError::Cancelled)?;
        self.interface.visit_definition_ids(visit, cancel)?;
        for signature in &self.signatures {
            signature.visit_definition_ids(visit, cancel)?;
        }
        for id in self
            .origins
            .keys()
            .chain(self.type_identities.keys())
            .chain(self.methods.keys().map(|(id, _)| id))
        {
            cancel
                .check()
                .map_err(|_| DefinitionMappingError::Cancelled)?;
            visit(id)?;
        }
        Ok(())
    }
}
