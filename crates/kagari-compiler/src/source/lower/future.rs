//! Async callables lower to an ordinary cold factory and a private resume body.
use crate::source::lower::{MirLoweringError, state::FunctionLowerer};
use kagari_abi::representation::ValueType;
use kagari_hir::hir::ids::ExprId;
use kagari_mir::instruction::{Instruction, MirValue, Terminator, ValueBuffer};

impl FunctionLowerer<'_, '_> {
    pub(super) fn mark_resume(&mut self) {
        if self.instance.resume {
            self.effects.may_suspend = true;
            self.function.name.push_str("$resume");
        }
    }

    pub(super) fn lower_future_factory(&mut self) -> Result<(), MirLoweringError> {
        let function = self
            .planner
            .enqueue_resume(&self.instance, self.function.debug.source_span)?;
        let mut arguments = ValueBuffer::new();
        for parameter in self.function.params.clone() {
            let value = self.alloc_temp(parameter.ty);
            let ty = self
                .function
                .semantic
                .locals
                .get(&parameter.local.index())
                .cloned()
                .ok_or(MirLoweringError::MissingBinding("Future parameter type"))?;
            self.function
                .semantic
                .registers
                .insert(value.temp.index(), ty);
            self.emit(Instruction::LoadLocal {
                dst: value,
                local: parameter.local,
            });
            arguments.push(value);
        }
        let future = self
            .function
            .semantic
            .result
            .clone()
            .ok_or(MirLoweringError::MissingBinding("Future factory result"))?;
        let dst = self.alloc_temp(ValueType::HeapObject);
        self.function
            .semantic
            .registers
            .insert(dst.temp.index(), future.clone());
        self.emit(Instruction::MakeFuture {
            dst,
            function,
            arguments,
            future,
        });
        self.set_terminator(Terminator::Return(Some(dst)));
        Ok(())
    }

    pub(super) fn lower_await(
        &mut self,
        site: ExprId,
        operand: ExprId,
    ) -> Result<MirValue, MirLoweringError> {
        if !self.instance.resume {
            return Err(MirLoweringError::MissingBinding("await resume body"));
        }
        let value = self.lower_expr(operand)?;
        if self.current_block_terminated() {
            return Ok(value);
        }
        let ty = self
            .analyzed
            .typed
            .type_table
            .expr_type(operand)
            .ok_or(MirLoweringError::MissingExprType(operand))?;
        let future = self.semantic_type(&ty)?;
        let dst = self.alloc_temp(self.expr_type(site)?);
        self.emit(Instruction::Await { dst, value, future });
        Ok(dst)
    }
}
