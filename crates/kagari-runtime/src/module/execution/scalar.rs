//! Select complete scalar operations and input shapes from verified code once.
use crate::{
    module::execution::{
        ExecutionInstruction, ScalarSlot,
        layout::{FrameLayout, scalar_type},
    },
    numeric::binary_operation,
};
use kagari_abi::representation::ValueType;
use kagari_bytecode::{
    instruction::{BinaryOp, BytecodeInstruction, JumpTarget, ScalarOperand, Slot, UnaryOp},
    module::{BytecodeFunction, BytecodeModule},
};
use kagari_contract::representation::builtin_representation;
use kagari_types::{
    integer::IntegerOp,
    payload::{
        comparison::ComparisonDomain,
        operation::{FloatOperation, FloatType, ScalarOperation, ShiftKind},
    },
    scalar::IntegerType,
};

#[derive(Clone, Copy)]
pub(super) enum ScalarInput {
    Slot(ScalarSlot),
    Constant(u64),
}

pub(super) enum ScalarTarget {
    Store(ScalarSlot),
    Branch {
        then_target: JumpTarget,
        else_target: JumpTarget,
    },
}

fn binary(op: BinaryOp, representation: ValueType) -> Option<ScalarOperation> {
    match op {
        BinaryOp::Numeric(operation) => {
            ScalarOperation::integer(operation.op, operation.input, operation.rhs)
        }
        BinaryOp::IdentityEq | BinaryOp::IdentityNotEq => None,
        _ => ScalarOperation::binary(binary_operation(op), scalar_type(representation)?),
    }
}

pub(super) fn prepare(
    instruction: &BytecodeInstruction,
    function: &BytecodeFunction,
    module: &BytecodeModule,
    layout: &FrameLayout,
) -> Option<ExecutionInstruction> {
    let slot = |slot: Slot| layout.location(slot.index())?.operand.scalar();
    let input = |operand: ScalarOperand| -> Option<ScalarInput> {
        Some(match operand {
            ScalarOperand::Slot(value) => ScalarInput::Slot(slot(value)?),
            ScalarOperand::Constant { constant, ty } => ScalarInput::Constant(
                module.constants[constant.index()]
                    .numeric_payload(ty)
                    .expect("verified numeric constant"),
            ),
        })
    };
    let representation = |operand: ScalarOperand| match operand {
        ScalarOperand::Slot(slot) => function.metadata.slots[slot.index()],
        ScalarOperand::Constant { ty, .. } => builtin_representation(ty),
    };
    let (operation, lhs, rhs, target) = match *instruction {
        BytecodeInstruction::Binary { dst, op, lhs, rhs } => (
            binary(op, function.metadata.slots[lhs.index()])?,
            ScalarInput::Slot(slot(lhs)?),
            ScalarInput::Slot(slot(rhs)?),
            ScalarTarget::Store(slot(dst)?),
        ),
        BytecodeInstruction::BinaryOperands { dst, op, lhs, rhs } => (
            binary(op, representation(lhs))?,
            input(lhs)?,
            input(rhs)?,
            ScalarTarget::Store(slot(dst)?),
        ),
        BytecodeInstruction::PredicateBranch {
            op,
            lhs,
            rhs,
            negated,
            then_target,
            else_target,
        } => (
            binary(op, representation(lhs))?,
            input(lhs)?,
            input(rhs)?,
            ScalarTarget::Branch {
                then_target: if negated { else_target } else { then_target },
                else_target: if negated { then_target } else { else_target },
            },
        ),
        BytecodeInstruction::Numeric {
            dst,
            operation,
            lhs,
            rhs,
        } => (
            ScalarOperation::integer(operation.op, operation.input, operation.rhs)?,
            ScalarInput::Slot(slot(lhs)?),
            ScalarInput::Slot(slot(rhs.unwrap_or(lhs))?),
            ScalarTarget::Store(slot(dst)?),
        ),
        BytecodeInstruction::Unary { dst, op, operand } => (
            match op {
                UnaryOp::Neg => ScalarOperation::negation(scalar_type(
                    function.metadata.slots[operand.index()],
                )?)?,
                UnaryOp::Not => ScalarOperation::BoolNot,
            },
            ScalarInput::Slot(slot(operand)?),
            ScalarInput::Slot(slot(operand)?),
            ScalarTarget::Store(slot(dst)?),
        ),
        BytecodeInstruction::Convert {
            dst,
            src,
            conversion,
        } => (
            ScalarOperation::conversion(conversion.source, conversion.target)?,
            ScalarInput::Slot(slot(src)?),
            ScalarInput::Slot(slot(src)?),
            ScalarTarget::Store(slot(dst)?),
        ),
        _ => return None,
    };
    ExecutionInstruction::scalar_operation(operation, lhs, rhs, target)
}

macro_rules! prepare_scalar {
    (binary { $(($bp:pat, $brr:ident, $brk:ident, $bt:ty, $be:path),)* }
     shift { $(($sk:ident, $srr:ident, $srk:ident, $se:path),)* }
     compare { $(($cd:ident, $crr:ident, $crk:ident, $jrr:ident, $jrk:ident, $ct:ty, $ce:path),)* }
     simple_compare { $(($sd:ident, $qrr:ident, $qjrr:ident, $qe:path),)* }
     unary { $(($u:ident, $ur:ident, $ue:path),)* }
     convert { $(($cv:ident, $cs:ty, $ctarget:ty),)* }) => {
        impl ExecutionInstruction {
            pub(super) fn scalar_operation(operation: ScalarOperation, lhs: ScalarInput, rhs: ScalarInput, target: ScalarTarget) -> Option<Self> {
                match (operation, lhs, rhs, target) {
                    $(
                        ($bp, ScalarInput::Slot(lhs), ScalarInput::Slot(rhs), ScalarTarget::Store(dst)) => Some(Self::$brr { dst, lhs, rhs }),
                        ($bp, ScalarInput::Slot(lhs), ScalarInput::Constant(rhs), ScalarTarget::Store(dst)) => Some(Self::$brk { dst, lhs, rhs: rhs as $bt }),
                    )*
                    $(
                        (ScalarOperation::Shift { kind: ShiftKind::$sk, count_domain }, ScalarInput::Slot(lhs), ScalarInput::Slot(rhs), ScalarTarget::Store(dst)) => Some(Self::$srr { dst, lhs, rhs, count_domain }),
                        (ScalarOperation::Shift { kind: ShiftKind::$sk, count_domain }, ScalarInput::Slot(lhs), ScalarInput::Constant(rhs), ScalarTarget::Store(dst)) => Some(Self::$srk { dst, lhs, rhs, count_domain }),
                    )*
                    $(
                        (ScalarOperation::Compare { domain: ComparisonDomain::$cd, condition }, ScalarInput::Slot(lhs), ScalarInput::Slot(rhs), ScalarTarget::Store(dst)) => Some(Self::$crr { dst, lhs, rhs, condition }),
                        (ScalarOperation::Compare { domain: ComparisonDomain::$cd, condition }, ScalarInput::Slot(lhs), ScalarInput::Constant(rhs), ScalarTarget::Store(dst)) => Some(Self::$crk { dst, lhs, rhs: rhs as $ct, condition }),
                        (ScalarOperation::Compare { domain: ComparisonDomain::$cd, condition }, ScalarInput::Slot(lhs), ScalarInput::Slot(rhs), ScalarTarget::Branch { then_target, else_target }) => Some(Self::$jrr { lhs, rhs, then_target, else_target, condition }),
                        (ScalarOperation::Compare { domain: ComparisonDomain::$cd, condition }, ScalarInput::Slot(lhs), ScalarInput::Constant(rhs), ScalarTarget::Branch { then_target, else_target }) => Some(Self::$jrk { lhs, rhs: rhs as $ct, then_target, else_target, condition }),
                    )*
                    $(
                        (ScalarOperation::Compare { domain: ComparisonDomain::$sd, condition }, ScalarInput::Slot(lhs), ScalarInput::Slot(rhs), ScalarTarget::Store(dst)) => Some(Self::$qrr { dst, lhs, rhs, condition }),
                        (ScalarOperation::Compare { domain: ComparisonDomain::$sd, condition }, ScalarInput::Slot(lhs), ScalarInput::Slot(rhs), ScalarTarget::Branch { then_target, else_target }) => Some(Self::$qjrr { lhs, rhs, then_target, else_target, condition }),
                    )*
                    $((ScalarOperation::$u, ScalarInput::Slot(src), _, ScalarTarget::Store(dst)) => Some(Self::$ur { dst, src }),)*
                    $((ScalarOperation::$cv { source, target }, ScalarInput::Slot(src), _, ScalarTarget::Store(dst)) => Some(Self::$cv { dst, src, source, target }),)*
                    (ScalarOperation::BoolToInteger { target }, ScalarInput::Slot(src), _, ScalarTarget::Store(dst)) => Some(Self::BoolToInteger { dst, src, target }),
                    (operation, lhs, rhs, target) => prepare_shared(operation, lhs, rhs, target),
                }
            }
        }
    };
}

scalar_catalog!(prepare_scalar);

fn prepare_shared(
    operation: ScalarOperation,
    lhs: ScalarInput,
    rhs: ScalarInput,
    target: ScalarTarget,
) -> Option<ExecutionInstruction> {
    let ScalarInput::Constant(lhs) = lhs else {
        return None;
    };
    macro_rules! store {
        ($kr:ident, $kk:ident, $dst:ident, $($field:ident),+) => {
            Some(match rhs {
                ScalarInput::Slot(rhs) => ExecutionInstruction::$kr { dst: $dst, lhs, rhs, $($field),+ },
                ScalarInput::Constant(rhs) => ExecutionInstruction::$kk { dst: $dst, lhs, rhs, $($field),+ },
            })
        };
    }
    match (operation, target) {
        (ScalarOperation::Integer { domain, op }, ScalarTarget::Store(dst)) => {
            store!(IntegerBinaryKR, IntegerBinaryKK, dst, domain, op)
        }
        (ScalarOperation::Float { domain, op }, ScalarTarget::Store(dst)) => {
            store!(FloatBinaryKR, FloatBinaryKK, dst, domain, op)
        }
        (ScalarOperation::Shift { kind, count_domain }, ScalarTarget::Store(dst)) => {
            store!(ShiftKR, ShiftKK, dst, kind, count_domain)
        }
        (ScalarOperation::Compare { domain, condition }, ScalarTarget::Store(dst)) => {
            store!(CompareStoreKR, CompareStoreKK, dst, domain, condition)
        }
        (
            ScalarOperation::Compare { domain, condition },
            ScalarTarget::Branch {
                then_target,
                else_target,
            },
        ) => Some(match rhs {
            ScalarInput::Slot(rhs) => ExecutionInstruction::CompareBranchKR {
                lhs,
                rhs,
                domain,
                condition,
                then_target,
                else_target,
            },
            // This pure predicate cannot trap after constant admission. Keep its PC.
            ScalarInput::Constant(rhs) => ExecutionInstruction::Jump(
                if domain
                    .evaluate(condition, lhs, rhs)
                    .expect("verified comparison constants")
                    == 1
                {
                    then_target
                } else {
                    else_target
                },
            ),
        }),
        _ => None,
    }
}
