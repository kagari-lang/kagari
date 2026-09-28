use kagari_abi::{
    numeric::NumericOperation,
    operations::{BinaryOp, UnaryOp},
    representation::ValueType,
    scalar::BuiltinType,
    types::AbiType,
};
use kagari_common::{
    arithmetic::{self, IntegerBinaryOp},
    integer::{self, IntegerOp},
};
use std::cmp::Ordering;

use crate::Constant;

/// Copyable, bounded facts. Strings/heap values/floating-point payloads are not
/// copied or interpreted by these portable scalar passes.
#[derive(Clone, Copy)]
pub(super) enum Scalar {
    Unit,
    Bool(bool),
    I32(i32),
    I64(i64),
    U64(u64),
}
impl Scalar {
    pub(super) fn from_constant(value: &Constant) -> Option<Self> {
        Some(match value {
            Constant::Unit => Self::Unit,
            Constant::Bool(value) => Self::Bool(*value),
            Constant::I32(value) => Self::I32(*value),
            Constant::I64(value) => Self::I64(*value),
            Constant::U64(value) => Self::U64(*value),
            _ => return None,
        })
    }
    pub(super) fn constant(self) -> Constant {
        match self {
            Self::Unit => Constant::Unit,
            Self::Bool(value) => Constant::Bool(value),
            Self::I32(value) => Constant::I32(value),
            Self::I64(value) => Constant::I64(value),
            Self::U64(value) => Constant::U64(value),
        }
    }
    fn integer(self) -> Option<i128> {
        match self {
            Self::I32(value) => Some(value.into()),
            Self::I64(value) => Some(value.into()),
            Self::U64(value) => Some(value.into()),
            _ => None,
        }
    }
    pub(super) fn fits(self, ty: Option<&AbiType>) -> bool {
        match (ty, self.integer()) {
            (Some(AbiType::Builtin(kind)), Some(value)) => kind
                .integer_bounds()
                .is_none_or(|(min, max)| min <= value && value <= max),
            _ => true,
        }
    }
    pub(super) fn unary(self, op: UnaryOp) -> Option<Self> {
        match (op, self) {
            (UnaryOp::Not, Self::Bool(value)) => Some(Self::Bool(!value)),
            (UnaryOp::Neg, Self::I32(value)) => arithmetic::i32_neg(value).ok().map(Self::I32),
            (UnaryOp::Neg, Self::I64(value)) => arithmetic::i64_neg(value).ok().map(Self::I64),
            _ => None,
        }
    }
    pub(super) fn binary(self, op: BinaryOp, rhs: Self) -> Option<Self> {
        if let BinaryOp::Numeric(operation) = op {
            return self.numeric(operation, Some(rhs));
        }
        let comparison = match (self, rhs) {
            (Self::Unit, Self::Unit) => Some(Ordering::Equal),
            (Self::Bool(a), Self::Bool(b)) => Some(a.cmp(&b)),
            (Self::I32(a), Self::I32(b)) => Some(a.cmp(&b)),
            (Self::I64(a), Self::I64(b)) => Some(a.cmp(&b)),
            (Self::U64(a), Self::U64(b)) => Some(a.cmp(&b)),
            _ => None,
        }?;
        let condition = match op {
            BinaryOp::Eq => Some(comparison == Ordering::Equal),
            BinaryOp::NotEq => Some(comparison != Ordering::Equal),
            BinaryOp::Lt => Some(comparison == Ordering::Less),
            BinaryOp::Gt => Some(comparison == Ordering::Greater),
            BinaryOp::Le => Some(comparison != Ordering::Greater),
            BinaryOp::Ge => Some(comparison != Ordering::Less),
            _ => None,
        };
        if let Some(condition) = condition {
            return Some(Self::Bool(condition));
        }
        let arithmetic = match op {
            BinaryOp::Add => IntegerBinaryOp::Add,
            BinaryOp::Sub => IntegerBinaryOp::Sub,
            BinaryOp::Mul => IntegerBinaryOp::Mul,
            BinaryOp::Div => IntegerBinaryOp::Div,
            BinaryOp::Rem => IntegerBinaryOp::Rem,
            _ => return None,
        };
        match (self, rhs) {
            (Self::I32(a), Self::I32(b)) => {
                arithmetic::i32_binary(arithmetic, a, b).ok().map(Self::I32)
            }
            (Self::I64(a), Self::I64(b)) => {
                arithmetic::i64_binary(arithmetic, a, b).ok().map(Self::I64)
            }
            // Use the same fixed-width primitive as typed unsigned operations.
            (Self::U64(_), Self::U64(_)) => {
                let op = match arithmetic {
                    IntegerBinaryOp::Add => IntegerOp::CheckedAdd,
                    IntegerBinaryOp::Sub => IntegerOp::CheckedSub,
                    IntegerBinaryOp::Mul => IntegerOp::CheckedMul,
                    IntegerBinaryOp::Div => IntegerOp::CheckedDiv,
                    IntegerBinaryOp::Rem => IntegerOp::CheckedRem,
                };
                self.numeric(
                    NumericOperation {
                        op,
                        input: BuiltinType::U64,
                        rhs: Some(BuiltinType::U64),
                    },
                    Some(rhs),
                )
            }
            _ => None,
        }
    }
    pub(super) fn numeric(self, operation: NumericOperation, rhs: Option<Self>) -> Option<Self> {
        operation.contract()?;
        let lhs = self.read_integer(operation.input)?;
        let rhs = match (operation.rhs, rhs) {
            (Some(kind), Some(rhs)) => rhs.read_integer(kind)?,
            (None, None) => 0,
            _ => return None,
        };
        let (bits, signed) = operation.input.integer_layout()?;
        let result = integer::integer_operation(operation.op, lhs, rhs, bits, signed).ok()?;
        Some(match AbiType::Builtin(operation.input).representation() {
            ValueType::I32 => Self::I32(result.try_into().ok()?),
            ValueType::I64 => Self::I64(result.try_into().ok()?),
            ValueType::U64 => Self::U64(result.try_into().ok()?),
            _ => return None,
        })
    }
    fn read_integer(self, kind: BuiltinType) -> Option<i128> {
        let representation = match self {
            Self::I32(_) => ValueType::I32,
            Self::I64(_) => ValueType::I64,
            Self::U64(_) => ValueType::U64,
            _ => return None,
        };
        if representation != AbiType::Builtin(kind).representation() {
            return None;
        }
        let value = self.integer()?;
        let (min, max) = kind.integer_bounds()?;
        (min <= value && value <= max).then_some(value)
    }
}
