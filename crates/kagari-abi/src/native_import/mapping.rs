//! Contextual identity traversal of the owning metadata records.
use crate::native_import::{NativeImport, NativeSignature};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        mapping::{
            DefinitionMapper, DefinitionMappingError, DefinitionRecord, check_cancel, map_sequence,
        },
        reference::DefinitionReference,
    },
};

impl<I: DefinitionReference> DefinitionRecord<I> for NativeSignature<I> {
    type Rebind<J: DefinitionReference> = NativeSignature<J>;
    fn map_identities<J: DefinitionReference>(
        &self,
        mapper: &mut DefinitionMapper<'_, I, J>,
    ) -> Result<Self::Rebind<J>, DefinitionMappingError> {
        mapper.check()?;
        Ok(NativeSignature {
            params: map_sequence(&self.params, |value| (value).map_identities(mapper))?,
            result: self.result.map_identities(mapper)?,
        })
    }
    fn visit_definitions(
        &self,
        visit: &mut impl FnMut(&I) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        for value0 in &self.params {
            (value0).visit_definitions(visit, cancel)?;
        }
        self.result.visit_definitions(visit, cancel)?;
        Ok(())
    }
}

impl<I: DefinitionReference> DefinitionRecord<I> for NativeImport<I> {
    type Rebind<J: DefinitionReference> = NativeImport<J>;
    fn map_identities<J: DefinitionReference>(
        &self,
        mapper: &mut DefinitionMapper<'_, I, J>,
    ) -> Result<Self::Rebind<J>, DefinitionMappingError> {
        mapper.check()?;
        Ok(NativeImport {
            result_adapter: self
                .result_adapter
                .as_ref()
                .map(|value| (value).map_identities(mapper))
                .transpose()?,
            generic: self
                .generic
                .as_ref()
                .map(|value| (value).map_identities(mapper))
                .transpose()?,
            instance: self.instance.map_identities(mapper)?,
            binding: mapper.reference(&self.binding)?,
            signature: self.signature.map_identities(mapper)?,
            requirements: map_sequence(&self.requirements, |value| (value).map_identities(mapper))?,
            callables: map_sequence(&self.callables, |value| (value).map_identities(mapper))?,
            host: self
                .host
                .as_ref()
                .map(|value| (value).map_identities(mapper))
                .transpose()?,
        })
    }
    fn visit_definitions(
        &self,
        visit: &mut impl FnMut(&I) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        if let Some(value0) = self.result_adapter.as_ref() {
            (value0).visit_definitions(visit, cancel)?;
        }
        if let Some(value0) = self.generic.as_ref() {
            (value0).visit_definitions(visit, cancel)?;
        }
        self.instance.visit_definitions(visit, cancel)?;
        check_cancel(cancel)?;
        visit(&self.binding)?;
        self.signature.visit_definitions(visit, cancel)?;
        for value0 in &self.requirements {
            (value0).visit_definitions(visit, cancel)?;
        }
        for value0 in &self.callables {
            (value0).visit_definitions(visit, cancel)?;
        }
        if let Some(value0) = self.host.as_ref() {
            (value0).visit_definitions(visit, cancel)?;
        }
        Ok(())
    }
}
