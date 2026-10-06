//! Prepared scalar semantics over complete, sign-extended integer or IEEE bits.
//! Selection happens once; execution carries no tagged Value or source resolver.
use crate::{
    integer::{self, IntegerOp},
    numeric::{self, Number},
    scalar::BuiltinType,
};

pub type ScalarKernel = fn(u64, u64) -> Result<u64, &'static str>;

#[derive(Debug, Clone, Copy)]
pub enum ScalarBinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    Eq,
    NotEq,
    Lt,
    Le,
    Gt,
    Ge,
}

macro_rules! select_op {
    ($op:expr, $function:ident $(, $parameter:expr)*) => {
        match $op {
            ScalarBinaryOp::Add => $function::<0 $(, $parameter)*>,
            ScalarBinaryOp::Sub => $function::<1 $(, $parameter)*>,
            ScalarBinaryOp::Mul => $function::<2 $(, $parameter)*>,
            ScalarBinaryOp::Div => $function::<3 $(, $parameter)*>,
            ScalarBinaryOp::Rem => $function::<4 $(, $parameter)*>,
            ScalarBinaryOp::Eq => $function::<5 $(, $parameter)*>,
            ScalarBinaryOp::NotEq => $function::<6 $(, $parameter)*>,
            ScalarBinaryOp::Lt => $function::<7 $(, $parameter)*>,
            ScalarBinaryOp::Le => $function::<8 $(, $parameter)*>,
            ScalarBinaryOp::Gt => $function::<9 $(, $parameter)*>,
            ScalarBinaryOp::Ge => $function::<10 $(, $parameter)*>,
        }
    };
}

/// Physical arithmetic uses its full carrier domain. Source-width operations use
/// integer_kernel instead, including narrow signed/unsigned checked arithmetic.
pub fn binary_kernel(op: ScalarBinaryOp, input: BuiltinType) -> Option<ScalarKernel> {
    Some(match input {
        BuiltinType::I32 => select_op!(op, integer_binary, 32, true),
        BuiltinType::I64 | BuiltinType::ISize => select_op!(op, integer_binary, 64, true),
        BuiltinType::U64 | BuiltinType::USize => select_op!(op, integer_binary, 64, false),
        BuiltinType::F32 => select_op!(op, float_binary, true),
        BuiltinType::F64 => select_op!(op, float_binary, false),
        BuiltinType::Bool | BuiltinType::Unit
            if matches!(op, ScalarBinaryOp::Eq | ScalarBinaryOp::NotEq) =>
        {
            match op {
                ScalarBinaryOp::Eq => equality::<false>,
                ScalarBinaryOp::NotEq => equality::<true>,
                _ => unreachable!(),
            }
        }
        _ => return None,
    })
}

fn equality<const INVERT: bool>(lhs: u64, rhs: u64) -> Result<u64, &'static str> {
    Ok(u64::from((lhs == rhs) != INVERT))
}

fn integer_binary<const OP: u8, const BITS: u32, const SIGNED: bool>(
    lhs: u64,
    rhs: u64,
) -> Result<u64, &'static str> {
    if OP < 5 {
        return integer_value::<OP, BITS, SIGNED>(lhs, rhs);
    }
    let lhs = decode_integer::<BITS, SIGNED>(lhs)?;
    let rhs = decode_integer::<BITS, SIGNED>(rhs)?;
    Ok(u64::from(match OP {
        5 => lhs == rhs,
        6 => lhs != rhs,
        7 => lhs < rhs,
        8 => lhs <= rhs,
        9 => lhs > rhs,
        10 => lhs >= rhs,
        _ => unreachable!(),
    }))
}

fn float_binary<const OP: u8, const F32: bool>(lhs: u64, rhs: u64) -> Result<u64, &'static str> {
    macro_rules! evaluate {
        ($lhs:expr, $rhs:expr) => {{
            let lhs = $lhs;
            let rhs = $rhs;
            match OP {
                0 => (lhs + rhs).to_bits() as u64,
                1 => (lhs - rhs).to_bits() as u64,
                2 => (lhs * rhs).to_bits() as u64,
                3 => (lhs / rhs).to_bits() as u64,
                4 => (lhs % rhs).to_bits() as u64,
                5 => u64::from(lhs == rhs),
                6 => u64::from(lhs != rhs),
                7 => u64::from(lhs < rhs),
                8 => u64::from(lhs <= rhs),
                9 => u64::from(lhs > rhs),
                10 => u64::from(lhs >= rhs),
                _ => unreachable!(),
            }
        }};
    }
    Ok(if F32 {
        evaluate!(f32::from_bits(lhs as u32), f32::from_bits(rhs as u32))
    } else {
        evaluate!(f64::from_bits(lhs), f64::from_bits(rhs))
    })
}

fn decode_integer<const BITS: u32, const SIGNED: bool>(value: u64) -> Result<i128, &'static str> {
    let value = if SIGNED {
        i128::from(value as i64)
    } else {
        i128::from(value)
    };
    let (min, max) = integer::bounds(BITS, SIGNED);
    if value < min || value > max {
        return Err("invalid numeric operand type or range");
    }
    Ok(value)
}

macro_rules! integer_width {
    ($input:expr, $function:ident, $op:expr $(, $parameter:expr)*) => {
        match $input {
            BuiltinType::I8 => $function::<$op, 8, true $(, $parameter)*>,
            BuiltinType::I16 => $function::<$op, 16, true $(, $parameter)*>,
            BuiltinType::I32 => $function::<$op, 32, true $(, $parameter)*>,
            BuiltinType::I64 | BuiltinType::ISize => $function::<$op, 64, true $(, $parameter)*>,
            BuiltinType::U8 => $function::<$op, 8, false $(, $parameter)*>,
            BuiltinType::U16 => $function::<$op, 16, false $(, $parameter)*>,
            BuiltinType::U32 => $function::<$op, 32, false $(, $parameter)*>,
            BuiltinType::U64 | BuiltinType::USize => $function::<$op, 64, false $(, $parameter)*>,
            _ => return None,
        }
    };
}

pub fn integer_kernel(
    op: IntegerOp,
    input: BuiltinType,
    rhs: Option<BuiltinType>,
) -> Option<ScalarKernel> {
    if matches!(op, IntegerOp::Shl | IntegerOp::Shr) {
        let select: fn(BuiltinType) -> Option<ScalarKernel> = match op {
            IntegerOp::Shl => integer_width!(input, select_shift, 0),
            IntegerOp::Shr => integer_width!(input, select_shift, 1),
            _ => unreachable!(),
        };
        return select(rhs?);
    }
    Some(match op {
        IntegerOp::CheckedAdd => integer_width!(input, integer_value, 0),
        IntegerOp::CheckedSub => integer_width!(input, integer_value, 1),
        IntegerOp::CheckedMul => integer_width!(input, integer_value, 2),
        IntegerOp::CheckedDiv => integer_width!(input, integer_value, 3),
        IntegerOp::CheckedRem => integer_width!(input, integer_value, 4),
        IntegerOp::BitAnd => integer_width!(input, integer_value, 5),
        IntegerOp::BitOr => integer_width!(input, integer_value, 6),
        IntegerOp::BitXor => integer_width!(input, integer_value, 7),
        IntegerOp::BitNot => integer_width!(input, integer_value, 8),
        IntegerOp::Shl | IntegerOp::Shr => unreachable!(),
    })
}

fn integer_value<const OP: u8, const BITS: u32, const SIGNED: bool>(
    lhs: u64,
    rhs: u64,
) -> Result<u64, &'static str> {
    let lhs = decode_integer::<BITS, SIGNED>(lhs)?;
    let (op, rhs) = match OP {
        0 => (IntegerOp::CheckedAdd, decode_integer::<BITS, SIGNED>(rhs)?),
        1 => (IntegerOp::CheckedSub, decode_integer::<BITS, SIGNED>(rhs)?),
        2 => (IntegerOp::CheckedMul, decode_integer::<BITS, SIGNED>(rhs)?),
        3 => (IntegerOp::CheckedDiv, decode_integer::<BITS, SIGNED>(rhs)?),
        4 => (IntegerOp::CheckedRem, decode_integer::<BITS, SIGNED>(rhs)?),
        5 => (IntegerOp::BitAnd, decode_integer::<BITS, SIGNED>(rhs)?),
        6 => (IntegerOp::BitOr, decode_integer::<BITS, SIGNED>(rhs)?),
        7 => (IntegerOp::BitXor, decode_integer::<BITS, SIGNED>(rhs)?),
        8 => (IntegerOp::BitNot, 0),
        _ => unreachable!(),
    };
    integer::integer_operation(op, lhs, rhs, BITS, SIGNED).map(|value| value as u64)
}

fn select_shift<const RIGHT: u8, const BITS: u32, const SIGNED: bool>(
    rhs: BuiltinType,
) -> Option<ScalarKernel> {
    Some(integer_width!(rhs, shift_value, RIGHT, BITS, SIGNED))
}

fn shift_value<
    const RIGHT: u8,
    const RBITS: u32,
    const RSIGNED: bool,
    const BITS: u32,
    const SIGNED: bool,
>(
    lhs: u64,
    rhs: u64,
) -> Result<u64, &'static str> {
    let lhs = decode_integer::<BITS, SIGNED>(lhs)?;
    let rhs = decode_integer::<RBITS, RSIGNED>(rhs)?;
    let op = if RIGHT == 0 {
        IntegerOp::Shl
    } else {
        IntegerOp::Shr
    };
    integer::integer_operation(op, lhs, rhs, BITS, SIGNED).map(|value| value as u64)
}

pub fn negation_kernel(input: BuiltinType) -> Option<ScalarKernel> {
    Some(match input {
        BuiltinType::I32 => negate_integer::<32>,
        BuiltinType::I64 | BuiltinType::ISize => negate_integer::<64>,
        BuiltinType::F32 => negate_float::<true>,
        BuiltinType::F64 => negate_float::<false>,
        _ => return None,
    })
}

fn negate_integer<const BITS: u32>(value: u64, _: u64) -> Result<u64, &'static str> {
    integer_value::<1, BITS, true>(0, value)
}

fn negate_float<const F32: bool>(value: u64, _: u64) -> Result<u64, &'static str> {
    Ok(value ^ if F32 { 1 << 31 } else { 1 << 63 })
}

pub fn boolean_not(value: u64, _: u64) -> Result<u64, &'static str> {
    Ok(u64::from(value == 0))
}

macro_rules! numeric_type {
    ($ty:expr, $function:ident $(, $parameter:expr)*) => {
        match $ty {
            BuiltinType::Bool => $function::<{ BuiltinType::Bool as u8 } $(, $parameter)*>,
            BuiltinType::I8 => $function::<{ BuiltinType::I8 as u8 } $(, $parameter)*>,
            BuiltinType::I16 => $function::<{ BuiltinType::I16 as u8 } $(, $parameter)*>,
            BuiltinType::I32 => $function::<{ BuiltinType::I32 as u8 } $(, $parameter)*>,
            BuiltinType::I64 => $function::<{ BuiltinType::I64 as u8 } $(, $parameter)*>,
            BuiltinType::ISize => $function::<{ BuiltinType::ISize as u8 } $(, $parameter)*>,
            BuiltinType::U8 => $function::<{ BuiltinType::U8 as u8 } $(, $parameter)*>,
            BuiltinType::U16 => $function::<{ BuiltinType::U16 as u8 } $(, $parameter)*>,
            BuiltinType::U32 => $function::<{ BuiltinType::U32 as u8 } $(, $parameter)*>,
            BuiltinType::U64 => $function::<{ BuiltinType::U64 as u8 } $(, $parameter)*>,
            BuiltinType::USize => $function::<{ BuiltinType::USize as u8 } $(, $parameter)*>,
            BuiltinType::F32 => $function::<{ BuiltinType::F32 as u8 } $(, $parameter)*>,
            BuiltinType::F64 => $function::<{ BuiltinType::F64 as u8 } $(, $parameter)*>,
            _ => return None,
        }
    };
}

pub fn conversion_kernel(source: BuiltinType, target: BuiltinType) -> Option<ScalarKernel> {
    if !source.can_cast_to(target) {
        return None;
    }
    let select: fn(BuiltinType) -> Option<ScalarKernel> = numeric_type!(source, select_target);
    select(target)
}

fn select_target<const SOURCE: u8>(target: BuiltinType) -> Option<ScalarKernel> {
    Some(numeric_type!(target, convert, SOURCE))
}

fn builtin<const CODE: u8>() -> BuiltinType {
    // Exhaustive identity table is shared by the generated source/target kernels;
    // constant codes are eliminated in each concrete monomorphization.
    const TYPES: [BuiltinType; 16] = [
        BuiltinType::Never,
        BuiltinType::Unit,
        BuiltinType::Bool,
        BuiltinType::I8,
        BuiltinType::I16,
        BuiltinType::I32,
        BuiltinType::I64,
        BuiltinType::ISize,
        BuiltinType::U8,
        BuiltinType::U16,
        BuiltinType::U32,
        BuiltinType::U64,
        BuiltinType::USize,
        BuiltinType::F32,
        BuiltinType::F64,
        BuiltinType::String,
    ];
    TYPES[usize::from(CODE)]
}

fn convert<const TARGET: u8, const SOURCE: u8>(value: u64, _: u64) -> Result<u64, &'static str> {
    let source = builtin::<SOURCE>();
    let number = match source {
        BuiltinType::F32 => Number::F32(f32::from_bits(value as u32)),
        BuiltinType::F64 => Number::F64(f64::from_bits(value)),
        _ => {
            let input = if source.integer_layout().is_some_and(|(_, signed)| signed) {
                i128::from(value as i64)
            } else {
                i128::from(value)
            };
            if let Some((min, max)) = source.integer_bounds()
                && (input < min || input > max)
            {
                return Err("invalid numeric operand type or range");
            }
            Number::Integer(input)
        }
    };
    if SOURCE == TARGET {
        return Ok(value);
    }
    Ok(
        match numeric::cast(
            number,
            builtin::<TARGET>().number_type().expect("numeric target"),
        ) {
            Number::Integer(v) => v as u64,
            Number::F32(v) => u64::from(v.to_bits()),
            Number::F64(v) => v.to_bits(),
        },
    )
}

#[cfg(test)]
mod tests;
