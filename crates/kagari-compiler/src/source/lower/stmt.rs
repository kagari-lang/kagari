use crate::source::{
    lower::{
        MirLoweringError,
        state::{FunctionLowerer, LoopScope},
    },
    types::{self, lower_type},
};
use hir::{Condition, StmtKind};
use kagari_abi::{
    operations::{IterOp, StandardEnumOp},
    representation::ValueType,
    standard::{surface::StandardEnum, traits::StandardTrait},
};
use kagari_hir::{
    builtin::traits::StandardTraitSemantics, hir, typeck::ResolvedIteration, types::TypeId,
};
use std::slice;

use kagari_mir::instruction::{Instruction, MirValue, Terminator};

impl FunctionLowerer<'_, '_> {
    pub(crate) fn lower_block(
        &mut self,
        block_id: hir::BlockId,
    ) -> Result<Option<MirValue>, MirLoweringError> {
        let previous_scope = self.current_scope;
        let result = self.lower_block_inner(block_id);
        self.current_scope = previous_scope;
        result
    }

    fn lower_block_inner(
        &mut self,
        block_id: hir::BlockId,
    ) -> Result<Option<MirValue>, MirLoweringError> {
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

    fn lower_stmt(&mut self, stmt_id: hir::StmtId) -> Result<(), MirLoweringError> {
        self.planner.check()?;
        let span = self.analyzed.lowered.source_map.stmt_span(stmt_id);
        self.with_debug_span(span, |this| this.lower_stmt_inner(stmt_id))
    }

    fn lower_stmt_inner(&mut self, stmt_id: hir::StmtId) -> Result<(), MirLoweringError> {
        let stmt = self.analyzed.lowered.module.stmt(stmt_id).clone();
        match stmt.kind {
            StmtKind::Binding {
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
                    let cell = self.alloc_temp(ValueType::HeapObject);
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
            StmtKind::Assign { target, value, op } => {
                let Some(location) = self.prepare_place(target)? else {
                    return Ok(());
                };
                let src = self.lower_expr(value)?;
                if self.current_block_terminated() {
                    return Ok(());
                }
                let numeric = match (
                    op,
                    self.analyzed.typed.type_table.place_type(target),
                    self.analyzed.typed.type_table.expr_type(value),
                ) {
                    (Some(op), Some(TypeId::Builtin(input)), Some(TypeId::Builtin(rhs))) => {
                        types::lower_numeric_operation(op, input, rhs)
                    }
                    _ => None,
                };
                self.commit_place(location, op, numeric, src)?;
                Ok(())
            }
            StmtKind::Return { expr } => {
                let value = match expr {
                    Some(expr) => Some(self.lower_expr(expr)?),
                    None => Some(self.lower_unit()),
                };
                if !self.current_block_terminated() {
                    self.set_terminator(Terminator::Return(value));
                }
                Ok(())
            }
            StmtKind::Expr(expr) => {
                let _ = self.lower_expr(expr)?;
                Ok(())
            }
            StmtKind::While { condition, body } => self.lower_while(condition, body),
            StmtKind::Loop { body } => self.lower_loop(body),
            StmtKind::For {
                pattern,
                iterable,
                body,
            } => self.lower_for(pattern, iterable, body),
            StmtKind::Break => {
                let scope = self
                    .loops
                    .last()
                    .copied()
                    .ok_or(MirLoweringError::InvalidLoopControl)?;
                if let Some(dst) = scope.break_value {
                    let value = self.lower_unit();
                    self.emit(Instruction::Move { dst, src: value });
                }
                self.set_terminator(Terminator::Jump(scope.break_block));
                Ok(())
            }
            StmtKind::BreakValue(expr) => {
                let scope = self
                    .loops
                    .last()
                    .copied()
                    .ok_or(MirLoweringError::InvalidLoopControl)?;
                let value = self.lower_expr(expr)?;
                if self.current_block_terminated() {
                    return Ok(());
                }
                let dst = scope
                    .break_value
                    .ok_or(MirLoweringError::InvalidLoopControl)?;
                self.emit(Instruction::Move { dst, src: value });
                self.set_terminator(Terminator::Jump(scope.break_block));
                Ok(())
            }
            StmtKind::Continue => {
                let scope = self
                    .loops
                    .last()
                    .copied()
                    .ok_or(MirLoweringError::InvalidLoopControl)?;
                self.set_terminator(Terminator::Jump(scope.continue_block));
                Ok(())
            }
        }
    }

    fn lower_while(
        &mut self,
        condition: hir::Condition,
        body: hir::BlockId,
    ) -> Result<(), MirLoweringError> {
        let cond_block = self.new_block();

        self.ensure_jump(cond_block);

        self.switch_to_block(cond_block);
        let cond = self.lower_expr(condition.value())?;
        if self.current_block_terminated() {
            return Ok(());
        }
        let body_block = self.new_block();
        let exit_block = self.new_block();
        let mut bindings = Vec::new();
        match condition {
            Condition::Expr(_) => self.set_terminator(Terminator::Branch {
                cond,
                then_block: body_block,
                else_block: exit_block,
            }),
            Condition::Binding {
                pattern,
                initializer,
            } => {
                let ty = self
                    .analyzed
                    .typed
                    .type_table
                    .expr_type(initializer)
                    .ok_or(MirLoweringError::MissingExprType(initializer))?;
                self.lower_pattern_decision(pattern, cond, &ty, exit_block, &mut bindings)?;
                self.set_terminator(Terminator::Jump(body_block));
            }
        }

        self.loops.push(LoopScope {
            break_block: exit_block,
            continue_block: cond_block,
            break_value: None,
        });
        self.switch_to_block(body_block);
        let outer_scope = self.current_scope;
        for (local, value) in bindings {
            self.emit(Instruction::StoreLocal { local, src: value });
            self.introduce_debug_local(local);
        }
        let _ = self.lower_block(body)?;
        self.ensure_jump(cond_block);
        self.loops.pop();

        self.current_scope = outer_scope;
        self.switch_to_block(exit_block);
        Ok(())
    }

    fn lower_loop(&mut self, body: hir::BlockId) -> Result<(), MirLoweringError> {
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
    ) -> Result<(), MirLoweringError> {
        let fact = self
            .analyzed
            .typed
            .type_table
            .iteration(iterable)
            .cloned()
            .ok_or(MirLoweringError::MissingBinding("checked for protocol"))?;
        self.lower_protocol_for(pattern, iterable, body, fact)
    }
}

impl FunctionLowerer<'_, '_> {
    fn lower_protocol_for(
        &mut self,
        pattern: hir::PatternId,
        iterable: hir::ExprId,
        body: hir::BlockId,
        fact: ResolvedIteration,
    ) -> Result<(), MirLoweringError> {
        let receiver = self
            .analyzed
            .typed
            .type_table
            .expr_type(iterable)
            .ok_or(MirLoweringError::MissingExprType(iterable))?;
        let source = self.lower_expr(iterable)?;
        if self.current_block_terminated() {
            return Ok(());
        }
        let iterator = self.lower_applied_operator(
            fact.into_interface,
            receiver,
            &StandardTrait::Iterable.contract().methods[0].id,
            &[source],
        )?;
        let concrete_iterator = self
            .planner
            .arguments(
                slice::from_ref(&fact.iterator),
                &self.instance.substitution,
                self.function.debug.source_span,
            )?
            .remove(0);
        let iter_abi = if matches!(concrete_iterator, kagari_hir::types::TypeId::Iter(_)) {
            Some(lower_type(&concrete_iterator))
        } else {
            None
        };
        if iter_abi.is_some() {
            self.emit(Instruction::BeginIteration {
                collection: iterator,
            });
        }
        let next_block = self.new_block();
        let body_block = self.new_block();
        let exit = self.new_block();
        self.ensure_jump(next_block);
        self.switch_to_block(next_block);
        let value = self.lower_applied_operator(
            fact.next_interface,
            fact.iterator,
            &StandardTrait::Iterator.contract().methods[0].id,
            &[iterator],
        )?;
        let optional = TypeId::StandardEnum {
            kind: StandardEnum::Option,
            args: vec![fact.item.clone()],
        };
        let some = self.standard_enum_op(&optional, StandardEnumOp::Test(0), Some(value))?;
        self.set_terminator(Terminator::Branch {
            cond: some,
            then_block: body_block,
            else_block: exit,
        });
        self.switch_to_block(body_block);
        let item = self.standard_enum_op(&optional, StandardEnumOp::Read(0), Some(value))?;
        let mut bindings = Vec::new();
        self.lower_pattern_decision(pattern, item, &fact.item, exit, &mut bindings)?;
        for (local, value) in bindings {
            self.emit(Instruction::StoreLocal { local, src: value });
            self.introduce_debug_local(local);
        }
        self.loops.push(LoopScope {
            break_block: exit,
            continue_block: next_block,
            break_value: None,
        });
        let _ = self.lower_block(body)?;
        self.ensure_jump(next_block);
        self.loops.pop();
        self.switch_to_block(exit);
        if let Some(ty) = iter_abi {
            let dst = self.alloc_temp(ValueType::Unit);
            self.emit(Instruction::Iter {
                dst,
                value: Some(iterator),
                ty,
                op: IterOp::Close,
            });
            self.emit(Instruction::EndIteration);
        }
        Ok(())
    }
}
