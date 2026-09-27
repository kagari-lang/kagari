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
    pub(super) fn lower_array_from_fn(
        &mut self,
        site: hir::ExprId,
        count: IrValue,
        callback: IrValue,
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
        let TypeId::Array(item, _) = &ty else {
            return Err(IrLoweringError::MissingBinding("array factory result"));
        };
        let array = self.collection_new(&ty)?;
        let index = self.usize_constant(0);
        let head = self.new_block();
        let body = self.new_block();
        let done = self.new_block();
        self.ensure_jump(head);
        self.switch_to_block(head);
        let more = self.alloc_temp(ValueType::Bool);
        self.emit(Instruction::Binary {
            dst: more,
            op: BinaryOp::Lt,
            lhs: index,
            rhs: count,
        });
        self.set_terminator(Terminator::Branch {
            cond: more,
            then_block: body,
            else_block: done,
        });
        self.switch_to_block(body);
        let value = self.iterator_callback(callback, item, &[index])?;
        self.collection_insert(&ty, array, value)?;
        let one = self.usize_constant(1);
        let next = self.alloc_temp(ValueType::U64);
        self.emit(Instruction::Binary {
            dst: next,
            op: BinaryOp::Add,
            lhs: index,
            rhs: one,
        });
        self.emit(Instruction::Move {
            dst: index,
            src: next,
        });
        self.ensure_jump(head);
        self.switch_to_block(done);
        Ok(array)
    }
}
