use kagari_hir::hir;

use crate::lower::IrLoweringError;
use crate::lower::state::{FunctionLowerer, LoopScope};
use crate::module::instruction::{
    BinaryOp, CallTarget, Constant, Instruction, IrValue, Terminator,
};
use crate::module::types::ValueType;
use kagari_hir::builtin::surface::StandardIntrinsic;

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
                let src = if self.cell_locals.contains(&local) {
                    let cell = self.alloc_temp(crate::module::ValueType::HeapObject);
                    self.emit(Instruction::MakeCell {
                        dst: cell,
                        value: src,
                    });
                    cell
                } else {
                    src
                };
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
            hir::StmtKind::For {
                pattern,
                iterable,
                body,
            } => self.lower_for(pattern, iterable, body),
            hir::StmtKind::Break => {
                let scope = self
                    .loops
                    .last()
                    .copied()
                    .ok_or(IrLoweringError::InvalidLoopControl)?;
                if let Some(dst) = scope.break_value {
                    let value = self.lower_unit();
                    self.emit(Instruction::Move { dst, src: value });
                }
                self.set_terminator(Terminator::Jump(scope.break_block));
                Ok(())
            }
            hir::StmtKind::BreakValue(expr) => {
                let scope = self
                    .loops
                    .last()
                    .copied()
                    .ok_or(IrLoweringError::InvalidLoopControl)?;
                let value = self.lower_expr(expr)?;
                if self.current_block_terminated() {
                    return Ok(());
                }
                let dst = scope
                    .break_value
                    .ok_or(IrLoweringError::InvalidLoopControl)?;
                self.emit(Instruction::Move { dst, src: value });
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
            break_value: None,
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
            break_value: None,
        });
        self.switch_to_block(body_block);
        let _ = self.lower_block(body)?;
        self.ensure_jump(body_block);
        self.loops.pop();

        self.switch_to_join(exit_block);
        Ok(())
    }

    fn lower_for(
        &mut self,
        pattern: hir::PatternId,
        iterable: hir::ExprId,
        body: hir::BlockId,
    ) -> Result<(), IrLoweringError> {
        let collection = self.lower_expr(iterable)?;
        if self.current_block_terminated() {
            return Ok(());
        }
        let iterable_ty = self
            .analyzed
            .typed
            .type_table
            .expr_type(iterable)
            .ok_or(IrLoweringError::MissingExprType(iterable))?;
        let item_ty = match kagari_hir::builtin::surface::iterable_protocol(&iterable_ty) {
            Some(kagari_hir::builtin::surface::IterableProtocol::Array { item })
            | Some(kagari_hir::builtin::surface::IterableProtocol::Set { item }) => item,
            Some(kagari_hir::builtin::surface::IterableProtocol::Map { key, value }) => {
                kagari_hir::types::TypeId::Tuple(vec![key, value])
            }
            Some(kagari_hir::builtin::surface::IterableProtocol::String { .. }) => {
                kagari_hir::types::TypeId::Builtin(kagari_hir::types::BuiltinType::String)
            }
            None => return Err(IrLoweringError::MissingBinding("checked for iterable")),
        };
        let guarded = collection.ty == ValueType::HeapObject;
        if guarded {
            self.emit(Instruction::BeginIteration { collection });
        }
        let items = self.alloc_temp(ValueType::HeapObject);
        self.emit(Instruction::Call {
            dst: Some(items),
            callee: CallTarget::StandardIntrinsic(StandardIntrinsic::IterToArray),
            args: smallvec::smallvec![collection],
        });
        let index = self.alloc_temp(ValueType::I64);
        let zero = self.lower_constant(Constant::I64(0), ValueType::I64);
        self.emit(Instruction::Move {
            dst: index,
            src: zero,
        });
        let cond_block = self.new_block();
        let element_block = self.new_block();
        let body_block = self.new_block();
        let step_block = self.new_block();
        let exit_block = self.new_block();
        self.ensure_jump(cond_block);
        self.switch_to_block(cond_block);
        let len = self.alloc_temp(ValueType::I64);
        self.emit(Instruction::Call {
            dst: Some(len),
            callee: CallTarget::StandardIntrinsic(StandardIntrinsic::ArrayLen),
            args: smallvec::smallvec![items],
        });
        let cond = self.alloc_temp(ValueType::Bool);
        self.emit(Instruction::Binary {
            dst: cond,
            op: BinaryOp::Lt,
            lhs: index,
            rhs: len,
        });
        self.set_terminator(Terminator::Branch {
            cond,
            then_block: element_block,
            else_block: exit_block,
        });
        self.switch_to_block(element_block);
        let item = self.alloc_temp(self.value_type(&item_ty)?);
        self.emit(Instruction::ReadAggregateIndex {
            dst: item,
            base: items,
            index,
        });
        let mut bindings = Vec::new();
        self.lower_pattern_decision(pattern, item, &item_ty, exit_block, &mut bindings)?;
        self.set_terminator(Terminator::Jump(body_block));
        self.loops.push(LoopScope {
            break_block: exit_block,
            continue_block: step_block,
            break_value: None,
        });
        self.switch_to_block(body_block);
        for (local, value) in bindings {
            self.emit(Instruction::StoreLocal { local, src: value });
            self.introduce_debug_local(local);
        }
        let _ = self.lower_block(body)?;
        self.ensure_jump(step_block);
        self.loops.pop();
        self.switch_to_block(step_block);
        let one = self.lower_constant(Constant::I64(1), ValueType::I64);
        let next = self.alloc_temp(ValueType::I64);
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
        self.set_terminator(Terminator::Jump(cond_block));
        self.switch_to_block(exit_block);
        if guarded {
            self.emit(Instruction::EndIteration);
        }
        Ok(())
    }
}
