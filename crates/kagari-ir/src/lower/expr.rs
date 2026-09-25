use kagari_hir::builtin::surface::StandardIntrinsic;
use kagari_hir::{builtin::BuiltinFunction, hir};
use std::ops::ControlFlow;

use crate::lower::IrLoweringError;
use crate::lower::state::FunctionLowerer;
use crate::module::instruction::{
    BinaryOp, CallTarget, Constant, Instruction, IrValue, RuntimeHelper, StructFieldInit,
    Terminator, ValueBuffer,
};
use crate::module::types::ValueType;

impl FunctionLowerer<'_, '_> {
    fn lower_closure(&mut self, expr_id: hir::ExprId) -> Result<IrValue, IrLoweringError> {
        let mut captures = ValueBuffer::new();
        for resolved in self.analyzed.names.closure_captures(expr_id) {
            let local = self.lookup_binding(*resolved)?;
            let ty = match resolved {
                kagari_hir::resolver::ResolvedName::Local(id) => self
                    .analyzed
                    .typed
                    .type_table
                    .local_type(*id)
                    .ok_or(IrLoweringError::MissingLocalType(*id))?,
                kagari_hir::resolver::ResolvedName::Param(id) => self
                    .analyzed
                    .typed
                    .functions
                    .iter()
                    .find(|function| function.id == self.instance.function)
                    .and_then(|function| function.params.iter().find(|param| param.id == *id))
                    .map(|param| param.ty.clone())
                    .ok_or(IrLoweringError::MissingBinding("captured parameter type"))?,
                _ => return Err(IrLoweringError::MissingBinding("closure capture")),
            };
            let physical = if matches!(resolved, kagari_hir::resolver::ResolvedName::Local(id) if self.cell_locals.contains(id))
            {
                ValueType::HeapObject
            } else {
                self.value_type(&ty)?
            };
            let value = self.alloc_temp(physical);
            self.emit(Instruction::LoadLocal { dst: value, local });
            captures.push(value);
        }
        let span = self.analyzed.lowered.source_map.expr_span(expr_id);
        let function = self
            .planner
            .enqueue_closure(&self.instance, expr_id, span)?;
        let dst = self.alloc_temp(ValueType::HeapObject);
        self.emit(Instruction::MakeClosure {
            dst,
            function,
            captures,
        });
        Ok(dst)
    }

    fn lower_values(
        &mut self,
        expressions: &[hir::ExprId],
    ) -> Result<ControlFlow<IrValue, ValueBuffer>, IrLoweringError> {
        let mut values = ValueBuffer::new();
        for expression in expressions {
            let value = self.lower_expr(*expression)?;
            if self.current_block_terminated() {
                return Ok(ControlFlow::Break(value));
            }
            values.push(value);
        }
        Ok(ControlFlow::Continue(values))
    }

    fn record_expr_layout(&mut self, expr_id: hir::ExprId) -> Result<(), IrLoweringError> {
        let ty = self
            .analyzed
            .typed
            .type_table
            .expr_type(expr_id)
            .ok_or(IrLoweringError::MissingExprType(expr_id))?;
        self.planner.record_layout_root(
            &ty,
            &self.instance.substitution,
            self.analyzed.lowered.source_map.expr_span(expr_id),
        )?;
        Ok(())
    }

    pub(crate) fn lower_expr(&mut self, expr_id: hir::ExprId) -> Result<IrValue, IrLoweringError> {
        self.planner.check()?;
        let mut value = self.lower_expr_value(expr_id)?;
        if !self.current_block_terminated() {
            self.record_expr_layout(expr_id)?;
            if let Some(coercion) = self
                .analyzed
                .typed
                .type_table
                .interface_coercion(expr_id)
                .cloned()
            {
                let dst = self.alloc_temp(ValueType::HeapObject);
                self.emit(Instruction::MakeInterface {
                    dst,
                    value,
                    implementation: coercion.implementation,
                });
                value = dst;
            }
        }
        Ok(value)
    }

    fn lower_expr_value(&mut self, expr_id: hir::ExprId) -> Result<IrValue, IrLoweringError> {
        if let Some(target) = self
            .analyzed
            .typed
            .type_table
            .enum_constructor(expr_id)
            .cloned()
        {
            let variant = target
                .variant
                .as_ref()
                .and_then(|id| self.analyzed.aggregates.variant(id))
                .ok_or(IrLoweringError::MissingBinding("checked enum variant"))?
                .slot;
            let args = match &self.analyzed.lowered.module.expr(expr_id).kind {
                hir::ExprKind::Call { args, .. } => args.to_vec(),
                hir::ExprKind::Name { .. } => Vec::new(),
                _ => {
                    return Err(IrLoweringError::MissingBinding(
                        "enum constructor expression",
                    ));
                }
            };
            let fields = match self.lower_values(&args)? {
                ControlFlow::Continue(fields) => fields,
                ControlFlow::Break(value) => return Ok(value),
            };
            let dst = self.alloc_temp(ValueType::HeapObject);
            self.emit(Instruction::MakeEnum {
                dst,
                enumeration: self.expr_nominal_instance(expr_id)?,
                variant,
                fields,
            });
            return Ok(dst);
        }
        if let Some(value) = self
            .analyzed
            .typed
            .type_table
            .scalar_value(expr_id)
            .cloned()
        {
            return Ok(self.lower_constant(value.into(), self.expr_type(expr_id)?));
        }
        let expr = self.analyzed.lowered.module.expr(expr_id).clone();
        match expr.kind {
            hir::ExprKind::Missing => Err(IrLoweringError::UnresolvedExpr(expr_id)),
            hir::ExprKind::Name { .. } => self.lower_name_expr(expr_id),
            hir::ExprKind::Literal(_) => Err(IrLoweringError::MissingBinding("checked literal")),
            hir::ExprKind::Prefix { op, expr } => {
                let operand = self.lower_expr(expr)?;
                if self.current_block_terminated() {
                    return Ok(operand);
                }
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
                if self.current_block_terminated() {
                    return Ok(lhs);
                }
                let rhs = self.lower_expr(rhs)?;
                if self.current_block_terminated() {
                    return Ok(rhs);
                }
                let dst = self.alloc_temp(self.expr_type(expr_id)?);
                self.emit(Instruction::Binary {
                    dst,
                    op: FunctionLowerer::lower_binary_op(op),
                    lhs,
                    rhs,
                });
                Ok(dst)
            }
            hir::ExprKind::Range {
                start,
                end,
                inclusive,
            } => self.lower_range(expr_id, start, end, inclusive),
            hir::ExprKind::Call { args, .. } => self.lower_call(expr_id, &args),
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
            hir::ExprKind::Field { receiver, .. } => self.lower_field(expr_id, receiver),
            hir::ExprKind::Index { receiver, index } => self.lower_index(expr_id, receiver, index),
            hir::ExprKind::Match { scrutinee, arms } => self.lower_match(expr_id, scrutinee, arms),
            hir::ExprKind::Loop { body } => self.lower_loop_expr(expr_id, body),
            hir::ExprKind::StructInit { fields, .. } => self.lower_struct_init(expr_id, fields),
            hir::ExprKind::Tuple(elements) => self.lower_tuple(expr_id, elements),
            hir::ExprKind::Array(elements) => self.lower_array(expr_id, elements),
            hir::ExprKind::Closure { .. } => self.lower_closure(expr_id),
        }
    }

    fn lower_if(
        &mut self,
        expr_id: hir::ExprId,
        condition: hir::ExprId,
        then_branch: hir::BlockId,
        else_branch: Option<hir::ExprId>,
    ) -> Result<IrValue, IrLoweringError> {
        let cond = self.lower_expr(condition)?;
        if self.current_block_terminated() {
            return Ok(cond);
        }
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

        self.switch_to_join(join_block);
        Ok(result)
    }

    fn lower_short_circuit(
        &mut self,
        expr_id: hir::ExprId,
        lhs: hir::ExprId,
        op: hir::BinaryOp,
        rhs: hir::ExprId,
    ) -> Result<IrValue, IrLoweringError> {
        let lhs = self.lower_expr(lhs)?;
        if self.current_block_terminated() {
            return Ok(lhs);
        }
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
    ) -> Result<IrValue, IrLoweringError> {
        let scrutinee_temp = self.lower_expr(scrutinee)?;
        if self.current_block_terminated() {
            return Ok(scrutinee_temp);
        }
        let result = self.alloc_temp(self.expr_type(expr_id)?);
        let exit_block = self.new_block();
        let fail_block = self.new_block();
        let mut decision_block = self.current_block;
        let scrutinee_ty = self
            .analyzed
            .typed
            .type_table
            .expr_type(scrutinee)
            .ok_or(IrLoweringError::MissingExprType(scrutinee))?;

        for arm in arms {
            let outer_scope = self.current_scope;
            let irrefutable = self
                .analyzed
                .lowered
                .module
                .pattern(arm.pattern)
                .kind
                .is_irrefutable();
            let arm_block = self.new_block();
            let next_decision = self.new_block();

            self.switch_to_block(decision_block);
            let mut bindings = Vec::new();
            self.lower_pattern_decision(
                arm.pattern,
                scrutinee_temp,
                &scrutinee_ty,
                next_decision,
                &mut bindings,
            )?;
            self.set_terminator(Terminator::Jump(arm_block));
            self.switch_to_block(arm_block);
            for (local, value) in bindings {
                self.emit(Instruction::StoreLocal { local, src: value });
                self.introduce_debug_local(local);
            }

            let arm_value = self.lower_expr(arm.expr)?;
            if !self.current_block_terminated() {
                self.emit(Instruction::Move {
                    dst: result,
                    src: arm_value,
                });
                self.set_terminator(Terminator::Jump(exit_block));
            }

            self.current_scope = outer_scope;

            decision_block = next_decision;
            if irrefutable {
                break;
            }
        }

        self.switch_to_block(decision_block);
        self.set_terminator(Terminator::Jump(fail_block));

        self.switch_to_block(fail_block);
        self.set_terminator(Terminator::Unreachable);

        self.switch_to_join(exit_block);
        Ok(result)
    }

    fn lower_loop_expr(
        &mut self,
        expr: hir::ExprId,
        body: hir::BlockId,
    ) -> Result<IrValue, IrLoweringError> {
        let result = self.alloc_temp(self.expr_type(expr)?);
        let body_block = self.new_block();
        let exit_block = self.new_block();
        self.ensure_jump(body_block);
        self.loops.push(super::state::LoopScope {
            break_block: exit_block,
            continue_block: body_block,
            break_value: Some(result),
        });
        self.switch_to_block(body_block);
        let _ = self.lower_block(body)?;
        self.ensure_jump(body_block);
        self.loops.pop();
        self.switch_to_join(exit_block);
        Ok(result)
    }

    pub(crate) fn lower_pattern_decision(
        &mut self,
        pattern: hir::PatternId,
        value: IrValue,
        expected: &kagari_hir::types::TypeId,
        fail: crate::module::ids::BlockId,
        bindings: &mut Vec<(crate::module::ids::LocalId, IrValue)>,
    ) -> Result<(), IrLoweringError> {
        match &self.analyzed.lowered.module.pattern(pattern).kind {
            hir::PatternKind::Wildcard => {}
            hir::PatternKind::Name { local, name } => {
                let local = *local;
                let name = name.clone();
                let local_ty = self
                    .analyzed
                    .typed
                    .type_table
                    .local_type(local)
                    .as_ref()
                    .map(|ty| self.value_type(ty))
                    .transpose()?
                    .ok_or(IrLoweringError::MissingLocalType(local))?;
                let ir_local = self.alloc_local(
                    name,
                    local_ty,
                    self.analyzed.lowered.source_map.local_span(local),
                );
                self.locals.insert(local, ir_local);
                bindings.push((ir_local, value));
            }
            hir::PatternKind::Literal(_) => {
                let scalar = self
                    .analyzed
                    .typed
                    .type_table
                    .pattern_scalar_value(pattern)
                    .cloned()
                    .ok_or(IrLoweringError::MissingBinding("checked pattern literal"))?;
                let ty = ValueType::from_type_id(&scalar.ty());
                let literal = self.lower_constant(scalar.into(), ty);
                let cond = self.alloc_temp(ValueType::Bool);
                self.emit(Instruction::Binary {
                    dst: cond,
                    op: BinaryOp::Eq,
                    lhs: value,
                    rhs: literal,
                });
                let next = self.new_block();
                self.set_terminator(Terminator::Branch {
                    cond,
                    then_block: next,
                    else_block: fail,
                });
                self.switch_to_block(next);
            }
            hir::PatternKind::Tuple(elements) => {
                let kagari_hir::types::TypeId::Tuple(types) = expected else {
                    return Err(IrLoweringError::MissingBinding("checked tuple pattern"));
                };
                let elements = elements.clone();
                let types = types.clone();
                if elements.len() != types.len() {
                    return Err(IrLoweringError::MissingBinding(
                        "checked tuple pattern arity",
                    ));
                }
                for (index, (element, ty)) in elements.into_iter().zip(types.iter()).enumerate() {
                    let index = i32::try_from(index)
                        .map_err(|_| IrLoweringError::MissingBinding("tuple pattern index"))?;
                    let index = self.lower_constant(Constant::I32(index), ValueType::I32);
                    let field = self.alloc_temp(self.value_type(ty)?);
                    self.emit(Instruction::ReadAggregateIndex {
                        dst: field,
                        base: value,
                        index,
                    });
                    self.lower_pattern_decision(element, field, ty, fail, bindings)?;
                }
            }
            hir::PatternKind::Struct { fields, .. } => {
                let fields = fields.clone();
                let resolved = self
                    .analyzed
                    .typed
                    .type_table
                    .pattern_fields(pattern)
                    .ok_or(IrLoweringError::MissingBinding("checked struct pattern"))?
                    .to_vec();
                if fields.len() != resolved.len() {
                    return Err(IrLoweringError::MissingBinding(
                        "checked struct pattern fields",
                    ));
                }
                for (field, declaration) in fields.into_iter().zip(resolved.iter()) {
                    let field_ref = self.aggregate_field_ref(declaration, expected)?;
                    let signature = self.analyzed.aggregates.field(declaration).ok_or(
                        IrLoweringError::MissingBinding("struct pattern field signature"),
                    )?;
                    let kagari_hir::types::TypeId::Struct(owner) = expected else {
                        return Err(IrLoweringError::MissingBinding(
                            "checked struct pattern type",
                        ));
                    };
                    let structure = self
                        .analyzed
                        .aggregates
                        .structure(&owner.declaration)
                        .ok_or(IrLoweringError::MissingBinding("struct pattern layout"))?;
                    let substitution = structure
                        .generic_params
                        .iter()
                        .cloned()
                        .zip(owner.arguments.iter().cloned())
                        .collect();
                    let ty = signature.ty.instantiate(&substitution);
                    let member = self.alloc_temp(self.value_type(&ty)?);
                    self.emit(Instruction::ReadAggregateField {
                        dst: member,
                        base: value,
                        field: field_ref,
                    });
                    self.lower_pattern_decision(field.pattern, member, &ty, fail, bindings)?;
                }
            }
            hir::PatternKind::EnumVariant { fields, .. } => {
                let fields = fields.clone();
                let kagari_hir::types::TypeId::Enum(owner) = expected else {
                    return Err(IrLoweringError::MissingBinding("checked enum pattern type"));
                };
                let variant = self
                    .analyzed
                    .typed
                    .type_table
                    .pattern_variant(pattern)
                    .ok_or(IrLoweringError::MissingBinding("checked enum variant"))?;
                let signature = self
                    .analyzed
                    .aggregates
                    .variant(variant)
                    .ok_or(IrLoweringError::MissingBinding("enum variant signature"))?;
                let enumeration = self
                    .analyzed
                    .aggregates
                    .enumeration(&owner.declaration)
                    .ok_or(IrLoweringError::MissingBinding("enum pattern layout"))?;
                let substitution = enumeration
                    .generic_params
                    .iter()
                    .cloned()
                    .zip(owner.arguments.iter().cloned())
                    .collect();
                let payload = signature
                    .payload
                    .iter()
                    .map(|ty| ty.instantiate(&substitution))
                    .collect::<Vec<_>>();
                let slot = signature.slot;
                if fields.len() != payload.len() {
                    return Err(IrLoweringError::MissingBinding(
                        "checked enum pattern arity",
                    ));
                }
                let concrete = kagari_hir::types::NominalType {
                    declaration: owner.declaration.clone(),
                    arguments: self.planner.arguments(
                        &owner.arguments,
                        &self.instance.substitution,
                        self.analyzed.lowered.source_map.pattern_span(pattern),
                    )?,
                };
                let enumeration = crate::module::abi::NominalAbiType::from_checked_type(&concrete);
                let cond = self.alloc_temp(ValueType::Bool);
                self.emit(Instruction::TestEnumVariant {
                    dst: cond,
                    value,
                    enumeration: enumeration.clone(),
                    variant: slot,
                });
                let next = self.new_block();
                self.set_terminator(Terminator::Branch {
                    cond,
                    then_block: next,
                    else_block: fail,
                });
                self.switch_to_block(next);
                for (index, (field, ty)) in fields.into_iter().zip(payload.iter()).enumerate() {
                    let member = self.alloc_temp(self.value_type(ty)?);
                    self.emit(Instruction::ReadEnumPayload {
                        dst: member,
                        value,
                        enumeration: enumeration.clone(),
                        variant: slot,
                        index,
                    });
                    self.lower_pattern_decision(field, member, ty, fail, bindings)?;
                }
            }
        }
        Ok(())
    }

    fn lower_tuple(
        &mut self,
        expr_id: hir::ExprId,
        elements: hir::ExprBuffer,
    ) -> Result<IrValue, IrLoweringError> {
        let elements = match self.lower_values(&elements)? {
            ControlFlow::Continue(elements) => elements,
            ControlFlow::Break(value) => return Ok(value),
        };
        let dst = self.alloc_temp(self.expr_type(expr_id)?);
        self.emit(Instruction::MakeTuple { dst, elements });
        Ok(dst)
    }

    fn lower_array(
        &mut self,
        expr_id: hir::ExprId,
        elements: hir::ExprBuffer,
    ) -> Result<IrValue, IrLoweringError> {
        let elements = match self.lower_values(&elements)? {
            ControlFlow::Continue(elements) => elements,
            ControlFlow::Break(value) => return Ok(value),
        };
        let dst = self.alloc_temp(self.expr_type(expr_id)?);
        self.emit(Instruction::MakeArray { dst, elements });
        Ok(dst)
    }

    fn lower_range(
        &mut self,
        expr_id: hir::ExprId,
        start: hir::ExprId,
        end: hir::ExprId,
        inclusive: bool,
    ) -> Result<IrValue, IrLoweringError> {
        let first = self.lower_expr(start)?;
        if self.current_block_terminated() {
            return Ok(first);
        }
        let last = self.lower_expr(end)?;
        if self.current_block_terminated() {
            return Ok(last);
        }
        let array = self.alloc_temp(self.expr_type(expr_id)?);
        self.emit(Instruction::MakeArray {
            dst: array,
            elements: ValueBuffer::new(),
        });
        let current = self.alloc_temp(ValueType::I32);
        self.emit(Instruction::Move {
            dst: current,
            src: first,
        });
        let condition = self.new_block();
        let append = self.new_block();
        let step = self.new_block();
        let exit = self.new_block();
        self.set_terminator(Terminator::Jump(condition));
        self.switch_to_block(condition);
        let keep_going = self.alloc_temp(ValueType::Bool);
        self.emit(Instruction::Binary {
            dst: keep_going,
            op: if inclusive {
                BinaryOp::Le
            } else {
                BinaryOp::Lt
            },
            lhs: current,
            rhs: last,
        });
        self.set_terminator(Terminator::Branch {
            cond: keep_going,
            then_block: append,
            else_block: exit,
        });
        self.switch_to_block(append);
        let pushed = self.alloc_temp(ValueType::HeapObject);
        self.emit(Instruction::Call {
            dst: Some(pushed),
            callee: CallTarget::StandardIntrinsic(StandardIntrinsic::ArrayPush),
            args: smallvec::smallvec![array, current],
        });
        if inclusive {
            let at_end = self.alloc_temp(ValueType::Bool);
            self.emit(Instruction::Binary {
                dst: at_end,
                op: BinaryOp::Eq,
                lhs: current,
                rhs: last,
            });
            self.set_terminator(Terminator::Branch {
                cond: at_end,
                then_block: exit,
                else_block: step,
            });
        } else {
            self.set_terminator(Terminator::Jump(step));
        }
        self.switch_to_block(step);
        let one = self.lower_constant(Constant::I32(1), ValueType::I32);
        let next = self.alloc_temp(ValueType::I32);
        self.emit(Instruction::Binary {
            dst: next,
            op: BinaryOp::Add,
            lhs: current,
            rhs: one,
        });
        self.emit(Instruction::Move {
            dst: current,
            src: next,
        });
        self.set_terminator(Terminator::Jump(condition));
        self.switch_to_join(exit);
        Ok(array)
    }

    fn lower_struct_init(
        &mut self,
        expr_id: hir::ExprId,
        fields: hir::FieldInitBuffer,
    ) -> Result<IrValue, IrLoweringError> {
        let target = self
            .analyzed
            .typed
            .type_table
            .struct_init(expr_id)
            .cloned()
            .ok_or(IrLoweringError::MissingBinding(
                "checked struct initializer",
            ))?;
        if fields.len() != target.fields.len() {
            return Err(IrLoweringError::MissingBinding(
                "checked initializer field count",
            ));
        }
        let mut lowered_fields = smallvec::SmallVec::new();
        for (field, target) in fields.iter().zip(target.fields) {
            let target =
                target.ok_or(IrLoweringError::MissingBinding("checked initializer field"))?;
            let value = self.lower_expr(field.value)?;
            if self.current_block_terminated() {
                return Ok(value);
            }
            lowered_fields.push(StructFieldInit {
                slot: self
                    .analyzed
                    .aggregates
                    .field(&target)
                    .ok_or(IrLoweringError::MissingBinding("checked field contract"))?
                    .slot,
                value,
            });
        }
        let dst = self.alloc_temp(self.expr_type(expr_id)?);
        self.emit(Instruction::MakeStruct {
            dst,
            structure: self.expr_nominal_instance(expr_id)?,
            fields: lowered_fields,
        });
        Ok(dst)
    }

    fn lower_host_path_read(
        &mut self,
        expr_id: hir::ExprId,
        checked: kagari_hir::typeck::ResolvedHostPath,
    ) -> Result<IrValue, IrLoweringError> {
        let root_or_view = self.lower_expr(checked.root)?;
        if self.current_block_terminated() {
            return Ok(root_or_view);
        }
        let dynamic_args = match self.lower_host_path_arguments(&checked.dynamic_arguments)? {
            ControlFlow::Break(value) => return Ok(value),
            ControlFlow::Continue(values) => values,
        };
        let dst = self.alloc_temp(self.expr_type(expr_id)?);
        let fingerprint = checked
            .contract
            .fingerprint()
            .map_err(|_| IrLoweringError::MissingBinding("checked host path contract"))?;
        self.emit(Instruction::ReadPath {
            dst,
            root_or_view,
            dynamic_args,
            path: crate::module::PathRef {
                declaration: Some(checked.declaration),
                contract_fingerprint: fingerprint,
                root_ty: root_or_view.ty,
                result_ty: dst.ty,
                read_only: true,
                debug_name: "host path read".into(),
            },
        });
        Ok(dst)
    }

    fn lower_field(
        &mut self,
        expr_id: hir::ExprId,
        receiver: hir::ExprId,
    ) -> Result<IrValue, IrLoweringError> {
        if let Some(checked) = self.analyzed.typed.type_table.host_path(expr_id).cloned() {
            return self.lower_host_path_read(expr_id, checked);
        }
        let base = self.lower_expr(receiver)?;
        if self.current_block_terminated() {
            return Ok(base);
        }
        let field = self
            .analyzed
            .typed
            .type_table
            .expr_field(expr_id)
            .ok_or(IrLoweringError::MissingBinding("checked field read"))?;
        let receiver_ty = self
            .analyzed
            .typed
            .type_table
            .expr_type(receiver)
            .ok_or(IrLoweringError::MissingExprType(receiver))?;
        let field = self.aggregate_field_ref(field, &receiver_ty)?;
        let dst = self.alloc_temp(self.expr_type(expr_id)?);
        self.emit(Instruction::ReadAggregateField { dst, base, field });
        Ok(dst)
    }

    fn lower_index(
        &mut self,
        expr_id: hir::ExprId,
        receiver: hir::ExprId,
        index: hir::ExprId,
    ) -> Result<IrValue, IrLoweringError> {
        if let Some(checked) = self.analyzed.typed.type_table.host_path(expr_id).cloned() {
            return self.lower_host_path_read(expr_id, checked);
        }
        let base = self.lower_expr(receiver)?;
        if self.current_block_terminated() {
            return Ok(base);
        }
        let index = self.lower_expr(index)?;
        if self.current_block_terminated() {
            return Ok(index);
        }
        let dst = self.alloc_temp(self.expr_type(expr_id)?);
        self.emit(Instruction::ReadAggregateIndex { dst, base, index });
        Ok(dst)
    }

    fn lower_call(
        &mut self,
        expr: hir::ExprId,
        args: &[hir::ExprId],
    ) -> Result<IrValue, IrLoweringError> {
        use kagari_hir::typeck::CallTarget as SemanticCallTarget;
        let call = self
            .analyzed
            .typed
            .type_table
            .call_resolution(expr)
            .ok_or(IrLoweringError::MissingBinding("checked call target"))?;
        let span = self.analyzed.lowered.source_map.expr_span(expr);
        let (target, impl_arguments, linked_trait_target) =
            if let SemanticCallTarget::TraitMethod { method, interface } = call.target {
                let receiver = call
                    .receiver
                    .ok_or(IrLoweringError::MissingBinding("trait receiver"))?;
                let ty = self
                    .analyzed
                    .typed
                    .type_table
                    .expr_type(receiver)
                    .ok_or(IrLoweringError::MissingExprType(receiver))?;
                let mut types = self
                    .planner
                    .arguments(&[ty], &self.instance.substitution, span)?;
                let ty = types.pop().expect("receiver type");
                let interface = kagari_hir::types::NominalType {
                    declaration: interface.declaration,
                    arguments: self.planner.arguments(
                        &interface.arguments,
                        &self.instance.substitution,
                        span,
                    )?,
                };
                if ty == kagari_hir::types::TypeId::Trait(interface.clone()) {
                    let trait_contract = self
                        .analyzed
                        .aggregates
                        .trait_(&interface.declaration)
                        .ok_or(IrLoweringError::MissingBinding("trait contract"))?;
                    let method_contract = self
                        .analyzed
                        .aggregates
                        .trait_method(&method)
                        .ok_or(IrLoweringError::MissingBinding("trait method contract"))?;
                    if method_contract.generic_params.len() != trait_contract.generic_params.len()
                        || !call.type_arguments.is_empty()
                    {
                        return Err(IrLoweringError::UnsupportedExpr(
                            "interface method requires static specialization",
                        ));
                    }
                    (
                        SemanticCallTarget::TraitMethod {
                            method,
                            interface: interface.clone(),
                        },
                        Vec::new(),
                        Some(CallTarget::InterfaceMethod(Box::new(
                            crate::module::instruction::InterfaceCallContract {
                                interface: crate::module::abi::NominalAbiType::from_checked_type(
                                    &interface,
                                ),
                                method_slot: u32::try_from(method_contract.slot).map_err(|_| {
                                    IrLoweringError::UnsupportedExpr(
                                        "interface method slot overflow",
                                    )
                                })?,
                            },
                        ))),
                    )
                } else if let Some(host_method) = self
                    .analyzed
                    .names
                    .hosts
                    .trait_method_binding(&method, &interface, &ty)
                {
                    (
                        SemanticCallTarget::HostFunction(host_method),
                        Vec::new(),
                        None,
                    )
                } else if let Some((implementation, impl_arguments)) = self
                    .analyzed
                    .typed
                    .type_table
                    .implementation_method(&method, &interface, &ty)
                {
                    (
                        SemanticCallTarget::Function(implementation),
                        impl_arguments,
                        None,
                    )
                } else {
                    let (implementation, impl_arguments) = self
                        .analyzed
                        .aggregates
                        .implementation_method(&method, &interface, &ty)
                        .ok_or(IrLoweringError::UnsupportedExpr(
                            "interface dispatch requires linked implementation tables",
                        ))?;
                    let trait_contract = self
                        .analyzed
                        .aggregates
                        .trait_(&interface.declaration)
                        .ok_or(IrLoweringError::MissingBinding("trait contract"))?;
                    let method_contract = self
                        .analyzed
                        .aggregates
                        .trait_method(&method)
                        .ok_or(IrLoweringError::MissingBinding("trait method contract"))?;
                    let method_params =
                        &method_contract.generic_params[trait_contract.generic_params.len()..];
                    let method_arguments = self.planner.arguments(
                        &call.type_arguments,
                        &self.instance.substitution,
                        span,
                    )?;
                    if method_params.len() != method_arguments.len() {
                        return Err(IrLoweringError::MissingBinding(
                            "checked trait method type arguments",
                        ));
                    }
                    let substitution = trait_contract
                        .generic_params
                        .iter()
                        .cloned()
                        .zip(interface.arguments.iter().cloned())
                        .chain(
                            method_params
                                .iter()
                                .cloned()
                                .zip(method_arguments.iter().cloned()),
                        )
                        .collect();
                    let params = method_contract
                        .params
                        .iter()
                        .map(|param| {
                            let ty = param
                                .ty
                                .with_self(&method_contract.owner, &ty)
                                .instantiate(&substitution);
                            self.planner.value_type(&ty, &Default::default(), span)
                        })
                        .collect::<Result<_, _>>()?;
                    let return_type = self.planner.value_type(
                        &method_contract
                            .return_type
                            .with_self(&method_contract.owner, &ty)
                            .instantiate(&substitution),
                        &Default::default(),
                        span,
                    )?;
                    (
                        SemanticCallTarget::TraitMethod { method, interface },
                        Vec::new(),
                        Some(CallTarget::SourceFunction(Box::new(
                            crate::module::instruction::SourceFunctionContract {
                                declaration: implementation.clone(),
                                arguments: impl_arguments
                                    .into_iter()
                                    .chain(method_arguments)
                                    .collect(),
                                params,
                                return_type,
                            },
                        ))),
                    )
                }
            } else {
                (call.target, Vec::new(), None)
            };
        let (callee, args) = match target {
            SemanticCallTarget::TerminatingCallee => {
                let callee = call.receiver.ok_or(IrLoweringError::MissingBinding(
                    "checked terminating callee",
                ))?;
                let value = self.lower_expr(callee)?;
                if !self.current_block_terminated() {
                    return Err(IrLoweringError::MissingBinding(
                        "callee termination contract",
                    ));
                }
                return Ok(value);
            }
            SemanticCallTarget::RuntimeHelper(helper) => {
                match self.lower_runtime_helper_call(helper, args)? {
                    ControlFlow::Continue(call) => call,
                    ControlFlow::Break(value) => return Ok(value),
                }
            }
            SemanticCallTarget::Value => {
                let receiver = call
                    .receiver
                    .ok_or(IrLoweringError::MissingBinding("closure callee"))?;
                let kagari_hir::types::TypeId::Function { params, result } = self
                    .analyzed
                    .typed
                    .type_table
                    .expr_type(receiver)
                    .ok_or(IrLoweringError::MissingExprType(receiver))?
                else {
                    return Err(IrLoweringError::MissingBinding("checked closure callee"));
                };
                let param_types = params
                    .iter()
                    .map(|ty| self.value_type(ty))
                    .collect::<Result<Vec<_>, _>>()?;
                let return_type = self.value_type(&result)?;
                let value = self.lower_expr(receiver)?;
                if self.current_block_terminated() {
                    return Ok(value);
                }
                let args = match self.lower_values(args)? {
                    ControlFlow::Continue(values) => values,
                    ControlFlow::Break(value) => return Ok(value),
                };
                (
                    CallTarget::Closure {
                        value,
                        params: param_types,
                        return_type,
                    },
                    args,
                )
            }
            target => {
                let mut lowered = ValueBuffer::new();
                if let Some(receiver) = call.receiver {
                    let value = self.lower_expr(receiver)?;
                    if self.current_block_terminated() {
                        return Ok(value);
                    }
                    lowered.push(value);
                }
                match self.lower_values(args)? {
                    ControlFlow::Continue(values) => lowered.extend(values),
                    ControlFlow::Break(value) => return Ok(value),
                }
                let target = match target {
                    SemanticCallTarget::Function(id) => {
                        let arguments = self.planner.arguments(
                            &impl_arguments
                                .iter()
                                .chain(&call.type_arguments)
                                .cloned()
                                .collect::<Vec<_>>(),
                            &self.instance.substitution,
                            span,
                        )?;
                        CallTarget::Function(self.planner.enqueue(id, arguments, span)?)
                    }
                    SemanticCallTarget::SourceFunction(id) => {
                        let imported =
                            self.analyzed.imported_functions.target(id).ok_or(
                                IrLoweringError::MissingBinding("source function contract"),
                            )?;
                        let params = imported
                            .signature
                            .params
                            .iter()
                            .map(|param| {
                                self.planner
                                    .value_type(&param.ty, &Default::default(), span)
                            })
                            .collect::<Result<_, _>>()?;
                        let return_type = self.planner.value_type(
                            &imported.signature.return_type,
                            &Default::default(),
                            span,
                        )?;
                        CallTarget::SourceFunction(Box::new(
                            crate::module::instruction::SourceFunctionContract {
                                declaration: imported.declaration.clone(),
                                arguments: Vec::new(),
                                params,
                                return_type,
                            },
                        ))
                    }
                    SemanticCallTarget::StandardIntrinsic(intrinsic) => {
                        CallTarget::StandardIntrinsic(intrinsic)
                    }
                    SemanticCallTarget::HostFunction(id) => CallTarget::HostFunction(Box::new(
                        self.analyzed
                            .names
                            .hosts
                            .function(id)
                            .ok_or(IrLoweringError::MissingBinding("host declaration"))?
                            .clone(),
                    )),
                    SemanticCallTarget::TraitMethod { .. } => linked_trait_target.ok_or(
                        IrLoweringError::MissingBinding("imported implementation contract"),
                    )?,
                    SemanticCallTarget::TerminatingCallee
                    | SemanticCallTarget::RuntimeHelper(_)
                    | SemanticCallTarget::Value => {
                        unreachable!()
                    }
                };
                (target, lowered)
            }
        };
        let dst = self.alloc_temp(self.expr_type(expr)?);
        self.emit(Instruction::Call {
            dst: Some(dst),
            callee,
            args,
        });
        Ok(dst)
    }

    fn lower_runtime_helper_call(
        &mut self,
        helper: BuiltinFunction,
        args: &[hir::ExprId],
    ) -> Result<ControlFlow<IrValue, (CallTarget, ValueBuffer)>, IrLoweringError> {
        let (target, operands): (_, smallvec::SmallVec<[hir::ExprId; 3]>) = match (helper, args) {
            (BuiltinFunction::TypeOf, [value]) => (
                CallTarget::RuntimeHelper(RuntimeHelper::ReflectTypeOf),
                smallvec::smallvec![*value],
            ),
            (BuiltinFunction::GetField, [base, field]) => (
                CallTarget::RuntimeHelper(RuntimeHelper::ReflectGetField(
                    self.checked_field_name(*field)?,
                )),
                smallvec::smallvec![*base],
            ),
            (BuiltinFunction::SetField, [base, field, value]) => (
                CallTarget::RuntimeHelper(RuntimeHelper::ReflectSetField(
                    self.checked_field_name(*field)?,
                )),
                smallvec::smallvec![*base, *value],
            ),
            (BuiltinFunction::SetIndex, [base, index, value]) => (
                CallTarget::RuntimeHelper(RuntimeHelper::ReflectSetIndex),
                smallvec::smallvec![*base, *index, *value],
            ),
            (BuiltinFunction::Print, [message]) => (
                CallTarget::HostFunction(Box::new(kagari_common::host_interface::standard_log())),
                smallvec::smallvec![*message],
            ),
            _ => {
                return Err(IrLoweringError::MissingBinding(
                    "checked runtime helper arguments",
                ));
            }
        };
        Ok(self
            .lower_values(&operands)?
            .map_continue(|values| (target, values)))
    }

    fn checked_field_name(&self, expr: hir::ExprId) -> Result<String, IrLoweringError> {
        match self.analyzed.typed.type_table.scalar_value(expr) {
            Some(kagari_hir::typeck::ScalarValue::String(value)) => Ok(value.clone()),
            _ => Err(IrLoweringError::MissingBinding(
                "checked reflection field name",
            )),
        }
    }
}
