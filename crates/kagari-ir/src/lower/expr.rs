use kagari_hir::hir;

use crate::lower::IrLoweringError;
use crate::lower::state::FunctionLowerer;
use crate::module::ids::TempId;
use crate::module::instruction::{CallTarget, Constant, Instruction, StructFieldInit, Terminator};
use crate::module::types::ValueType;

impl FunctionLowerer<'_> {
    pub(crate) fn lower_expr(&mut self, expr_id: hir::ExprId) -> Result<TempId, IrLoweringError> {
        let expr = self.analyzed.lowered.module.expr(expr_id).clone();
        match expr.kind {
            hir::ExprKind::Name(_) => self.lower_name_expr(expr_id),
            hir::ExprKind::Literal(literal) => match literal.kind {
                hir::LiteralKind::Number => {
                    let value = literal.text.parse::<i32>().unwrap_or_default();
                    Ok(self.lower_constant(Constant::I32(value), ValueType::I32))
                }
                hir::LiteralKind::Float => {
                    let value = literal.text.parse::<f32>().unwrap_or_default();
                    Ok(self.lower_constant(Constant::F32(value), ValueType::F32))
                }
                hir::LiteralKind::String => {
                    Ok(self.lower_constant(Constant::Str(literal.text), ValueType::Str))
                }
                hir::LiteralKind::Bool => {
                    let value = literal.text == "true";
                    Ok(self.lower_constant(Constant::Bool(value), ValueType::Bool))
                }
            },
            hir::ExprKind::Prefix { op, expr } => {
                let operand = self.lower_expr(expr)?;
                let dst = self.alloc_temp(self.expr_type(expr_id)?);
                self.emit(Instruction::Unary {
                    dst,
                    op: FunctionLowerer::lower_unary_op(op),
                    operand,
                });
                Ok(dst)
            }
            hir::ExprKind::Binary { lhs, op, rhs } => {
                if matches!(op, hir::BinaryOp::AndAnd | hir::BinaryOp::OrOr) {
                    return self.lower_short_circuit(expr_id, lhs, op, rhs);
                }
                let lhs = self.lower_expr(lhs)?;
                let rhs = self.lower_expr(rhs)?;
                let dst = self.alloc_temp(self.expr_type(expr_id)?);
                self.emit(Instruction::Binary {
                    dst,
                    op: FunctionLowerer::lower_binary_op(op),
                    lhs,
                    rhs,
                });
                Ok(dst)
            }
            hir::ExprKind::Call { callee, args } => {
                let args = args
                    .iter()
                    .map(|arg| self.lower_expr(*arg))
                    .collect::<Result<_, _>>()?;
                let callee = match self.lower_direct_callee(callee)? {
                    Some(callee) => callee,
                    None => CallTarget::Temp(self.lower_expr(callee)?),
                };
                let dst = self.alloc_temp(self.expr_type(expr_id)?);
                self.emit(Instruction::Call {
                    dst: Some(dst),
                    callee,
                    args,
                });
                Ok(dst)
            }
            hir::ExprKind::Block(block) => {
                if let Some(temp) = self.lower_block(block)? {
                    Ok(temp)
                } else {
                    Ok(self.lower_unit())
                }
            }
            hir::ExprKind::If {
                condition,
                then_branch,
                else_branch,
            } => self.lower_if(expr_id, condition, then_branch, else_branch),
            hir::ExprKind::Field { receiver, name } => self.lower_field(expr_id, receiver, name),
            hir::ExprKind::Index { receiver, index } => self.lower_index(expr_id, receiver, index),
            hir::ExprKind::Match { scrutinee, arms } => self.lower_match(expr_id, scrutinee, arms),
            hir::ExprKind::StructInit { path, fields } => {
                self.lower_struct_init(expr_id, path, fields)
            }
            hir::ExprKind::Tuple(elements) => self.lower_tuple(expr_id, elements),
            hir::ExprKind::Array(elements) => self.lower_array(expr_id, elements),
        }
    }

    fn lower_if(
        &mut self,
        expr_id: hir::ExprId,
        condition: hir::ExprId,
        then_branch: hir::BlockId,
        else_branch: Option<hir::ExprId>,
    ) -> Result<TempId, IrLoweringError> {
        let cond = self.lower_expr(condition)?;
        let then_block = self.new_block();
        let else_block = self.new_block();
        let join_block = self.new_block();
        let result = self.alloc_temp(self.expr_type(expr_id)?);

        self.set_terminator(Terminator::Branch {
            cond,
            then_block,
            else_block,
        });

        self.switch_to_block(then_block);
        let then_value = self
            .lower_block(then_branch)?
            .unwrap_or_else(|| self.lower_unit());
        if !self.current_block_terminated() {
            self.emit(Instruction::Move {
                dst: result,
                src: then_value,
            });
            self.set_terminator(Terminator::Jump(join_block));
        }

        self.switch_to_block(else_block);
        let else_value = match else_branch {
            Some(expr) => self.lower_expr(expr)?,
            None => self.lower_unit(),
        };
        if !self.current_block_terminated() {
            self.emit(Instruction::Move {
                dst: result,
                src: else_value,
            });
            self.set_terminator(Terminator::Jump(join_block));
        }

        self.switch_to_block(join_block);
        Ok(result)
    }

    fn lower_short_circuit(
        &mut self,
        expr_id: hir::ExprId,
        lhs: hir::ExprId,
        op: hir::BinaryOp,
        rhs: hir::ExprId,
    ) -> Result<TempId, IrLoweringError> {
        let lhs = self.lower_expr(lhs)?;
        let rhs_block = self.new_block();
        let short_block = self.new_block();
        let join_block = self.new_block();
        let result = self.alloc_temp(self.expr_type(expr_id)?);

        match op {
            hir::BinaryOp::AndAnd => {
                self.set_terminator(Terminator::Branch {
                    cond: lhs,
                    then_block: rhs_block,
                    else_block: short_block,
                });

                self.switch_to_block(short_block);
                let short_value = self.lower_constant(Constant::Bool(false), ValueType::Bool);
                self.emit(Instruction::Move {
                    dst: result,
                    src: short_value,
                });
                self.set_terminator(Terminator::Jump(join_block));
            }
            hir::BinaryOp::OrOr => {
                self.set_terminator(Terminator::Branch {
                    cond: lhs,
                    then_block: short_block,
                    else_block: rhs_block,
                });

                self.switch_to_block(short_block);
                let short_value = self.lower_constant(Constant::Bool(true), ValueType::Bool);
                self.emit(Instruction::Move {
                    dst: result,
                    src: short_value,
                });
                self.set_terminator(Terminator::Jump(join_block));
            }
            _ => unreachable!("short-circuit lowering called for non-short-circuit op"),
        }

        self.switch_to_block(rhs_block);
        let rhs = self.lower_expr(rhs)?;
        if !self.current_block_terminated() {
            self.emit(Instruction::Move {
                dst: result,
                src: rhs,
            });
            self.set_terminator(Terminator::Jump(join_block));
        }

        self.switch_to_block(join_block);
        Ok(result)
    }

    fn lower_match(
        &mut self,
        expr_id: hir::ExprId,
        scrutinee: hir::ExprId,
        arms: hir::MatchArmBuffer,
    ) -> Result<TempId, IrLoweringError> {
        let scrutinee_temp = self.lower_expr(scrutinee)?;
        let result = self.alloc_temp(self.expr_type(expr_id)?);
        let exit_block = self.new_block();
        let fail_block = self.new_block();
        let mut decision_block = self.current_block;

        for arm in arms {
            let arm_block = self.new_block();
            let next_decision = self.new_block();

            self.switch_to_block(decision_block);
            match &self.analyzed.lowered.module.pattern(arm.pattern).kind {
                hir::PatternKind::Wildcard => {
                    self.set_terminator(Terminator::Jump(arm_block));
                }
                hir::PatternKind::Literal(literal) => {
                    let literal_temp = self.lower_literal_value(literal);
                    let cond = self.alloc_temp(crate::module::types::ValueType::Bool);
                    self.emit(Instruction::Binary {
                        dst: cond,
                        op: crate::module::instruction::BinaryOp::Eq,
                        lhs: scrutinee_temp,
                        rhs: literal_temp,
                    });
                    self.set_terminator(Terminator::Branch {
                        cond,
                        then_block: arm_block,
                        else_block: next_decision,
                    });
                }
                hir::PatternKind::Name { local, name } => {
                    let local_ty = self
                        .analyzed
                        .typed
                        .type_table
                        .local_type(*local)
                        .map(ValueType::from_type_id)
                        .ok_or(IrLoweringError::MissingLocalType(*local))?;
                    let ir_local = self.alloc_local(name.clone(), local_ty);
                    self.locals.insert(*local, ir_local);
                    self.set_terminator(Terminator::Jump(arm_block));

                    self.switch_to_block(arm_block);
                    self.emit(Instruction::StoreLocal {
                        local: ir_local,
                        src: scrutinee_temp,
                    });
                    let arm_value = self.lower_expr(arm.expr)?;
                    if !self.current_block_terminated() {
                        self.emit(Instruction::Move {
                            dst: result,
                            src: arm_value,
                        });
                        self.set_terminator(Terminator::Jump(exit_block));
                    }

                    decision_block = next_decision;
                    continue;
                }
            }

            self.switch_to_block(arm_block);
            let arm_value = self.lower_expr(arm.expr)?;
            if !self.current_block_terminated() {
                self.emit(Instruction::Move {
                    dst: result,
                    src: arm_value,
                });
                self.set_terminator(Terminator::Jump(exit_block));
            }

            decision_block = next_decision;
        }

        self.switch_to_block(decision_block);
        self.set_terminator(Terminator::Jump(fail_block));

        self.switch_to_block(fail_block);
        self.set_terminator(Terminator::Unreachable);

        self.switch_to_block(exit_block);
        Ok(result)
    }

    fn lower_literal_value(&mut self, literal: &hir::Literal) -> TempId {
        match literal.kind {
            hir::LiteralKind::Number => {
                let value = literal.text.parse::<i32>().unwrap_or_default();
                self.lower_constant(Constant::I32(value), ValueType::I32)
            }
            hir::LiteralKind::Float => {
                let value = literal.text.parse::<f32>().unwrap_or_default();
                self.lower_constant(Constant::F32(value), ValueType::F32)
            }
            hir::LiteralKind::String => {
                self.lower_constant(Constant::Str(literal.text.clone()), ValueType::Str)
            }
            hir::LiteralKind::Bool => {
                let value = literal.text == "true";
                self.lower_constant(Constant::Bool(value), ValueType::Bool)
            }
        }
    }

    fn lower_tuple(
        &mut self,
        expr_id: hir::ExprId,
        elements: hir::ExprBuffer,
    ) -> Result<TempId, IrLoweringError> {
        let elements = elements
            .iter()
            .map(|expr| self.lower_expr(*expr))
            .collect::<Result<_, _>>()?;
        let dst = self.alloc_temp(self.expr_type(expr_id)?);
        self.emit(Instruction::MakeTuple { dst, elements });
        Ok(dst)
    }

    fn lower_array(
        &mut self,
        expr_id: hir::ExprId,
        elements: hir::ExprBuffer,
    ) -> Result<TempId, IrLoweringError> {
        let elements = elements
            .iter()
            .map(|expr| self.lower_expr(*expr))
            .collect::<Result<_, _>>()?;
        let dst = self.alloc_temp(self.expr_type(expr_id)?);
        self.emit(Instruction::MakeArray { dst, elements });
        Ok(dst)
    }

    fn lower_struct_init(
        &mut self,
        expr_id: hir::ExprId,
        path: String,
        fields: hir::FieldInitBuffer,
    ) -> Result<TempId, IrLoweringError> {
        let fields = fields
            .iter()
            .map(|field| {
                Ok(StructFieldInit {
                    name: field.name.clone(),
                    value: self.lower_expr(field.value)?,
                })
            })
            .collect::<Result<_, IrLoweringError>>()?;
        let dst = self.alloc_temp(self.expr_type(expr_id)?);
        self.emit(Instruction::MakeStruct {
            dst,
            name: path,
            fields,
        });
        Ok(dst)
    }

    fn lower_field(
        &mut self,
        expr_id: hir::ExprId,
        receiver: hir::ExprId,
        name: String,
    ) -> Result<TempId, IrLoweringError> {
        let base = self.lower_expr(receiver)?;
        let dst = self.alloc_temp(self.expr_type(expr_id)?);
        self.emit(Instruction::ReadField { dst, base, name });
        Ok(dst)
    }

    fn lower_index(
        &mut self,
        expr_id: hir::ExprId,
        receiver: hir::ExprId,
        index: hir::ExprId,
    ) -> Result<TempId, IrLoweringError> {
        let base = self.lower_expr(receiver)?;
        let index = self.lower_expr(index)?;
        let dst = self.alloc_temp(self.expr_type(expr_id)?);
        self.emit(Instruction::ReadIndex { dst, base, index });
        Ok(dst)
    }
}
