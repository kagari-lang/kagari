//! Checked protocol calls and ordinary variant identities selected for one `?`.
use crate::{
    hir::ids::ExprId,
    typeck::table::{ResolvedCall, TypeTable},
    types::TypeId,
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        DefinitionPath,
        mapping::{DefinitionMapper, DefinitionMappingError, DefinitionRecord},
        reference::DefinitionReference,
    },
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedPropagation<I: DefinitionReference = DefinitionPath> {
    pub branch: ResolvedCall<I>,
    pub from_residual: ResolvedCall<I>,
    pub return_type: TypeId<I>,
    pub break_variant: I,
    pub continue_variant: I,
}

impl TypeTable {
    pub(crate) fn insert_propagation(&mut self, id: ExprId, fact: ResolvedPropagation) {
        self.propagations.insert(id, fact);
    }
}

impl<I: DefinitionReference> TypeTable<I> {
    pub fn propagation(&self, id: ExprId) -> Option<&ResolvedPropagation<I>> {
        self.propagations.get(&id)
    }
}

impl<I: DefinitionReference> DefinitionRecord<I> for ResolvedPropagation<I> {
    type Rebind<J: DefinitionReference> = ResolvedPropagation<J>;

    fn map_identities<J: DefinitionReference>(
        &self,
        mapper: &mut DefinitionMapper<'_, I, J>,
    ) -> Result<Self::Rebind<J>, DefinitionMappingError> {
        Ok(ResolvedPropagation {
            branch: self.branch.map_identities(mapper)?,
            from_residual: self.from_residual.map_identities(mapper)?,
            return_type: self.return_type.map_identities(mapper)?,
            break_variant: mapper.reference(&self.break_variant)?,
            continue_variant: mapper.reference(&self.continue_variant)?,
        })
    }

    fn visit_definitions(
        &self,
        visit: &mut impl FnMut(&I) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        self.branch.visit_definitions(visit, cancel)?;
        self.from_residual.visit_definitions(visit, cancel)?;
        self.return_type.visit_definitions(visit, cancel)?;
        visit(&self.break_variant)?;
        visit(&self.continue_variant)
    }
}
