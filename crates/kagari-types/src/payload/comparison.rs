//! Comparison conditions independent of scalar representation and operand shape.
//! Known-domain entrypoints expand into prepared handlers, keeping their checked
//! result out of a separate call ABI. Dynamic domain routing stays out of line.
use crate::payload::decode_integer;

/// Ordered conditions preserve unordered floating behavior without inversion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Comparison {
    Eq,
    NotEq,
    Lt,
    Le,
    Gt,
    Ge,
}

impl Comparison {
    #[inline(always)]
    fn test<T: PartialOrd>(self, lhs: T, rhs: T) -> u64 {
        u64::from(match self {
            Self::Eq => lhs == rhs,
            Self::NotEq => lhs != rhs,
            Self::Lt => lhs < rhs,
            Self::Le => lhs <= rhs,
            Self::Gt => lhs > rhs,
            Self::Ge => lhs >= rhs,
        })
    }
}

/// Physical comparison domains established by canonical lowering.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComparisonDomain {
    I32,
    I64,
    U64,
    F32,
    F64,
    Char,
    Scalar,
}

impl ComparisonDomain {
    /// Evaluate a shared comparison after both operands have been fetched.
    #[inline(never)]
    pub fn evaluate(self, condition: Comparison, lhs: u64, rhs: u64) -> Result<u64, &'static str> {
        match self {
            Self::I32 => i32_compare(lhs, rhs, condition),
            Self::I64 => i64_compare(lhs, rhs, condition),
            Self::U64 => u64_compare(lhs, rhs, condition),
            Self::F32 => f32_compare(lhs, rhs, condition),
            Self::F64 => f64_compare(lhs, rhs, condition),
            Self::Char => char_compare(lhs, rhs, condition),
            Self::Scalar => scalar_compare(lhs, rhs, condition),
        }
    }
}

/// Compare complete I32 carriers with the selected condition.
#[inline(always)]
pub fn i32_compare(lhs: u64, rhs: u64, condition: Comparison) -> Result<u64, &'static str> {
    let lhs = decode_integer::<32, true>(lhs)?;
    let rhs = decode_integer::<32, true>(rhs)?;
    Ok(condition.test(lhs, rhs))
}

/// Compare complete I64 carriers with the selected condition.
#[inline(always)]
pub fn i64_compare(lhs: u64, rhs: u64, condition: Comparison) -> Result<u64, &'static str> {
    let lhs = decode_integer::<64, true>(lhs)?;
    let rhs = decode_integer::<64, true>(rhs)?;
    Ok(condition.test(lhs, rhs))
}

/// Compare complete U64 carriers with the selected condition.
#[inline(always)]
pub fn u64_compare(lhs: u64, rhs: u64, condition: Comparison) -> Result<u64, &'static str> {
    let lhs = decode_integer::<64, false>(lhs)?;
    let rhs = decode_integer::<64, false>(rhs)?;
    Ok(condition.test(lhs, rhs))
}

/// Compare complete F32 carriers with the selected condition.
#[inline(always)]
pub fn f32_compare(lhs: u64, rhs: u64, condition: Comparison) -> Result<u64, &'static str> {
    let lhs = f32::from_bits(lhs as u32);
    let rhs = f32::from_bits(rhs as u32);
    Ok(condition.test(lhs, rhs))
}

/// Compare complete F64 carriers with the selected condition.
#[inline(always)]
pub fn f64_compare(lhs: u64, rhs: u64, condition: Comparison) -> Result<u64, &'static str> {
    let lhs = f64::from_bits(lhs);
    let rhs = f64::from_bits(rhs);
    Ok(condition.test(lhs, rhs))
}

/// Compare complete Char carriers with the selected condition.
#[inline(always)]
pub fn char_compare(lhs: u64, rhs: u64, condition: Comparison) -> Result<u64, &'static str> {
    for value in [lhs, rhs] {
        if u32::try_from(value).ok().and_then(char::from_u32).is_none() {
            return Err("invalid Unicode scalar operand");
        }
    }
    Ok(condition.test(lhs, rhs))
}

/// Compare complete Scalar carriers with the selected condition.
#[inline(always)]
pub fn scalar_compare(lhs: u64, rhs: u64, condition: Comparison) -> Result<u64, &'static str> {
    Ok(condition.test(lhs, rhs))
}
