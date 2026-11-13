//! Cold canonical scalar opcodes use the same admitted slots and numeric kernels.
use crate::{
    Runtime,
    error::{RuntimeError, trap::TrapReason},
    frame::{
        FrameStack,
        instruction::{Inputs, InstructionScope},
    },
    numeric,
    value::carriers::ScopedValue,
    value_check::scoped::CheckError,
    value_semantics::comparison,
};
use kagari_abi::representation::ValueType;
use kagari_bytecode::instruction::{BinaryOp, Slot, UnaryOp};
use kagari_contract::numeric::{NumericConversion, NumericOperation};
use std::slice;

impl FrameStack {
    pub(crate) fn apply_scalar_unary(
        &self,
        runtime: &mut Runtime,
        destination: Slot,
        operation: UnaryOp,
        operand: Slot,
    ) -> Result<(), RuntimeError> {
        let inputs = Inputs::prepare(self, runtime, slice::from_ref(&operand), destination)?;
        inputs.write(self, runtime, |cx| {
            let value = numeric::unary(operation, cx.view(0)?)?;
            Ok(value.get(cx.slots)?)
        })
    }

    pub(crate) fn apply_scalar_binary(
        &self,
        runtime: &mut Runtime,
        destination: Slot,
        operation: BinaryOp,
        lhs: Slot,
        rhs: Slot,
    ) -> Result<(), RuntimeError> {
        Inputs::prepare(self, runtime, &[lhs, rhs], destination)?
            .write(self, runtime, |cx| binary(cx, operation))
    }

    pub(crate) fn convert_scalar(
        &self,
        runtime: &mut Runtime,
        destination: Slot,
        operation: NumericConversion,
        operand: Slot,
    ) -> Result<(), RuntimeError> {
        let inputs = Inputs::prepare(self, runtime, slice::from_ref(&operand), destination)?;
        inputs.write(self, runtime, |cx| {
            let value = numeric::convert(operation, cx.view(0)?)?;
            Ok(value.get(cx.slots)?)
        })
    }

    pub(crate) fn apply_numeric(
        &self,
        runtime: &mut Runtime,
        destination: Slot,
        operation: NumericOperation,
        lhs: Slot,
        rhs: Option<Slot>,
    ) -> Result<(), RuntimeError> {
        let operands = [lhs, rhs.unwrap_or(lhs)];
        let inputs = Inputs::prepare(
            self,
            runtime,
            &operands[..1 + usize::from(rhs.is_some())],
            destination,
        )?;
        inputs.write(self, runtime, |cx| {
            let lhs = cx.view(0)?;
            let rhs = (cx.len() == 2).then(|| cx.view(1)).transpose()?;
            let value = numeric::fixed_integer(operation, lhs, rhs)?;
            Ok(value.get(cx.slots)?)
        })
    }

    pub(crate) fn branch_scalar_predicate(
        &self,
        runtime: &mut Runtime,
        operation: BinaryOp,
        operands: [Slot; 2],
        negated: bool,
        targets: (usize, usize),
    ) -> Result<(), RuntimeError> {
        let inputs = Inputs::prepare_effect(self, runtime, &operands)?;
        let predicate = inputs.access(runtime, |cx| {
            let value = binary(cx, operation)?;
            match value.scalar_payload() {
                Some((ValueType::Bool, value)) => Ok(value != 0),
                _ => Err(RuntimeError::from(TrapReason::Provider(
                    "predicate result must be bool".into(),
                ))
                .into()),
            }
        })?;
        // Branch admission/PC mutation occurs after the source loan has ended.
        self.current_mut(runtime)?.jump_to(
            runtime,
            if predicate != negated {
                targets.0
            } else {
                targets.1
            },
        )
    }
}

fn binary<'gc>(
    cx: &mut InstructionScope<'_, '_, 'gc, '_>,
    operation: BinaryOp,
) -> Result<ScopedValue<'gc>, CheckError> {
    match operation {
        BinaryOp::IdentityEq | BinaryOp::IdentityNotEq => {
            let lhs = cx.field(0)?;
            let rhs = cx.field(1)?;
            let equal = comparison::identity_equal(lhs, rhs)?;
            Ok(ScopedValue::from(
                equal == (operation == BinaryOp::IdentityEq),
            ))
        }
        BinaryOp::Eq | BinaryOp::NotEq => {
            let lhs = cx.field(0)?;
            let rhs = cx.field(1)?;
            let equal = comparison::script_equal(cx.slots.scope(), cx.resources, lhs, rhs)?;
            Ok(ScopedValue::from(equal == (operation == BinaryOp::Eq)))
        }
        operation => {
            let lhs = cx.view(0)?;
            let rhs = cx.view(1)?;
            let result = match operation {
                BinaryOp::Lt | BinaryOp::Gt | BinaryOp::Le | BinaryOp::Ge => {
                    numeric::scalar_binary(operation, lhs, rhs).map_err(|_| {
                        RuntimeError::from(TrapReason::Provider(
                            match operation {
                                BinaryOp::Lt => "lt expects matching numeric operands",
                                BinaryOp::Gt => "gt expects matching numeric operands",
                                BinaryOp::Le => "le expects matching numeric operands",
                                BinaryOp::Ge => "ge expects matching numeric operands",
                                _ => unreachable!(),
                            }
                            .into(),
                        ))
                    })?
                }
                _ => numeric::binary(operation, lhs, rhs)?,
            };
            Ok(result.get(cx.slots)?)
        }
    }
}
