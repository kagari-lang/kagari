use kagari_hir::hir;

use crate::lower::IrLoweringError;
use crate::lower::state::{FunctionLowerer, LoopScope};
use crate::module::instruction::{Instruction, IrValue, Terminator};

impl FunctionLowerer<'_, '_> {
    pub(crate) fn lower_block(
        &mut self,
        block_id: hir::BlockId,
    ) -> Result<Option<IrValue>, IrLoweringError> {
        let previous_scope = self.current_scope;
        let result = self.lower_block_inner(block_id);
        self.current_scope = previous_scope;
        result
    }

    fn lower_block_inner(
        &mut self,
        block_id: hir::BlockId,
    ) -> Result<Option<IrValue>, IrLoweringError> {
        self.planner.check()?;
        let block = self.analyzed.lowered.module.block(block_id).clone();
        for stmt in &block.statements {
            if self.current_block_terminated() {
                return Ok(None);
            }
            self.lower_stmt(*stmt)?;
        }

        if self.current_block_terminated() {
            return Ok(None);
        }
        if let Some(expr) = block.tail_expr {
            let span = self.analyzed.lowered.source_map.expr_span(expr);
            self.with_debug_span(span, |this| this.lower_expr(expr).map(Some))
        } else {
            Ok(None)
        }
    }

    fn lower_stmt(&mut self, stmt_id: hir::StmtId) -> Result<(), IrLoweringError> {
        self.planner.check()?;
        let span = self.analyzed.lowered.source_map.stmt_span(stmt_id);
        self.with_debug_span(span, |this| this.lower_stmt_inner(stmt_id))
    }

    fn lower_stmt_inner(&mut self, stmt_id: hir::StmtId) -> Result<(), IrLoweringError> {
        let stmt = self.analyzed.lowered.module.stmt(stmt_id).clone();
        match stmt.kind {
            hir::StmtKind::Binding {
                local,
                name,
                initializer,
                ..
            } => {
                let src = self.lower_expr(initializer)?;
                if self.current_block_terminated() {
                    return Ok(());
                }
                let dst = self.bind_local(local, name)?;
                self.emit(Instruction::StoreLocal { local: dst, src });
                self.introduce_debug_local(dst);
                Ok(())
            }
            hir::StmtKind::Assign { target, value, op } => {
                let Some(location) = self.prepare_place(target)? else {
                    return Ok(());
                };
                let src = self.lower_expr(value)?;
                if self.current_block_terminated() {
                    return Ok(());
                }
                self.commit_place(location, op, src)?;
                Ok(())
            }
            hir::StmtKind::Return { expr } => {
                let value = match expr {
                    Some(expr) => Some(self.lower_expr(expr)?),
                    None => Some(self.lower_unit()),
                };
                if !self.current_block_terminated() {
                    self.set_terminator(Terminator::Return(value));
                }
                Ok(())
            }
            hir::StmtKind::Expr(expr) => {
                let _ = self.lower_expr(expr)?;
                Ok(())
            }
            hir::StmtKind::While { condition, body } => self.lower_while(condition, body),
            hir::StmtKind::Loop { body } => self.lower_loop(body),
            hir::StmtKind::Break => {
                let scope = self
                    .loops
                    .last()
                    .copied()
                    .ok_or(IrLoweringError::InvalidLoopControl)?;
                self.set_terminator(Terminator::Jump(scope.break_block));
                Ok(())
            }
            hir::StmtKind::Continue => {
                let scope = self
                    .loops
                    .last()
                    .copied()
                    .ok_or(IrLoweringError::InvalidLoopControl)?;
                self.set_terminator(Terminator::Jump(scope.continue_block));
                Ok(())
            }
        }
    }

    fn lower_while(
        &mut self,
        condition: hir::ExprId,
        body: hir::BlockId,
    ) -> Result<(), IrLoweringError> {
        let cond_block = self.new_block();

        self.ensure_jump(cond_block);

        self.switch_to_block(cond_block);
        let cond = self.lower_expr(condition)?;
        if self.current_block_terminated() {
            return Ok(());
        }
        let body_block = self.new_block();
        let exit_block = self.new_block();
        self.set_terminator(Terminator::Branch {
            cond,
            then_block: body_block,
            else_block: exit_block,
        });

        self.loops.push(LoopScope {
            break_block: exit_block,
            continue_block: cond_block,
        });
        self.switch_to_block(body_block);
        let _ = self.lower_block(body)?;
        self.ensure_jump(cond_block);
        self.loops.pop();

        self.switch_to_block(exit_block);
        Ok(())
    }

    fn lower_loop(&mut self, body: hir::BlockId) -> Result<(), IrLoweringError> {
        let body_block = self.new_block();
        let exit_block = self.new_block();

        self.ensure_jump(body_block);

        self.loops.push(LoopScope {
            break_block: exit_block,
            continue_block: body_block,
        });
        self.switch_to_block(body_block);
        let _ = self.lower_block(body)?;
        self.ensure_jump(body_block);
        self.loops.pop();

        self.switch_to_join(exit_block);
        Ok(())
    }
}
