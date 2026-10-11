//! Checked protocol calls and ordinary variant identities selected for one `?`.
use crate::{
    hir::ids::ExprId,
    typeck::table::{ResolvedCall, TypeTable},
    types::TypeId,
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        mapping::{DefinitionMapper, DefinitionMappingError},
        scoped::{DefinitionReferences, RemapDefinitions},
        table::DefinitionId,
    },
};

/// Checked branch/residual calls and ordinary enum variants selected for one postfix `?`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedPropagation {
    /// Selected Try branch operation on the operand.
    pub branch: ResolvedCall,
    /// Selected conversion used on the early-return residual path.
    pub from_residual: ResolvedCall,
    /// Enclosing return type expected by residual conversion.
    pub return_type: TypeId,
    /// Canonical variant selecting the residual/early-return branch.
    pub break_variant: DefinitionId,
    /// Canonical variant carrying the successful output.
    pub continue_variant: DefinitionId,
}

impl TypeTable {
    pub(crate) fn insert_propagation(&mut self, id: ExprId, fact: ResolvedPropagation) {
        self.propagations.insert(id, fact);
    }
}

impl TypeTable {
    /// Borrows the recorded propagation contract, or `None` when selection did not publish one.
    pub fn propagation(&self, id: ExprId) -> Option<&ResolvedPropagation> {
        self.propagations.get(&id)
    }
}

impl RemapDefinitions for ResolvedPropagation {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_>,
    ) -> Result<Self, DefinitionMappingError> {
        Ok(ResolvedPropagation {
            branch: self.branch.remap_definitions(mapper)?,
            from_residual: self.from_residual.remap_definitions(mapper)?,
            return_type: self.return_type.remap_definitions(mapper)?,
            break_variant: mapper.reference(&self.break_variant)?,
            continue_variant: mapper.reference(&self.continue_variant)?,
        })
    }
}

impl DefinitionReferences for ResolvedPropagation {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        self.branch.visit_definition_ids(visit, cancel)?;
        self.from_residual.visit_definition_ids(visit, cancel)?;
        self.return_type.visit_definition_ids(visit, cancel)?;
        visit(&self.break_variant)?;
        visit(&self.continue_variant)
    }
}
