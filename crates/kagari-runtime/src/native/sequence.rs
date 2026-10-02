//! Finite specialization of script scalar layouts, selected from the declared type.
//! No inference from the first element, no per-element GC roots for primitive data.
use crate::{
    error::{RuntimeError, RuntimeErrorKind},
    native::{
        binding::NativeResult,
        scalar::NativeScalar,
        storage::{NativePayload, NativeStorage},
        storage_type::StorageType,
    },
    value::Value,
};
use kagari_abi::{
    scalar::BuiltinType,
    types::{AbiType, native::NativeStorageLayout},
};
use std::{any::Any, collections::TryReserveError, rc::Rc};

#[derive(Debug)]
pub(crate) enum SequenceStorage {
    Unit(Vec<()>),
    Bool(Vec<bool>),
    I8(Vec<i8>),
    I16(Vec<i16>),
    I32(Vec<i32>),
    I64(Vec<i64>),
    ISize(Vec<isize>),
    U8(Vec<u8>),
    U16(Vec<u16>),
    U32(Vec<u32>),
    U64(Vec<u64>),
    USize(Vec<usize>),
    F32(Vec<f32>),
    F64(Vec<f64>),
    Traced(Vec<Value>),
}
macro_rules! storage {
    ($($variant:ident:$ty:ty),+) => {
        impl SequenceStorage {
            pub(crate) fn empty(element: &AbiType) -> Self {
                match element {
                    $(AbiType::Builtin(BuiltinType::$variant) => Self::$variant(Vec::new()),)+
                    _ => Self::Traced(Vec::new()),
                }
            }
            pub(crate) fn len(&self) -> usize {
                match self { $(Self::$variant(values) => values.len(),)+ Self::Traced(values) => values.len() }
            }
            pub(crate) fn capacity(&self) -> usize {
                match self { $(Self::$variant(values) => values.capacity(),)+ Self::Traced(values) => values.capacity() }
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
            pub(crate) fn append_repeated(&mut self, value: Value, count: usize) -> NativeResult<()> {
                let length = self.len().checked_add(count).ok_or_else(|| RuntimeError::resource_limit("sequence length"))?;
                match self { $(Self::$variant(values) => values.resize(length, <$ty>::decode(value)?),)+ Self::Traced(values) => values.resize(length, value) }
                Ok(())
            }
            pub(crate) fn copy_range(&self, start: usize, end: usize) -> NativeResult<Self> {
                if start > end || end > self.len() { return Err(invalid_index()); }
                Ok(match self { $(Self::$variant(values) => Self::$variant(copy_values(&values[start..end])?),)+ Self::Traced(values) => Self::Traced(copy_values(&values[start..end])?) })
            }
            pub(crate) fn copy_excluding(&self, start: usize, end: usize) -> NativeResult<Self> {
                if start > end || end > self.len() { return Err(invalid_index()); }
                Ok(match self { $(Self::$variant(values) => Self::$variant(copy_segments(&values[..start], &values[end..])?),)+ Self::Traced(values) => Self::Traced(copy_segments(&values[..start], &values[end..])?) })
            }
            pub(crate) fn append_storage(&mut self, source: &Self) -> NativeResult<()> {
                match (self, source) {
                    $((Self::$variant(values), Self::$variant(source)) => values.extend_from_slice(source),)+
                    (Self::Traced(values), Self::Traced(source)) => values.extend_from_slice(source),
                    _ => return Err(RuntimeError::module_validation("sequence copy scalar layout")),
                }
                Ok(())
            }
            pub(crate) fn overwrite(&mut self, start: usize, source: Self) -> NativeResult<()> {
                let end = start.checked_add(source.len()).ok_or_else(invalid_index)?;
                if end > self.len() { return Err(invalid_index()); }
                match (self, source) {
                    $((Self::$variant(values), Self::$variant(source)) => values[start..end].copy_from_slice(&source),)+
                    (Self::Traced(values), Self::Traced(source)) => { for (slot, value) in values[start..end].iter_mut().zip(source) { *slot = value; } }
                    _ => return Err(RuntimeError::module_validation("sequence copy scalar layout")),
                }
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
            pub(crate) fn pop(&mut self) -> Option<Value> {
                match self { $(Self::$variant(values) => values.pop().map(NativeScalar::encode),)+ Self::Traced(values) => values.pop() }
            }
            pub(crate) fn remove(&mut self, index: usize) -> Option<Value> {
                if index >= self.len() { return None; }
                Some(match self { $(Self::$variant(values) => values.remove(index).encode(),)+ Self::Traced(values) => values.remove(index) })
            }
            pub(crate) fn swap_remove(&mut self, index: usize) -> Option<Value> {
                if index >= self.len() { return None; }
                Some(match self { $(Self::$variant(values) => values.swap_remove(index).encode(),)+ Self::Traced(values) => values.swap_remove(index) })
            }
            pub(crate) fn clear(&mut self) { match self { $(Self::$variant(values) => values.clear(),)+ Self::Traced(values) => values.clear() } }
            pub(crate) fn truncate(&mut self, length: usize) { match self { $(Self::$variant(values) => values.truncate(length),)+ Self::Traced(values) => values.truncate(length) } }
            pub(crate) fn reverse(&mut self) { match self { $(Self::$variant(values) => values.reverse(),)+ Self::Traced(values) => values.reverse() } }
            pub(crate) fn swap(&mut self, left: usize, right: usize) -> NativeResult<()> {
                if left >= self.len() || right >= self.len() { return Err(invalid_index()); }
                match self { $(Self::$variant(values) => values.swap(left, right),)+ Self::Traced(values) => values.swap(left, right) }
                Ok(())
            }
        }
    };
}
storage!(Unit:(), Bool:bool, I8:i8, I16:i16, I32:i32, I64:i64, ISize:isize,
    U8:u8, U16:u16, U32:u32, U64:u64, USize:usize, F32:f32, F64:f64);
fn copy_values<T: Clone>(values: &[T]) -> NativeResult<Vec<T>> {
    copy_segments(values, &[])
}
fn copy_segments<T: Clone>(left: &[T], right: &[T]) -> NativeResult<Vec<T>> {
    let mut copy = Vec::new();
    let length = left
        .len()
        .checked_add(right.len())
        .ok_or_else(|| RuntimeError::resource_limit("sequence copy length"))?;
    copy.try_reserve_exact(length)
        .map_err(|_| RuntimeError::resource_limit("sequence copy capacity"))?;
    copy.extend_from_slice(left);
    copy.extend_from_slice(right);
    Ok(copy)
}
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
    pub(crate) fn snapshot(&self) -> Vec<Value> {
        (0..self.len())
            .map(|index| self.get(index).expect("bounded storage index"))
            .collect()
    }
}

#[derive(Debug)]
pub(crate) struct SequencePayload {
    pub(crate) element: AbiType,
    pub(crate) contract: Rc<StorageType>,
    pub(crate) values: SequenceStorage,
    pub(crate) leased_units: Option<usize>,
}
impl NativePayload for SequencePayload {
    fn trace<'payload>(&'payload self, visit: &mut dyn FnMut(&'payload Value)) {
        for value in self.values.traced() {
            visit(value);
        }
    }
    fn units(&self) -> usize {
        self.leased_units.unwrap_or_else(|| self.values.len())
    }
}
impl NativeStorage {
    pub(crate) fn sequence(element: usize) -> Self {
        Self::with_layout(NativeStorageLayout::Sequence { element }, move |context| {
            let item = match context.ty() {
                AbiType::NativeObject(nominal) => nominal.arguments.get(element),
                AbiType::Array(item, _) if element == 0 => Some(item.as_ref()),
                _ => None,
            }
            .ok_or_else(|| RuntimeError::module_validation("sequence element type"))?;
            Ok(SequencePayload {
                leased_units: None,
                element: item.clone(),
                contract: context.element_contract(element)?,
                values: SequenceStorage::empty(item),
            })
        })
    }
}

mod sealed {
    pub trait Element {}
}
/// Supported contiguous scalar elements. Views borrow one buffer for the closure.
pub trait NativeElement: NativeScalar + sealed::Element {
    #[doc(hidden)]
    fn slice(storage: &dyn Any) -> Option<&[Self]>;
    #[doc(hidden)]
    fn slice_mut(storage: &mut dyn Any) -> Option<&mut [Self]>;
}
macro_rules! elements {
    ($($variant:ident:$rust:ty),+) => { $(
        impl sealed::Element for $rust {}
        impl NativeElement for $rust {
            fn slice(storage: &dyn Any) -> Option<&[Self]> {
                match storage.downcast_ref::<SequenceStorage>()? {
                    SequenceStorage::$variant(values) => Some(values), _ => None,
                }
            }
            fn slice_mut(storage: &mut dyn Any) -> Option<&mut [Self]> {
                match storage.downcast_mut::<SequenceStorage>()? {
                    SequenceStorage::$variant(values) => Some(values), _ => None,
                }
            }
        }
    )+ };
}
elements!(Unit:(), Bool:bool, I8:i8, I16:i16, I32:i32, I64:i64, ISize:isize,
    U8:u8, U16:u16, U32:u32, U64:u64, USize:usize, F32:f32, F64:f64);
