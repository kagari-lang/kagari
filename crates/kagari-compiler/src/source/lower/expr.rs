use crate::source::elaborate::model::{
    CheckedCapture, CheckedExprKind, Conversion, OperationId, OperatorSyntax,
};
use crate::source::lower::{
    MirLoweringError,
    instances::CallableInstance,
    state::{FunctionLowerer, LoopScope},
    support::lower_scalar,
};
use kagari_abi::representation::ValueType;
use kagari_contract::{
    operations::UnaryOp, representation::semantic_representation, standard::RuntimePrimitive,
};
use kagari_hir::{
    compile_input::body::BodyKey,
    hir::{
        expr::{Condition, ops::BinaryOp as HirBinaryOp},
        ids::{BlockId, ExprId},
    },
};
use kagari_mir::instruction::{
    CallTarget, Constant, Instruction, MirValue, Terminator, ValueBuffer,
};
use kagari_types::{language::Protocol, scalar::BuiltinType, ty::Ty};
use std::ops::ControlFlow;

mod aggregates;
mod calls;
mod patterns;
mod propagation;
mod shared;

mod equality;
mod interfaces;
mod local_functions;
mod operators;
mod standard;

impl FunctionLowerer<'_, '_> {
    fn lower_closure(
        &mut self,
        expr_id: ExprId,
        key: BodyKey,
        checked: Vec<CheckedCapture>,
        is_async: bool,
        is_block: bool,
    ) -> Result<MirValue, MirLoweringError> {
        let parent = self.source_body().key;
        if key.module != parent.module
            || key.arena != parent.arena
            || key.owner != parent.owner
            || key.closure != Some(expr_id)
        {
            return Err(MirLoweringError::MissingBinding(
                "nested body belongs to another expansion",
            ));
        }
        let mut captures = ValueBuffer::new();
        for capture in checked {
            let local = self.lookup_binding(capture.id)?;
            let physical = if capture.by_cell {
                ValueType::HeapObject
            } else {
                self.value_type(&capture.ty)?
            };
            let value = self.alloc_temp(physical);
            self.emit(Instruction::LoadLocal { dst: value, local });
            let semantic = self.semantic_type(&capture.ty)?;
            self.function
                .semantic
                .registers
                .insert(value.temp.index(), semantic);
            captures.push(value);
        }
        let span = self.expression_span(expr_id);
        let function =
            self.planner
                .enqueue_closure(&self.instance, expr_id, is_async, is_block, span)?;
        if is_block {
            let ty = self
                .expression_type(expr_id)
                .ok_or(MirLoweringError::MissingExprType(expr_id))?;
            let future = self.semantic_type(&ty)?;
            return Ok(self.make_future(function, captures, future));
        }
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
        expressions: &[ExprId],
    ) -> Result<ControlFlow<MirValue, ValueBuffer>, MirLoweringError> {
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

    pub(crate) fn lower_expr(&mut self, expr_id: ExprId) -> Result<MirValue, MirLoweringError> {
        let operation = *self
            .source_body()
            .results
            .get(&expr_id)
            .ok_or(MirLoweringError::UnresolvedExpr(expr_id))?;
        self.lower_operation(operation)
    }

    fn lower_operation(&mut self, operation: OperationId) -> Result<MirValue, MirLoweringError> {
        self.planner.check()?;
        let operation = self.source_body().operation(operation).clone();
        let converted = matches!(operation.kind, CheckedExprKind::Convert { .. });
        let value = self.lower_operation_value(operation.source, operation.kind)?;
        // A symbolic result can become Never after the instance substitution.
        if !self.current_block_terminated() && value.ty == ValueType::Never {
            self.set_terminator(Terminator::Unreachable);
        }
        if !self.current_block_terminated() {
            if !converted {
                self.planner.record_layout_root(
                    &operation.ty,
                    &self.instance.substitution,
                    operation.span,
                )?;
            }
            let semantic = self.semantic_type(&operation.ty)?;
            self.function
                .semantic
                .registers
                .insert(value.temp.index(), semantic);
        }
        Ok(value)
    }

    fn lower_operation_value(
        &mut self,
        expr_id: ExprId,
        kind: CheckedExprKind,
    ) -> Result<MirValue, MirLoweringError> {
        match kind {
            CheckedExprKind::Constant(value) => {
                Ok(self.lower_constant(lower_scalar(value), self.expr_type(expr_id)?))
            }
            CheckedExprKind::Read(binding) => self.lower_binding(expr_id, binding),
            CheckedExprKind::FunctionValue {
                declaration,
                arguments,
            } => self.lower_local_function_value(expr_id, declaration, &arguments),
            CheckedExprKind::AssociatedConstant(fact) => {
                let value = self.planner.associated_constant(
                    &fact,
                    &self.instance.substitution,
                    &self.implementation_bounds,
                    self.expression_span(expr_id),
                )?;
                Ok(self.lower_constant(lower_scalar(value), self.expr_type(expr_id)?))
            }
            CheckedExprKind::Call(call) | CheckedExprKind::Format { call, .. } => {
                self.lower_call(expr_id, call)
            }
            CheckedExprKind::Helper { helper, operands } => {
                let (callee, args) = match self.lower_runtime_helper_call(helper, &operands)? {
                    ControlFlow::Break(value) => return Ok(value),
                    ControlFlow::Continue(call) => call,
                };
                let dst = self.alloc_temp(self.expr_type(expr_id)?);
                self.emit(Instruction::Call {
                    dst: Some(dst),
                    callee,
                    args,
                });
                Ok(dst)
            }
            CheckedExprKind::Diverging(prefix) => match self.lower_values(&prefix)? {
                ControlFlow::Break(value) => Ok(value),
                ControlFlow::Continue(_) => Err(MirLoweringError::MissingBinding(
                    "diverging prefix completed",
                )),
            },
            CheckedExprKind::Convert { value, conversion } => {
                let value = self.lower_operation(value)?;
                if self.current_block_terminated() {
                    return Ok(value);
                }
                match conversion {
                    Conversion::Callable {
                        receiver,
                        interface,
                    } => {
                        let function = self.planner.enqueue_callable(
                            &self.instance,
                            CallableInstance {
                                receiver,
                                interface,
                                span: self.expression_span(expr_id),
                            },
                        )?;
                        let dst = self.alloc_temp(ValueType::HeapObject);
                        self.emit(Instruction::MakeClosure {
                            dst,
                            function,
                            captures: smallvec::smallvec![value],
                        });
                        Ok(dst)
                    }
                    Conversion::Interface(coercion) => {
                        self.lower_interface_coercion(expr_id, value, coercion)
                    }
                }
            }
            CheckedExprKind::NumericCast { value, conversion } => {
                let src = self.lower_expr(value)?;
                if self.current_block_terminated() {
                    return Ok(src);
                }
                let (_, result) = conversion
                    .contract()
                    .ok_or(MirLoweringError::MissingBinding("numeric cast"))?;
                let dst = self.alloc_temp(semantic_representation(&result));
                self.emit(Instruction::Convert {
                    dst,
                    src,
                    conversion,
                });
                Ok(dst)
            }
            CheckedExprKind::Operator { call, syntax } => {
                let values = match self.lower_values(&call.operands)? {
                    ControlFlow::Break(value) => return Ok(value),
                    ControlFlow::Continue(values) => values,
                };
                if let OperatorSyntax::Binary(
                    op @ (HirBinaryOp::Lt | HirBinaryOp::Le | HirBinaryOp::Gt | HirBinaryOp::Ge),
                ) = syntax
                {
                    self.lower_ordering_operator(&call, op, &values)
                } else {
                    self.lower_selected_operator(&call, &values)
                }
            }
            CheckedExprKind::Logical { lhs, op, rhs } => {
                self.lower_short_circuit(expr_id, lhs, op, rhs)
            }
            CheckedExprKind::Unary { operand, op } => {
                let operand = self.lower_expr(operand)?;
                if self.current_block_terminated() {
                    return Ok(operand);
                }
                let dst = self.alloc_temp(self.expr_type(expr_id)?);
                self.emit(Instruction::Unary {
                    dst,
                    op: Self::lower_unary_op(op),
                    operand,
                });
                Ok(dst)
            }
            CheckedExprKind::PrimitiveBinary { lhs, op, rhs } => {
                let values = match self.lower_values(&[lhs, rhs])? {
                    ControlFlow::Break(value) => return Ok(value),
                    ControlFlow::Continue(values) => values,
                };
                let dst = self.alloc_temp(self.expr_type(expr_id)?);
                self.emit(Instruction::Binary {
                    dst,
                    op: Self::lower_binary_op(op),
                    lhs: values[0],
                    rhs: values[1],
                });
                Ok(dst)
            }
            CheckedExprKind::Equality {
                lhs,
                rhs,
                negate,
                receiver,
            } => {
                let values = match self.lower_values(&[lhs, rhs])? {
                    ControlFlow::Break(value) => return Ok(value),
                    ControlFlow::Continue(values) => values,
                };
                let ty = self
                    .planner
                    .arguments(
                        &[receiver],
                        &self.instance.substitution,
                        self.expression_span(expr_id),
                    )?
                    .remove(0);
                let equal = self.lower_protocol(Protocol::PartialEq, &ty, &values, 0)?;
                if !negate {
                    return Ok(equal);
                }
                let dst = self.alloc_temp(ValueType::Bool);
                self.emit(Instruction::Unary {
                    dst,
                    op: UnaryOp::Not,
                    operand: equal,
                });
                Ok(dst)
            }
            CheckedExprKind::Identity(value) => self.lower_expr(value),
            CheckedExprKind::Field { receiver, field } => {
                self.lower_field(expr_id, receiver, field)
            }
            CheckedExprKind::Index {
                receiver,
                index,
                operation,
            } => self.lower_index(expr_id, receiver, index, operation),
            CheckedExprKind::HostRead(path) => self.lower_host_path_read(expr_id, path),
            CheckedExprKind::Struct {
                declaration,
                fields,
                base,
                base_fields,
            } => self.lower_struct_init(expr_id, declaration, fields, base, base_fields),
            CheckedExprKind::Enum {
                declaration,
                variant,
                fields,
            } => {
                let signature = self
                    .planner
                    .catalog
                    .variant(&variant)
                    .ok_or(MirLoweringError::MissingBinding("checked enum variant"))?;
                if signature.owner != declaration || signature.payload.len() != fields.len() {
                    return Err(MirLoweringError::MissingBinding("checked enum constructor"));
                }
                let variant = signature.slot;
                let fields = match self.lower_values(&fields)? {
                    ControlFlow::Break(value) => return Ok(value),
                    ControlFlow::Continue(values) => values,
                };
                let dst = self.alloc_temp(ValueType::HeapObject);
                self.emit(Instruction::MakeEnum {
                    dst,
                    enumeration: self.expr_nominal_instance(expr_id)?,
                    variant,
                    fields,
                });
                Ok(dst)
            }
            CheckedExprKind::Tuple(elements) if elements.is_empty() => Ok(self.lower_unit()),
            CheckedExprKind::Tuple(elements) => self.lower_tuple(expr_id, elements),
            CheckedExprKind::Array(elements) => self.lower_array(expr_id, elements),
            CheckedExprKind::ArrayRepeat { value, count } => {
                let values = match self.lower_values(&[value, count])? {
                    ControlFlow::Break(value) => return Ok(value),
                    ControlFlow::Continue(values) => values,
                };
                let dst = self.alloc_temp(self.expr_type(expr_id)?);
                self.emit(Instruction::RepeatArray {
                    element: self.array_element_type(expr_id)?,
                    dst,
                    value: values[0],
                    count: values[1],
                });
                Ok(dst)
            }
            CheckedExprKind::Range {
                start,
                end,
                inclusive,
            } => self.lower_range(expr_id, start, end, inclusive),
            CheckedExprKind::InterpolatedString(parts) => {
                let elements = match self.lower_values(&parts)? {
                    ControlFlow::Break(value) => return Ok(value),
                    ControlFlow::Continue(values) => values,
                };
                let array = self.alloc_temp(ValueType::HeapObject);
                self.emit(Instruction::MakeArray {
                    element: Ty::Builtin(BuiltinType::String),
                    dst: array,
                    elements,
                });
                let separator = self.lower_constant(Constant::Str(String::new()), ValueType::Str);
                let dst = self.alloc_temp(ValueType::Str);
                self.emit(Instruction::Call {
                    dst: Some(dst),
                    callee: CallTarget::RuntimePrimitive(RuntimePrimitive::StringPartsJoin),
                    args: smallvec::smallvec![array, separator],
                });
                Ok(dst)
            }
            CheckedExprKind::Await { future } => self.lower_await(expr_id, future),
            CheckedExprKind::Propagate { value, protocol } => {
                self.lower_propagation(value, *protocol)
            }
            CheckedExprKind::Block(block) => Ok(self
                .lower_block(block)?
                .unwrap_or_else(|| self.lower_unit())),
            CheckedExprKind::If {
                condition,
                then_branch,
                else_branch,
            } => self.lower_if(expr_id, condition, then_branch, else_branch),
            CheckedExprKind::Match { scrutinee, arms } => {
                self.lower_match(expr_id, scrutinee, arms)
            }
            CheckedExprKind::Loop(body) => self.lower_loop_expr(expr_id, body),
            CheckedExprKind::Closure {
                key,
                is_async,
                captures,
                ..
            } => self.lower_closure(expr_id, key, captures, is_async, false),
            CheckedExprKind::AsyncBlock { key, captures } => {
                self.lower_closure(expr_id, key, captures, true, true)
            }
        }
    }

    fn lower_if(
        &mut self,
        expr_id: ExprId,
        condition: Condition,
        then_branch: BlockId,
        else_branch: Option<ExprId>,
    ) -> Result<MirValue, MirLoweringError> {
        let cond = self.lower_expr(condition.value())?;
        if self.current_block_terminated() {
            return Ok(cond);
        }
        let outer_scope = self.current_scope;
        let then_block = self.new_block();
        let else_block = self.new_block();
        let join_block = self.new_block();
        let result = self.alloc_temp(self.expr_type(expr_id)?);
        let mut bindings = Vec::new();
        match condition {
            Condition::Expr(_) => self.set_terminator(Terminator::Branch {
                cond,
                then_block,
                else_block,
            }),
            Condition::Binding {
                pattern,
                initializer,
            } => {
                let ty = self
                    .expression_type(initializer)
                    .ok_or(MirLoweringError::MissingExprType(initializer))?;
                self.lower_pattern_decision(pattern, cond, &ty, Some(else_block), &mut bindings)?;
                self.set_terminator(Terminator::Jump(then_block));
            }
        }

        self.switch_to_block(then_block);
        self.publish_pattern_bindings(bindings)?;
        let then_value = self.lower_block(then_branch)?;
        if !self.current_block_terminated() {
            let then_value = if else_branch.is_some() {
                then_value.unwrap_or_else(|| self.lower_unit())
            } else {
                self.lower_unit()
            };
            self.emit(Instruction::Move {
                dst: result,
                src: then_value,
            });
            self.set_terminator(Terminator::Jump(join_block));
        }

        self.current_scope = outer_scope;
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
        expr_id: ExprId,
        lhs: ExprId,
        op: HirBinaryOp,
        rhs: ExprId,
    ) -> Result<MirValue, MirLoweringError> {
        let lhs = self.lower_expr(lhs)?;
        if self.current_block_terminated() {
            return Ok(lhs);
        }
        let rhs_block = self.new_block();
        let short_block = self.new_block();
        let join_block = self.new_block();
        let result = self.alloc_temp(self.expr_type(expr_id)?);

        match op {
            HirBinaryOp::AndAnd => {
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
            HirBinaryOp::OrOr => {
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

    fn lower_loop_expr(
        &mut self,
        expr: ExprId,
        body: BlockId,
    ) -> Result<MirValue, MirLoweringError> {
        let result = self.alloc_temp(self.expr_type(expr)?);
        let body_block = self.new_block();
        let exit_block = self.new_block();
        self.ensure_jump(body_block);
        self.loops.push(LoopScope {
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
}
