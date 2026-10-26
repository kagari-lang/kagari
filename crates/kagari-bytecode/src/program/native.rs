//! Nonserialized evidence of native nominal occurrences in a verified program.
use crate::{instruction::NativeImportId, program::ModuleRef};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        mapping::{DefinitionMapper, DefinitionMappingError, check_cancel},
        scoped::{DefinitionReferences, RemapDefinitions},
        table::DefinitionId,
    },
};
use kagari_contract::{callable::generic::GenericBody, ids::FunctionRef};
use kagari_types::ty::NominalTy;

/// Location of a checked native application; this does not authorize a constructor call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeApplicationOccurrence {
    Import(NativeImportId),
    SharedCall {
        function: FunctionRef,
        offset: usize,
    },
}

/// An exact opaque application checked under its carrier's dependency and binder scope.
/// Only full bytecode verification issues these records. Consumers must retain the
/// immutable owner and compare supplying type scopes, not just nominal spellings.
/// Remapping is valid only together with that owner's identity-preserving import.
#[derive(Debug, Clone)]
pub struct NativeApplicationWitness {
    carrier: ModuleRef,
    occurrence: NativeApplicationOccurrence,
    nominal: NominalTy,
    binder: Option<GenericBody>,
}

impl NativeApplicationWitness {
    pub(crate) fn new(
        carrier: ModuleRef,
        occurrence: NativeApplicationOccurrence,
        nominal: NominalTy,
        binder: Option<GenericBody>,
    ) -> Self {
        Self {
            carrier,
            occurrence,
            nominal,
            binder,
        }
    }

    pub fn carrier(&self) -> ModuleRef {
        self.carrier
    }

    pub fn occurrence(&self) -> NativeApplicationOccurrence {
        self.occurrence
    }

    pub fn nominal(&self) -> &NominalTy {
        &self.nominal
    }

    /// Authored assumptions of the import body or shared-call carrier, never inferred bounds.
    pub fn binder(&self) -> Option<&GenericBody> {
        self.binder.as_ref()
    }

    /// Open carriers cannot provide host admission, even for a concrete-looking child.
    pub fn is_closed(&self) -> bool {
        self.binder.is_none() && self.nominal.arguments.iter().all(|ty| ty.is_concrete())
    }
}

impl RemapDefinitions for NativeApplicationWitness {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_>,
    ) -> Result<Self, DefinitionMappingError> {
        mapper.check()?;
        Ok(Self {
            carrier: self.carrier,
            occurrence: self.occurrence,
            nominal: self.nominal.remap_definitions(mapper)?,
            binder: self
                .binder
                .as_ref()
                .map(|body| body.remap_definitions(mapper))
                .transpose()?,
        })
    }
}

impl DefinitionReferences for NativeApplicationWitness {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        self.nominal.visit_definition_ids(visit, cancel)?;
        if let Some(body) = &self.binder {
            body.visit_definition_ids(visit, cancel)?;
        }
        Ok(())
    }
}
