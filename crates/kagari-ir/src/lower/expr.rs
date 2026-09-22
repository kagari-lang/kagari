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
        let value = self.lower_expr_value(expr_id)?;
        if !self.current_block_terminated() {
            self.record_expr_layout(expr_id)?;
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
            hir::ExprKind::StructInit { fields, .. } => self.lower_struct_init(expr_id, fields),
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

        for arm in arms {
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
            match &self.analyzed.lowered.module.pattern(arm.pattern).kind {
                hir::PatternKind::Wildcard => {
                    self.set_terminator(Terminator::Jump(arm_block));
                }
                hir::PatternKind::Literal(_) => {
                    let value = self
                        .analyzed
                        .typed
                        .type_table
                        .pattern_scalar_value(arm.pattern)
                        .cloned()
                        .ok_or(IrLoweringError::MissingBinding("checked pattern literal"))?;
                    let ty = ValueType::from_type_id(&value.ty());
                    let literal_temp = self.lower_constant(value.into(), ty);
                    let cond = self.alloc_temp(ValueType::Bool);
                    self.emit(Instruction::Binary {
                        dst: cond,
                        op: BinaryOp::Eq,
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
                        .as_ref()
                        .map(|ty| self.value_type(ty))
                        .transpose()?
                        .ok_or(IrLoweringError::MissingLocalType(*local))?;
                    let ir_local = self.alloc_local(
                        name.clone(),
                        local_ty,
                        self.analyzed.lowered.source_map.local_span(*local),
                    );
                    self.locals.insert(*local, ir_local);
                    self.set_terminator(Terminator::Jump(arm_block));

                    self.switch_to_block(arm_block);
                    self.emit(Instruction::StoreLocal {
                        local: ir_local,
                        src: scrutinee_temp,
                    });
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

    fn lower_field(
        &mut self,
        expr_id: hir::ExprId,
        receiver: hir::ExprId,
    ) -> Result<IrValue, IrLoweringError> {
        if let Some(checked) = self.analyzed.typed.type_table.host_path(expr_id).cloned() {
            let root_or_view = self.lower_expr(checked.root)?;
            if self.current_block_terminated() {
                return Ok(root_or_view);
            }
            let dst = self.alloc_temp(self.expr_type(expr_id)?);
            let fingerprint = checked
                .contract
                .fingerprint()
                .map_err(|_| IrLoweringError::MissingBinding("checked host path contract"))?;
            self.emit(Instruction::ReadPath {
                dst,
                root_or_view,
                dynamic_args: Default::default(),
                path: crate::module::PathRef {
                    field_declaration: Some(checked.declaration),
                    contract_fingerprint: fingerprint,
                    root_ty: root_or_view.ty,
                    result_ty: dst.ty,
                    read_only: true,
                    debug_name: "host field read".into(),
                },
            });
            return Ok(dst);
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
        let target = if let SemanticCallTarget::TraitMethod(method) = call.target {
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
            let implementation = self
                .analyzed
                .typed
                .type_table
                .implementation_method(&method, &ty)
                .ok_or(IrLoweringError::UnsupportedExpr(
                    "interface dispatch requires linked implementation tables",
                ))?;
            SemanticCallTarget::Function(implementation)
        } else {
            call.target
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
            SemanticCallTarget::TraitMethod(_) => {
                return Err(IrLoweringError::UnsupportedExpr(
                    "interface dispatch requires linked implementation tables",
                ));
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
                            &call.type_arguments,
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
                    SemanticCallTarget::TerminatingCallee
                    | SemanticCallTarget::RuntimeHelper(_)
                    | SemanticCallTarget::TraitMethod(_) => {
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
