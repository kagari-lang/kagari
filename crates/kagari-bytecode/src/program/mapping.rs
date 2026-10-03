//! Contextual identity traversal of the owning metadata records.
use crate::program::BytecodeProgram;
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        mapping::{
            DefinitionMapper, DefinitionMappingError, DefinitionRecord, check_cancel, map_sequence,
        },
        reference::DefinitionReference,
    },
};

impl<I: DefinitionReference> DefinitionRecord<I> for BytecodeProgram<I> {
    type Rebind<J: DefinitionReference> = BytecodeProgram<J>;

    fn map_identities<J: DefinitionReference>(
        &self,
        mapper: &mut DefinitionMapper<'_, I, J>,
    ) -> Result<Self::Rebind<J>, DefinitionMappingError> {
        mapper.check()?;
        Ok(BytecodeProgram {
            root: self.root,
            modules: map_sequence(&self.modules, |value| (value).map_identities(mapper))?,
        })
    }

    fn visit_definitions(
        &self,
        visit: &mut impl FnMut(&I) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        for value0 in &self.modules {
            (value0).visit_definitions(visit, cancel)?;
        }
        Ok(())
    }
}
