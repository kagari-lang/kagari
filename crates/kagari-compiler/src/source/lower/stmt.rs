use crate::source::{
    lower::{
        MirLoweringError,
        state::{FunctionLowerer, LoopScope},
    },
    types,
};

use kagari_hir::{
    hir::{
        expr::Condition,
        ids::{BlockId, ExprId, PatternId, StmtId},
        stmt::StmtKind,
    },
    typeck::table::ResolvedIteration,
    types::NominalType,
    types::TypeId,
};
use kagari_mir::instruction::{Instruction, MirValue, Terminator};
use kagari_types::language::{Protocol, binding};

impl FunctionLowerer<'_, '_> {
    pub(crate) fn lower_block(
        &mut self,
        block_id: BlockId,
    ) -> Result<Option<MirValue>, MirLoweringError> {
        let previous_scope = self.current_scope;
        let result = self.lower_block_inner(block_id);
        self.current_scope = previous_scope;
        result
    }

    fn lower_block_inner(
        &mut self,
        block_id: BlockId,
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

    fn lower_stmt(&mut self, stmt_id: StmtId) -> Result<(), MirLoweringError> {
        self.planner.check()?;
        let span = self.analyzed.lowered.source_map.stmt_span(stmt_id);
        self.with_debug_span(span, |this| this.lower_stmt_inner(stmt_id))
    }

    fn lower_stmt_inner(&mut self, stmt_id: StmtId) -> Result<(), MirLoweringError> {
        let stmt = self.analyzed.lowered.module.stmt(stmt_id).clone();
        match stmt.kind {
            StmtKind::Function(_) => Err(MirLoweringError::MissingBinding(
                "local function execution pending LF04",
            )),
            StmtKind::Binding {
                pattern,
                initializer,
                ..
            } => {
                let src = self.lower_expr(initializer)?;
                if self.current_block_terminated() {
                    return Ok(());
                }
                let ty = self
                    .analyzed
                    .typed
                    .type_table
                    .coerced_expr_type(&self.planner.definitions(), initializer)
                    .ok_or(MirLoweringError::MissingExprType(initializer))?;
                let mut bindings = Vec::new();
                self.lower_pattern_decision(pattern, src, &ty, None, &mut bindings)?;
                self.publish_declaration_bindings(pattern, bindings)
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

    fn lower_while(&mut self, condition: Condition, body: BlockId) -> Result<(), MirLoweringError> {
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
                self.lower_pattern_decision(pattern, cond, &ty, Some(exit_block), &mut bindings)?;
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
        self.publish_pattern_bindings(bindings)?;
        let _ = self.lower_block(body)?;
        self.ensure_jump(cond_block);
        self.loops.pop();

        self.current_scope = outer_scope;
        self.switch_to_block(exit_block);
        Ok(())
    }

    fn lower_loop(&mut self, body: BlockId) -> Result<(), MirLoweringError> {
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
        pattern: PatternId,
        iterable: ExprId,
        body: BlockId,
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
        pattern: PatternId,
        iterable: ExprId,
        body: BlockId,
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
            &self.protocol_method(Protocol::Iterable, 0)?,
            &[source],
        )?;
        let next_block = self.new_block();
        let body_block = self.new_block();
        let exit = self.new_block();
        self.ensure_jump(next_block);
        self.switch_to_block(next_block);
        let value = self.lower_applied_operator(
            fact.next_interface,
            fact.iterator,
            &self.protocol_method(Protocol::Iterator, 0)?,
            &[iterator],
        )?;
        let optional = TypeId::Enum(NominalType {
            declaration: self
                .planner
                .definitions()
                .lookup(&binding::option_declaration())
                .ok_or(MirLoweringError::MissingBinding("Option declaration"))?,
            arguments: vec![fact.item.clone()],
            associated_types: Default::default(),
        });
        let some = self.test_enum_variant(&optional, value, 0)?;
        self.set_terminator(Terminator::Branch {
            cond: some,
            then_block: body_block,
            else_block: exit,
        });
        self.switch_to_block(body_block);
        let item = self.read_enum_field(&optional, value, 0, 0)?;
        let mut bindings = Vec::new();
        self.lower_pattern_decision(pattern, item, &fact.item, Some(exit), &mut bindings)?;
        self.publish_pattern_bindings(bindings)?;
        self.loops.push(LoopScope {
            break_block: exit,
            continue_block: next_block,
            break_value: None,
        });
        let _ = self.lower_block(body)?;
        self.ensure_jump(next_block);
        self.loops.pop();
        self.switch_to_block(exit);
        Ok(())
    }
}
