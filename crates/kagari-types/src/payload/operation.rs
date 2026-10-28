//! Admitted scalar operations with independent domains kept as data.
use crate::{
    integer::IntegerOp,
    payload::{
        self,
        comparison::{Comparison, ComparisonDomain},
        shared,
    },
    scalar::{BuiltinType, IntegerType},
};

mod select;

/// Floating representation selected before execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FloatType {
    F32,
    F64,
}

impl FloatType {
    pub fn from_builtin(ty: BuiltinType) -> Option<Self> {
        match ty {
            BuiltinType::F32 => Some(Self::F32),
            BuiltinType::F64 => Some(Self::F64),
            _ => None,
        }
    }

    pub fn builtin_type(self) -> BuiltinType {
        match self {
            Self::F32 => BuiltinType::F32,
            Self::F64 => BuiltinType::F64,
        }
    }
}

/// Arithmetic operations supported by both floating representations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FloatOperation {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
}

/// Left shift domain and direction; the count has an independent integer domain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShiftKind {
    I8Shl,
    I8Shr,
    I16Shl,
    I16Shr,
    I32Shl,
    I32Shr,
    I64Shl,
    I64Shr,
    U8Shl,
    U8Shr,
    U16Shl,
    U16Shr,
    U32Shl,
    U32Shr,
    U64Shl,
    U64Shr,
}

impl ShiftKind {
    pub fn new(domain: IntegerType, op: IntegerOp) -> Option<Self> {
        Some(match (domain, op) {
            (IntegerType::I8, IntegerOp::Shl) => Self::I8Shl,
            (IntegerType::I8, IntegerOp::Shr) => Self::I8Shr,
            (IntegerType::I16, IntegerOp::Shl) => Self::I16Shl,
            (IntegerType::I16, IntegerOp::Shr) => Self::I16Shr,
            (IntegerType::I32, IntegerOp::Shl) => Self::I32Shl,
            (IntegerType::I32, IntegerOp::Shr) => Self::I32Shr,
            (IntegerType::I64, IntegerOp::Shl) => Self::I64Shl,
            (IntegerType::I64, IntegerOp::Shr) => Self::I64Shr,
            (IntegerType::U8, IntegerOp::Shl) => Self::U8Shl,
            (IntegerType::U8, IntegerOp::Shr) => Self::U8Shr,
            (IntegerType::U16, IntegerOp::Shl) => Self::U16Shl,
            (IntegerType::U16, IntegerOp::Shr) => Self::U16Shr,
            (IntegerType::U32, IntegerOp::Shl) => Self::U32Shl,
            (IntegerType::U32, IntegerOp::Shr) => Self::U32Shr,
            (IntegerType::U64, IntegerOp::Shl) => Self::U64Shl,
            (IntegerType::U64, IntegerOp::Shr) => Self::U64Shr,
            _ => return None,
        })
    }

    pub fn parts(self) -> (IntegerType, IntegerOp) {
        match self {
            Self::I8Shl => (IntegerType::I8, IntegerOp::Shl),
            Self::I8Shr => (IntegerType::I8, IntegerOp::Shr),
            Self::I16Shl => (IntegerType::I16, IntegerOp::Shl),
            Self::I16Shr => (IntegerType::I16, IntegerOp::Shr),
            Self::I32Shl => (IntegerType::I32, IntegerOp::Shl),
            Self::I32Shr => (IntegerType::I32, IntegerOp::Shr),
            Self::I64Shl => (IntegerType::I64, IntegerOp::Shl),
            Self::I64Shr => (IntegerType::I64, IntegerOp::Shr),
            Self::U8Shl => (IntegerType::U8, IntegerOp::Shl),
            Self::U8Shr => (IntegerType::U8, IntegerOp::Shr),
            Self::U16Shl => (IntegerType::U16, IntegerOp::Shl),
            Self::U16Shr => (IntegerType::U16, IntegerOp::Shr),
            Self::U32Shl => (IntegerType::U32, IntegerOp::Shl),
            Self::U32Shr => (IntegerType::U32, IntegerOp::Shr),
            Self::U64Shl => (IntegerType::U64, IntegerOp::Shl),
            Self::U64Shr => (IntegerType::U64, IntegerOp::Shr),
        }
    }
}

/// Scalar semantics selected from verified executable contracts.
/// Shape and physical locations belong to runtime preparation, not this owner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScalarOperation {
    Integer {
        domain: IntegerType,
        op: IntegerOp,
    },
    Float {
        domain: FloatType,
        op: FloatOperation,
    },
    Shift {
        kind: ShiftKind,
        count_domain: IntegerType,
    },
    Compare {
        domain: ComparisonDomain,
        condition: Comparison,
    },
    IntegerConvert {
        source: IntegerType,
        target: IntegerType,
    },
    IntegerToFloat {
        source: IntegerType,
        target: FloatType,
    },
    FloatToInteger {
        source: FloatType,
        target: IntegerType,
    },
    FloatConvert {
        source: FloatType,
        target: FloatType,
    },
    BoolToInteger {
        target: IntegerType,
    },
    I8BitNot,
    I16BitNot,
    I32BitNot,
    I64BitNot,
    U8BitNot,
    U16BitNot,
    U32BitNot,
    U64BitNot,
    I32Neg,
    I64Neg,
    F32Neg,
    F64Neg,
    BoolNot,
}

impl ScalarOperation {
    /// Evaluate complete carriers using the same semantics as prepared handlers.
    /// Domain failures precede arithmetic; no Value allocation or function pointer.
    #[inline(never)]
    pub fn evaluate(self, lhs: u64, rhs: u64) -> Result<u64, &'static str> {
        match self {
            Self::Integer { domain, op } => shared::integer(op, domain, lhs, rhs),
            Self::Float { domain, op } => shared::float(op, domain, lhs, rhs),
            Self::Shift { kind, count_domain } => shared::shift(kind, count_domain, lhs, rhs),
            Self::Compare { domain, condition } => domain.evaluate(condition, lhs, rhs),
            Self::IntegerConvert { source, target } => {
                payload::convert(lhs, source.builtin_type(), target.builtin_type())
            }
            Self::IntegerToFloat { source, target } => {
                payload::convert(lhs, source.builtin_type(), target.builtin_type())
            }
            Self::FloatToInteger { source, target } => {
                payload::convert(lhs, source.builtin_type(), target.builtin_type())
            }
            Self::FloatConvert { source, target } => {
                payload::convert(lhs, source.builtin_type(), target.builtin_type())
            }
            Self::BoolToInteger { target } => {
                payload::convert(lhs, BuiltinType::Bool, target.builtin_type())
            }
            Self::I8BitNot => payload::integer_value::<8, 8, true>(lhs, rhs),
            Self::I16BitNot => payload::integer_value::<8, 16, true>(lhs, rhs),
            Self::I32BitNot => payload::integer_value::<8, 32, true>(lhs, rhs),
            Self::I64BitNot => payload::integer_value::<8, 64, true>(lhs, rhs),
            Self::U8BitNot => payload::integer_value::<8, 8, false>(lhs, rhs),
            Self::U16BitNot => payload::integer_value::<8, 16, false>(lhs, rhs),
            Self::U32BitNot => payload::integer_value::<8, 32, false>(lhs, rhs),
            Self::U64BitNot => payload::integer_value::<8, 64, false>(lhs, rhs),
            Self::I32Neg => payload::negate_integer::<32>(lhs, rhs),
            Self::I64Neg => payload::negate_integer::<64>(lhs, rhs),
            Self::F32Neg => payload::negate_float::<true>(lhs, rhs),
            Self::F64Neg => payload::negate_float::<false>(lhs, rhs),
            Self::BoolNot => payload::boolean_not(lhs, rhs),
        }
    }
}
