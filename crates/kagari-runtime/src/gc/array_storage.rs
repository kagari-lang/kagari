//! Finite specialization of script scalar layouts, selected from the declared type.
//! No inference from the first element, no per-element GC roots for primitive data.

use crate::{
    error::{RuntimeError, RuntimeErrorKind},
    native::{
        binding::NativeResult,
        scalar::NativeScalar,
        storage::{
            NativePayload,
            scan::{NativeScanKind, ScanBudget, ScanStatus},
            value::StoredValue,
        },
        storage_type::StorageType,
    },
    resource::HeapCharge,
    value::Value,
};
use kagari_types::{scalar::BuiltinType, ty::Ty};
use std::mem::size_of;
use std::{collections::TryReserveError, sync::Arc};

#[derive(Debug)]
pub(crate) enum ArrayStorage {
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
    Traced(Vec<StoredValue>),
}

macro_rules! storage {
    ($($variant:ident:$ty:ty),+) => {
        impl ArrayStorage {
            pub(crate) fn empty(element: &Ty) -> Self {
                match element {
                    $(Ty::Builtin(BuiltinType::$variant) => Self::$variant(Vec::new()),)+
                    _ => Self::Traced(Vec::new()),
                }
            }

            pub(crate) fn len(&self) -> usize {
                match self { $(Self::$variant(values) => values.len(),)+ Self::Traced(values) => values.len() }
            }

            #[cfg(test)]
            pub(crate) fn capacity(&self) -> usize {
                match self { $(Self::$variant(values) => values.capacity(),)+ Self::Traced(values) => values.capacity() }
            }

            pub(crate) fn retained_bytes(&self) -> usize {
                match self {
                    $(Self::$variant(values) => values.capacity().checked_mul(size_of::<$ty>()).expect("allocated array capacity"),)+
                    Self::Traced(values) => values.capacity().checked_mul(size_of::<Value>()).expect("allocated array capacity"),
                }
            }

            #[cfg(feature = "execution-diagnostics")]
            pub(crate) fn backing_bytes(&self) -> (usize, usize) {
                match self {
                    $(Self::$variant(values) => (values.len() * size_of::<$ty>(), values.capacity() * size_of::<$ty>()),)+
                    Self::Traced(values) => (values.len() * size_of::<Value>(), values.capacity() * size_of::<Value>()),
                }
            }

            pub(crate) fn try_reserve(&mut self, additional: usize) -> Result<(), TryReserveError> {
                match self { $(Self::$variant(values) => values.try_reserve(additional),)+ Self::Traced(values) => values.try_reserve(additional) }
            }

            pub(crate) fn get(&self, index: usize) -> Option<Value> {
                match self { $(Self::$variant(values) => values.get(index).copied().map(NativeScalar::encode),)+ Self::Traced(values) => values.get(index).map(StoredValue::value) }
            }

            pub(crate) fn push(&mut self, value: Value) -> NativeResult<()> {
                match self { $(Self::$variant(values) => values.push(<$ty>::decode(value)?),)+ Self::Traced(values) => values.push(StoredValue::new(value)) }
                Ok(())
            }

            pub(crate) fn append_repeated(&mut self, value: Value, count: usize) -> NativeResult<()> {
                let length = self.len().checked_add(count).ok_or_else(|| RuntimeError::resource_limit("array length"))?;
                match self { $(Self::$variant(values) => values.resize(length, <$ty>::decode(value)?),)+ Self::Traced(values) => values.resize(length, StoredValue::new(value)) }
                Ok(())
            }

            pub(crate) fn copy_range(&self, start: usize, end: usize) -> NativeResult<Self> {
                if start > end || end > self.len() { return Err(invalid_index()); }
                Ok(match self { $(Self::$variant(values) => Self::$variant(copy_values(&values[start..end])?),)+ Self::Traced(values) => Self::Traced(copy_values(&values[start..end])?) })
            }





            pub(crate) fn overwrite(&mut self, start: usize, source: Self) -> NativeResult<()> {
                let end = start.checked_add(source.len()).ok_or_else(invalid_index)?;
                if end > self.len() { return Err(invalid_index()); }
                match (self, source) {
                    $((Self::$variant(values), Self::$variant(source)) => values[start..end].copy_from_slice(&source),)+
                    (Self::Traced(values), Self::Traced(source)) => { for (slot, value) in values[start..end].iter_mut().zip(source) { *slot = value; } }
                    _ => return Err(RuntimeError::module_validation("array copy scalar layout")),
                }
                Ok(())
            }



            pub(crate) fn set(&mut self, index: usize, value: Value) -> NativeResult<()> {
                if index >= self.len() { return Err(invalid_index()); }
                match self { $(Self::$variant(values) => values[index] = <$ty>::decode(value)?,)+ Self::Traced(values) => values[index] = StoredValue::new(value) }
                Ok(())
            }














        }
    };
}

storage!(Unit:(), Bool:bool, Char:char, I8:i8, I16:i16, I32:i32, I64:i64,
    U8:u8, U16:u16, U32:u32, U64:u64, F32:f32, F64:f64);

fn copy_values<T: Clone>(values: &[T]) -> NativeResult<Vec<T>> {
    copy_segments(values, &[])
}

fn copy_segments<T: Clone>(left: &[T], right: &[T]) -> NativeResult<Vec<T>> {
    let mut copy = Vec::new();
    let length = left
        .len()
        .checked_add(right.len())
        .ok_or_else(|| RuntimeError::resource_limit("array copy length"))?;
    copy.try_reserve_exact(length)
        .map_err(|_| RuntimeError::resource_limit("array copy capacity"))?;
    copy.extend_from_slice(left);
    copy.extend_from_slice(right);
    Ok(copy)
}

fn invalid_index() -> RuntimeError {
    RuntimeError::new(
        RuntimeErrorKind::IndexOutOfBounds,
        "array index is out of bounds",
    )
}

impl ArrayStorage {
    pub(crate) fn traced(&self) -> &[StoredValue] {
        match self {
            Self::Traced(values) => values,
            _ => &[],
        }
    }

    pub(crate) fn snapshot(&self) -> Vec<Value> {
        (0..self.len())
            .map(|index| self.get(index).expect("bounded storage index"))
            .collect()
    }
}

#[derive(Debug)]
pub(crate) struct ArrayPayload {
    pub(crate) contract: Arc<StorageType>,
    pub(crate) values: ArrayStorage,
}

unsafe impl NativePayload for ArrayPayload {
    type ScanState = (usize, usize);
    const SCAN_KIND: NativeScanKind = NativeScanKind::Resumable;

    fn begin_scan(&self) -> Self::ScanState {
        (0, self.values.traced().len())
    }

    fn scan<'payload>(
        &'payload self,
        state: &mut Self::ScanState,
        budget: &mut ScanBudget,
        visit: &mut dyn FnMut(&'payload StoredValue),
    ) -> ScanStatus {
        let values = self.values.traced();
        while state.0 < state.1 {
            if !budget.take(1) {
                return ScanStatus::Pending;
            }
            if let Some(value) = values.get(state.0) {
                visit(value);
            }
            state.0 += 1;
        }
        ScanStatus::Complete
    }

    fn charge(&self) -> HeapCharge {
        HeapCharge::new(self.values.len(), self.values.retained_bytes())
    }
}
