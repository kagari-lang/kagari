use super::*;
use kagari_hir::types::TypeId;

impl FunctionLowerer<'_, '_> {
    pub(super) fn lower_collection_factory(
        &mut self,
        site: hir::ExprId,
        input: IrValue,
    ) -> Result<IrValue, IrLoweringError> {
        let ty = self
            .analyzed
            .typed
            .type_table
            .expr_type(site)
            .ok_or(IrLoweringError::MissingExprType(site))?;
        let span = self.analyzed.lowered.source_map.expr_span(site);
        let ty = self
            .planner
            .arguments(&[ty], &self.instance.substitution, span)?
            .remove(0);
        let item = kagari_hir::builtin::traits::collection_item(&ty)
            .ok_or(IrLoweringError::MissingBinding("collection factory result"))?;
        let source = TypeId::Array(
            Box::new(item),
            kagari_common::collection::CollectionAccess::ReadOnly,
        );
        self.lower_collect(&ty, &source, input)
    }
}
