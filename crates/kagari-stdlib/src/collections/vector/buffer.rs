//! Finite specialization of script scalar layouts, selected from the declared type.
//! No inference from the first element, no per-element GC roots for primitive data.

use kagari_runtime::{
    error::{RuntimeError, RuntimeErrorKind},
    native::{binding::NativeResult, scalar::NativeScalar},
    value::Value,
};
use kagari_types::{scalar::BuiltinType, ty::Ty};
use std::{collections::TryReserveError, mem::size_of};

#[derive(Debug)]
pub(crate) enum SequenceStorage {
    Unit(Vec<()>),
    Bool(Vec<bool>),
    Char(Vec<char>),
    I8(Vec<i8>),
    I16(Vec<i16>),
    I32(Vec<i32>),
    I64(Vec<i64>),
    U8(Vec<u8>),
    U16(Vec<u16>),
    U32(Vec<u32>),
    U64(Vec<u64>),
    F32(Vec<f32>),
    F64(Vec<f64>),
    Traced(Vec<Value>),
}

macro_rules! storage {
    ($($variant:ident:$ty:ty),+) => {
        impl SequenceStorage {
            pub(crate) fn empty(element: &Ty) -> Self {
                match element {
                    $(Ty::Builtin(BuiltinType::$variant) => Self::$variant(Vec::new()),)+
                    _ => Self::Traced(Vec::new()),
                }
            }

            pub(crate) fn empty_like(&self) -> Self {
                match self { $(Self::$variant(_) => Self::$variant(Vec::new()),)+ Self::Traced(_) => Self::Traced(Vec::new()) }
            }

            pub(crate) fn len(&self) -> usize {
                match self { $(Self::$variant(values) => values.len(),)+ Self::Traced(values) => values.len() }
            }

            pub(crate) fn retained_bytes(&self) -> usize {
                match self {
                    $(Self::$variant(values) => values.capacity().checked_mul(size_of::<$ty>()).expect("allocated sequence capacity"),)+
                    Self::Traced(values) => values.capacity().checked_mul(size_of::<Value>()).expect("allocated sequence capacity"),
                }
            }

            pub(crate) fn try_reserve(&mut self, additional: usize) -> Result<(), TryReserveError> {
                match self { $(Self::$variant(values) => values.try_reserve(additional),)+ Self::Traced(values) => values.try_reserve(additional) }
            }

            pub(crate) fn get(&self, index: usize) -> Option<Value> {
                match self { $(Self::$variant(values) => values.get(index).copied().map(NativeScalar::encode),)+ Self::Traced(values) => values.get(index).cloned() }
            }

            pub(crate) fn push(&mut self, value: Value) -> NativeResult<()> {
                match self { $(Self::$variant(values) => values.push(<$ty>::decode(value)?),)+ Self::Traced(values) => values.push(value) }
                Ok(())
            }

            pub(crate) fn insert(&mut self, index: usize, value: Value) -> NativeResult<()> {
                if index > self.len() { return Err(invalid_index()); }
                match self { $(Self::$variant(values) => values.insert(index, <$ty>::decode(value)?),)+ Self::Traced(values) => values.insert(index, value) }
                Ok(())
            }

            pub(crate) fn set(&mut self, index: usize, value: Value) -> NativeResult<()> {
                if index >= self.len() { return Err(invalid_index()); }
                match self { $(Self::$variant(values) => values[index] = <$ty>::decode(value)?,)+ Self::Traced(values) => values[index] = value }
                Ok(())
            }

            pub(crate) fn remove(&mut self, index: usize) -> Option<Value> {
                if index >= self.len() { return None; }
                Some(match self { $(Self::$variant(values) => values.remove(index).encode(),)+ Self::Traced(values) => values.remove(index) })
            }

            pub(crate) fn clear(&mut self) { match self { $(Self::$variant(values) => values.clear(),)+ Self::Traced(values) => values.clear() } }

            pub(crate) fn truncate(&mut self, length: usize) { match self { $(Self::$variant(values) => values.truncate(length),)+ Self::Traced(values) => values.truncate(length) } }

            pub(crate) fn swap(&mut self, left: usize, right: usize) -> NativeResult<()> {
                if left >= self.len() || right >= self.len() { return Err(invalid_index()); }
                match self { $(Self::$variant(values) => values.swap(left, right),)+ Self::Traced(values) => values.swap(left, right) }
                Ok(())
            }
        }
    };
}

storage!(Unit:(), Bool:bool, Char:char, I8:i8, I16:i16, I32:i32, I64:i64,
    U8:u8, U16:u16, U32:u32, U64:u64, F32:f32, F64:f64);

fn invalid_index() -> RuntimeError {
    RuntimeError::new(
        RuntimeErrorKind::IndexOutOfBounds,
        "sequence index is out of bounds",
    )
}

impl SequenceStorage {
    pub(crate) fn traced(&self) -> &[Value] {
        match self {
            Self::Traced(values) => values,
            _ => &[],
        }
    }
}
