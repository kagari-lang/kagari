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

/// Checked branch/residual calls and ordinary enum variants selected for one postfix `?`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedPropagation<I: DefinitionReference = DefinitionPath> {
    /// Selected Try branch operation on the operand.
    pub branch: ResolvedCall<I>,
    /// Selected conversion used on the early-return residual path.
    pub from_residual: ResolvedCall<I>,
    /// Enclosing return type expected by residual conversion.
    pub return_type: TypeId<I>,
    /// Canonical variant selecting the residual/early-return branch.
    pub break_variant: I,
    /// Canonical variant carrying the successful output.
    pub continue_variant: I,
}

impl TypeTable {
    pub(crate) fn insert_propagation(&mut self, id: ExprId, fact: ResolvedPropagation) {
        self.propagations.insert(id, fact);
    }
}

impl<I: DefinitionReference> TypeTable<I> {
    /// Borrows the recorded propagation contract, or `None` when selection did not publish one.
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
