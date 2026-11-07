//! Emit admitted call operands once, then consume canonical instance plans.

use crate::source::{
    elaborate::model::{CheckedCall, CheckedCallTarget, CheckedExprKind, MethodOperands},
    logging,
    lower::{MirLoweringError, state::FunctionLowerer},
};
use kagari_abi::representation::ValueType;
use kagari_hir::{
    builtin::BuiltinFunction, hir::ids::ExprId, typeck::scalar::ScalarValue, types::TypeId,
};
use kagari_mir::instruction::{CallTarget, Instruction, MirValue, RuntimeHelper, ValueBuffer};
use smallvec::SmallVec;
use std::{ops::ControlFlow, slice};

impl FunctionLowerer<'_, '_> {
    pub(super) fn call_function_value(
        &mut self,
        callback: MirValue,
        result: &TypeId,
        args: &[MirValue],
    ) -> Result<MirValue, MirLoweringError> {
        let dst = self.alloc_temp(self.value_type(result)?);
        self.emit(Instruction::Call {
            dst: Some(dst),
            callee: CallTarget::Closure {
                value: callback,
                params: args.iter().map(|v| v.ty).collect(),
                return_type: dst.ty,
            },
            args: args.iter().copied().collect(),
        });
        self.function
            .semantic
            .registers
            .insert(dst.temp.index(), self.planner.encode_type(result));
        Ok(dst)
    }

    pub(super) fn lower_call(
        &mut self,
        expr: ExprId,
        call: CheckedCall,
    ) -> Result<MirValue, MirLoweringError> {
        let span = self.expression_span(expr);
        // A callable value is evaluated before its arguments. A selected method's
        // receiver is already the first ordinary operand in the checked contract.
        let callback = if let CheckedCallTarget::Value { callee } = call.target {
            let value = self.lower_expr(callee)?;
            if self.current_block_terminated() {
                return Ok(value);
            }
            Some(value)
        } else {
            None
        };
        let mut values = match self.lower_values(&call.operands)? {
            ControlFlow::Break(value) => return Ok(value),
            ControlFlow::Continue(values) => values,
        };
        let target = match call.target {
            CheckedCallTarget::Direct {
                declaration,
                arguments,
            } => self.planner.direct_call(
                declaration,
                &arguments,
                &call.signature,
                &self.instance.substitution,
                span,
            )?,
            CheckedCallTarget::Value { .. } => CallTarget::Closure {
                value: callback.expect("callable evaluated before operands"),
                params: call
                    .signature
                    .params
                    .iter()
                    .map(|ty| self.value_type(ty))
                    .collect::<Result<_, _>>()?,
                return_type: self.value_type(&call.signature.return_type)?,
            },
            CheckedCallTarget::Method {
                operands,
                method,
                interface,
                receiver,
                arguments,
            } => {
                if operands == MethodOperands::Callable {
                    let concrete = self
                        .planner
                        .arguments(
                            slice::from_ref(&receiver),
                            &self.instance.substitution,
                            span,
                        )?
                        .remove(0);
                    if let TypeId::Function { result, .. } = concrete {
                        return self.call_function_value(values[0], &result, &values[1..]);
                    }
                    let tuple = if values.len() == 1 {
                        self.lower_unit()
                    } else {
                        let dst = self.alloc_temp(ValueType::HeapObject);
                        self.emit(Instruction::MakeTuple {
                            dst,
                            elements: values[1..].iter().copied().collect(),
                        });
                        dst
                    };
                    values.truncate(1);
                    values.push(tuple);
                }
                return self
                    .lower_applied_method(interface, receiver, &method, &arguments, &values);
            }
        };
        let dst = self.alloc_temp(self.expr_type(expr)?);
        self.emit(Instruction::Call {
            dst: Some(dst),
            callee: target,
            args: values,
        });
        Ok(dst)
    }

    pub(super) fn lower_runtime_helper_call(
        &mut self,
        helper: BuiltinFunction,
        args: &[ExprId],
    ) -> Result<ControlFlow<MirValue, (CallTarget, ValueBuffer)>, MirLoweringError> {
        let (target, operands): (_, SmallVec<[ExprId; 3]>) = match (helper, args) {
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
                CallTarget::Native(Box::new(logging::import(
                    self.planner.context,
                    &self.planner.options.cancel,
                )?)),
                smallvec::smallvec![*message],
            ),
            _ => {
                return Err(MirLoweringError::MissingBinding(
                    "checked runtime helper arguments",
                ));
            }
        };
        Ok(self
            .lower_values(&operands)?
            .map_continue(|values| (target, values)))
    }

    pub(super) fn checked_field_name(&self, expr: ExprId) -> Result<String, MirLoweringError> {
        match self.source_body().value(expr).map(|value| &value.kind) {
            Some(CheckedExprKind::Constant(ScalarValue::String(value))) => Ok(value.clone()),
            _ => Err(MirLoweringError::MissingBinding(
                "checked reflection field name",
            )),
        }
    }
}
