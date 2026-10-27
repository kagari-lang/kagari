//! Admission of supported scalar operation/domain combinations.
use crate::{
    integer::IntegerOp,
    payload::{
        ScalarBinaryOp,
        comparison::{Comparison, ComparisonDomain},
        operation::{FloatOperation, FloatType, ScalarOperation, ShiftKind},
    },
    scalar::{BuiltinType, IntegerType},
};

impl ScalarOperation {
    pub fn binary(op: ScalarBinaryOp, input: BuiltinType) -> Option<Self> {
        let condition = match op {
            ScalarBinaryOp::Eq => Some(Comparison::Eq),
            ScalarBinaryOp::NotEq => Some(Comparison::NotEq),
            ScalarBinaryOp::Lt => Some(Comparison::Lt),
            ScalarBinaryOp::Le => Some(Comparison::Le),
            ScalarBinaryOp::Gt => Some(Comparison::Gt),
            ScalarBinaryOp::Ge => Some(Comparison::Ge),
            _ => None,
        };
        if let Some(condition) = condition {
            let domain = match input {
                BuiltinType::I32 => ComparisonDomain::I32,
                BuiltinType::I64 => ComparisonDomain::I64,
                BuiltinType::U64 => ComparisonDomain::U64,
                BuiltinType::F32 => ComparisonDomain::F32,
                BuiltinType::F64 => ComparisonDomain::F64,
                BuiltinType::Char => ComparisonDomain::Char,
                BuiltinType::Bool | BuiltinType::Unit
                    if matches!(condition, Comparison::Eq | Comparison::NotEq) =>
                {
                    ComparisonDomain::Scalar
                }
                _ => return None,
            };
            return Some(Self::Compare { domain, condition });
        }
        if matches!(
            input,
            BuiltinType::I32 | BuiltinType::I64 | BuiltinType::U64
        ) {
            let op = match op {
                ScalarBinaryOp::Add => IntegerOp::CheckedAdd,
                ScalarBinaryOp::Sub => IntegerOp::CheckedSub,
                ScalarBinaryOp::Mul => IntegerOp::CheckedMul,
                ScalarBinaryOp::Div => IntegerOp::CheckedDiv,
                ScalarBinaryOp::Rem => IntegerOp::CheckedRem,
                _ => return None,
            };
            return Self::integer(op, input, Some(input));
        }
        let domain = FloatType::from_builtin(input)?;
        let op = match op {
            ScalarBinaryOp::Add => FloatOperation::Add,
            ScalarBinaryOp::Sub => FloatOperation::Sub,
            ScalarBinaryOp::Mul => FloatOperation::Mul,
            ScalarBinaryOp::Div => FloatOperation::Div,
            ScalarBinaryOp::Rem => FloatOperation::Rem,
            _ => return None,
        };
        Some(Self::Float { domain, op })
    }

    pub fn integer(op: IntegerOp, input: BuiltinType, rhs: Option<BuiltinType>) -> Option<Self> {
        let domain = IntegerType::from_builtin(input)?;
        Some(match op {
            IntegerOp::Shl | IntegerOp::Shr => Self::Shift {
                kind: ShiftKind::new(domain, op)?,
                count_domain: IntegerType::from_builtin(rhs?)?,
            },
            IntegerOp::BitNot => match domain {
                IntegerType::I8 => Self::I8BitNot,
                IntegerType::I16 => Self::I16BitNot,
                IntegerType::I32 => Self::I32BitNot,
                IntegerType::I64 => Self::I64BitNot,
                IntegerType::U8 => Self::U8BitNot,
                IntegerType::U16 => Self::U16BitNot,
                IntegerType::U32 => Self::U32BitNot,
                IntegerType::U64 => Self::U64BitNot,
            },
            _ => Self::Integer { domain, op },
        })
    }

    pub fn negation(input: BuiltinType) -> Option<Self> {
        Some(match input {
            BuiltinType::I32 => Self::I32Neg,
            BuiltinType::I64 => Self::I64Neg,
            BuiltinType::F32 => Self::F32Neg,
            BuiltinType::F64 => Self::F64Neg,
            _ => return None,
        })
    }

    pub fn conversion(source: BuiltinType, target: BuiltinType) -> Option<Self> {
        if let Some(source) = IntegerType::from_builtin(source) {
            return Some(if let Some(target) = IntegerType::from_builtin(target) {
                Self::IntegerConvert { source, target }
            } else {
                Self::IntegerToFloat {
                    source,
                    target: FloatType::from_builtin(target)?,
                }
            });
        }
        if let Some(source) = FloatType::from_builtin(source) {
            return Some(if let Some(target) = IntegerType::from_builtin(target) {
                Self::FloatToInteger { source, target }
            } else {
                Self::FloatConvert {
                    source,
                    target: FloatType::from_builtin(target)?,
                }
            });
        }
        (source == BuiltinType::Bool)
            .then(|| IntegerType::from_builtin(target).map(|target| Self::BoolToInteger { target }))
            .flatten()
    }
}
