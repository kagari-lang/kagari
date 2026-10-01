//! Real Rust integer methods, with explicit script widths independent of storage.
macro_rules! numeric_module {
    ($(($name:ident, $rust:ty, $abi:ident, $script:literal, $wire:ident)),*;
     $(($unsigned:ident, $signed:ident)),*) => {
        #[kagari_native_macros::native_module("std::numeric", runtime = crate)]
        pub mod numeric {
            use crate::{
                error::RuntimeError,
                native_module::types::TypeExpression,
                native_value::{NativeCall, NativeResult, NativeValue, parse::ParseFailure, result::NativeResultValue},
                value::Value,
            };
            use kagari_abi::{scalar::BuiltinType, types::AbiType};

            $(
                /// Rust's carrier is private; its metadata names the real script scalar.
                #[derive(Clone, Copy)]
                pub struct $name(pub(crate) $rust);
                impl NativeValue for $name {
                    fn type_expression(_: &[&'static str]) -> TypeExpression {
                        TypeExpression::Named { path: vec![$script], arguments: vec![], bindings: vec![] }
                    }
                    fn read(call: &NativeCall, value: Value, expected: &AbiType) -> NativeResult<Self> {
                        if *expected != AbiType::Builtin(BuiltinType::$abi) {
                            return Err(RuntimeError::module_validation("numeric scalar type mismatch"));
                        }
                        call.check(&value, expected)?;
                        let Value::$wire(value) = value else {
                            return Err(RuntimeError::module_validation("numeric scalar storage mismatch"));
                        };
                        Ok(Self(<$rust>::try_from(value).map_err(|_| RuntimeError::module_validation("numeric scalar width mismatch"))?))
                    }
                    fn write(self, call: &NativeCall, expected: &AbiType) -> NativeResult<Value> {
                        if *expected != AbiType::Builtin(BuiltinType::$abi) {
                            return Err(RuntimeError::module_validation("numeric scalar type mismatch"));
                        }
                        let value = Value::$wire(self.0.into());
                        call.check(&value, expected)?;
                        call.retain(value)
                    }
                }
                #[native_impl]
                impl $name {
                    /// Parse a complete string in radix 2..=36, without trimming.
                    pub fn from_str_radix(#[context] call: &NativeCall, text: String, radix: u32)
                        -> NativeResult<NativeResultValue<Self, ParseFailure>> {
                        call.charge_work(text.len() as u64)?;
                        let parsed = if (2..=36).contains(&radix) {
                            <$rust>::from_str_radix(&text, radix).map(Self).map_err(ParseFailure::integer)
                        } else { Err(ParseFailure::InvalidRadix) };
                        NativeResultValue::from_result(call, parsed)
                    }
                    /// Wrap at the receiver width.
                    pub fn wrapping_add(&self, rhs: Self) -> Self { Self(self.0.wrapping_add(rhs.0)) }
                    /// Wrap at the receiver width.
                    pub fn wrapping_sub(&self, rhs: Self) -> Self { Self(self.0.wrapping_sub(rhs.0)) }
                    /// Wrap at the receiver width.
                    pub fn wrapping_mul(&self, rhs: Self) -> Self { Self(self.0.wrapping_mul(rhs.0)) }
                    /// Return None on overflow.
                    pub fn checked_add(&self, rhs: Self) -> Option<Self> { self.0.checked_add(rhs.0).map(Self) }
                    /// Return None on overflow.
                    pub fn checked_sub(&self, rhs: Self) -> Option<Self> { self.0.checked_sub(rhs.0).map(Self) }
                    /// Return None on overflow.
                    pub fn checked_mul(&self, rhs: Self) -> Option<Self> { self.0.checked_mul(rhs.0).map(Self) }
                    /// Return None on zero divisor or signed MIN / -1 overflow.
                    pub fn checked_div(&self, rhs: Self) -> Option<Self> { self.0.checked_div(rhs.0).map(Self) }
                    /// Return None on zero divisor or signed MIN % -1 overflow.
                    pub fn checked_rem(&self, rhs: Self) -> Option<Self> { self.0.checked_rem(rhs.0).map(Self) }
                    /// Return the wrapped result and signed overflow or unsigned carry.
                    pub fn overflowing_add(&self, rhs: Self) -> (Self, bool) {
                        let (value, overflow) = self.0.overflowing_add(rhs.0); (Self(value), overflow)
                    }
                    /// Return the wrapped result and signed overflow or unsigned borrow.
                    pub fn overflowing_sub(&self, rhs: Self) -> (Self, bool) {
                        let (value, overflow) = self.0.overflowing_sub(rhs.0); (Self(value), overflow)
                    }
                    /// Return the wrapped result and the receiver's overflow flag.
                    pub fn overflowing_mul(&self, rhs: Self) -> (Self, bool) {
                        let (value, overflow) = self.0.overflowing_mul(rhs.0); (Self(value), overflow)
                    }
                    /// Clamp an overflowing sum to the receiver's bounds.
                    pub fn saturating_add(&self, rhs: Self) -> Self { Self(self.0.saturating_add(rhs.0)) }
                    /// Clamp an overflowing difference to the receiver's bounds.
                    pub fn saturating_sub(&self, rhs: Self) -> Self { Self(self.0.saturating_sub(rhs.0)) }
                    /// Clamp an overflowing product to the receiver's bounds.
                    pub fn saturating_mul(&self, rhs: Self) -> Self { Self(self.0.saturating_mul(rhs.0)) }
                    /// Rotate low-width bits, reducing the count modulo the width.
                    pub fn rotate_left(&self, rhs: u32) -> Self { Self(self.0.rotate_left(rhs)) }
                    /// Rotate low-width bits, reducing the count modulo the width.
                    pub fn rotate_right(&self, rhs: u32) -> Self { Self(self.0.rotate_right(rhs)) }
                }
            )*
            $(
                #[native_impl]
                impl $unsigned {
                    /// Add the corresponding signed offset, wrapping at this width.
                    pub fn wrapping_add_signed(&self, rhs: $signed) -> Self {
                        Self(self.0.wrapping_add_signed(rhs.0))
                    }
                }
            )*
        }
    };
}

numeric_module! {
    (I8, i8, I8, "i8", I32), (I16, i16, I16, "i16", I32),
    (I32, i32, I32, "i32", I32), (I64, i64, I64, "i64", I64),
    (ISize, i64, ISize, "isize", I64),
    (U8, u8, U8, "u8", I64), (U16, u16, U16, "u16", I64),
    (U32, u32, U32, "u32", I64), (U64, u64, U64, "u64", U64),
    (USize, u64, USize, "usize", U64);
    (U8, I8), (U16, I16), (U32, I32), (U64, I64), (USize, ISize)
}
