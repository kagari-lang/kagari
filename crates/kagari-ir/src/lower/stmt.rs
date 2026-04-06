use kagari_hir::hir;

use crate::lower::IrLoweringError;
use crate::lower::state::{FunctionLowerer, LoopScope};
use crate::module::ids::TempId;
use crate::module::instruction::{Instruction, Terminator};

impl FunctionLowerer<'_> {
    pub(crate) fn lower_block(
        &mut self,
        block_id: hir::BlockId,
    ) -> Result<Option<TempId>, IrLoweringError> {
        let block = self.analyzed.lowered.module.block(block_id).clone();
        for stmt in &block.statements {
            self.lower_stmt(*stmt)?;
        }

        if let Some(expr) = block.tail_expr {
            self.lower_expr(expr).map(Some)
        } else {
            Ok(None)
        }
    }

    fn lower_stmt(&mut self, stmt_id: hir::StmtId) -> Result<(), IrLoweringError> {
        let stmt = self.analyzed.lowered.module.stmt(stmt_id).clone();
        match stmt.kind {
            hir::StmtKind::Let {
                local,
                name,
                initializer,
                ..
            } => {
                let src = self.lower_expr(initializer)?;
                let dst = self.bind_local(local, name)?;
                self.emit(Instruction::StoreLocal { local: dst, src });
                Ok(())
            }
            hir::StmtKind::Assign { target, value } => {
                let src = self.lower_expr(value)?;
                let resolved = self
                    .analyzed
                    .names
                    .place_resolution(target)
                    .ok_or(IrLoweringError::UnresolvedPlace(target))?;
                let local = self.lookup_binding(resolved)?;
                self.emit(Instruction::StoreLocal { local, src });
                Ok(())
            }
            hir::StmtKind::Return { expr } => {
                let value = match expr {
                    Some(expr) => Some(self.lower_expr(expr)?),
                    None => Some(self.lower_unit()),
                };
                self.set_terminator(Terminator::Return(value));
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
        let body_block = self.new_block();
        let exit_block = self.new_block();

        self.ensure_jump(cond_block);

        self.switch_to_block(cond_block);
        let cond = self.lower_expr(condition)?;
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

        self.switch_to_block(exit_block);
        Ok(())
    }
}
