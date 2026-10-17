//! Structural protocol operations supplied through ordinary shared call contracts.
use crate::{
    callable::{generic::GenericBody, shared::SharedCall},
    types::proofs::ProofCatalog,
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        DefinitionKind,
        mapping::{DefinitionMapper, DefinitionMappingError},
        scoped::{DefinitionReferences, RemapDefinitions},
        table::{DefinitionId, DefinitionTable},
    },
};
use kagari_types::{
    callable::{CallableImplementation, Signature},
    declaration::requirement::NativeCallableRequirement,
    language::{Protocol, adapter::adapter_contract},
    ty::{GenericBound, GenericParam, Ty, substitution::TypeTransformError},
};

/// A tuple operation composed from independently checked element operations.
/// The shared body owns traversal; the call retains each argument's supplying scope.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SharedAdapterWitness {
    pub requirement: NativeCallableRequirement,
    pub call: SharedCall,
}

impl SharedAdapterWitness {
    /// Recover the exact generic adapter contract, without selecting implementations.
    pub fn template(
        &self,
        definitions: &DefinitionTable,
    ) -> Option<(GenericBody, NativeCallableRequirement, Signature)> {
        let Ty::Tuple(items) = &self.requirement.receiver else {
            return None;
        };
        let (kind, signature) = adapter_contract(&self.requirement, definitions)?;
        if items.is_empty()
            || items.len() > 4096
            || !matches!(kind, Protocol::PartialEq | Protocol::Hash)
            || self.call.implementation != CallableImplementation::Script
            || self.call.arguments != *items
            || self.call.signature != signature
            || !self.call.instance.arguments.is_empty()
            || !definitions
                .resolve(self.call.instance.declaration)
                .is_ok_and(|view| {
                    view.segments().count() == 1
                        && view.last().is_some_and(|part| {
                            part.kind == DefinitionKind::Function
                                && part.name == format!("$derived_tuple_{}", kind.name())
                                && part.occurrence as usize == items.len()
                        })
                })
        {
            return None;
        }
        let parameters: Vec<_> = (0..items.len())
            .map(|position| GenericParam {
                owner: self.call.instance.declaration,
                position,
            })
            .collect();
        let bounds = parameters
            .iter()
            .map(|parameter| GenericBound {
                ty: parameter.as_type(),
                constraints: vec![self.requirement.interface.clone()],
            })
            .collect();
        let required = NativeCallableRequirement {
            receiver: Ty::Tuple(parameters.iter().map(GenericParam::as_type).collect()),
            ..self.requirement.clone()
        };
        let (_, signature) = adapter_contract(&required, definitions)?;
        Some((GenericBody { parameters, bounds }, required, signature))
    }

    pub fn valid(
        &self,
        catalog: &ProofCatalog<'_>,
        parameters: &[GenericParam],
        assumptions: &[GenericBound],
        cancel: &CancellationToken,
    ) -> Result<bool, TypeTransformError> {
        let Some((body, required, signature)) = self.template(catalog.definitions()) else {
            return Ok(false);
        };
        Ok(catalog
            .implicit_callable_signature(&required, &body.bounds, cancel)?
            .as_ref()
            == Some(&signature)
            && self
                .call
                .check(&body, &signature, catalog, parameters, assumptions, cancel)?)
    }
}

impl RemapDefinitions for SharedAdapterWitness {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_>,
    ) -> Result<Self, DefinitionMappingError> {
        mapper.check()?;
        Ok(Self {
            requirement: self.requirement.remap_definitions(mapper)?,
            call: self.call.remap_definitions(mapper)?,
        })
    }
}

impl DefinitionReferences for SharedAdapterWitness {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        self.requirement.visit_definition_ids(visit, cancel)?;
        self.call.visit_definition_ids(visit, cancel)
    }
}
