//! Owning array handles expose checked operations, never unrestricted heap references.
use super::{NativeCall, NativeResult, NativeValue, invalid};
use crate::{
    error::{RuntimeError, RuntimeErrorKind},
    gc::{HeapObjectId, RootSet},
    native::ops_api::ops::Index,
    native_module::types::TypeExpression,
    native_value::{reorder::NativeReorder, representation::NativeRepresentation},
    value::Value,
};
use kagari_abi::types::{AbiType, native::NativeTypeConstructor};
use kagari_common::collection::CollectionAccess;
use std::marker::PhantomData;

impl<T: NativeValue> NativeRepresentation for NativeArray<T> {
    const CONSTRUCTOR: NativeTypeConstructor = NativeTypeConstructor::Array;
}

pub struct NativeArray<T: NativeValue> {
    call: NativeCall,
    id: HeapObjectId,
    item: AbiType,
    access: CollectionAccess,
    _root: RootSet,
    _type: PhantomData<T>,
}
impl<T: NativeValue> NativeArray<T> {
    fn check_mutable(&self) -> NativeResult<()> {
        if self.access == CollectionAccess::Mutable {
            Ok(())
        } else {
            Err(invalid())
        }
    }
    pub fn new(call: &NativeCall) -> NativeResult<Self> {
        let expected = call.result_type();
        let id = call.heap.alloc_array(vec![])?;
        Self::read(call, Value::Array(id), expected)
    }
    pub fn len(&self) -> usize {
        self.call
            .heap
            .array_len(self.id)
            .expect("rooted, checked array handle remains live")
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    pub fn get(&self, index: usize) -> NativeResult<Option<T>> {
        self.call
            .heap
            .array_get(self.id, index)
            .map(|value| {
                self.call.check(&value, &self.item)?;
                T::read(&self.call, value, &self.item)
            })
            .transpose()
    }
    pub fn set(&self, index: usize, value: T) -> NativeResult<()> {
        self.check_mutable()?;
        self.call
            .heap
            .array_set(self.id, index, value.write(&self.call, &self.item)?)
    }
    pub fn push(&self, value: T) -> NativeResult<()> {
        self.check_mutable()?;
        self.call
            .heap
            .array_push(self.id, value.write(&self.call, &self.item)?)
    }
    /// Prepare a rooted replacement while blocking target writes through aliases.
    pub fn prepare_reorder(&self) -> NativeResult<NativeReorder<T>> {
        self.check_mutable()?;
        NativeReorder::new(self.call.clone(), self.id, self.item.clone())
    }
}
impl<T: NativeValue> NativeValue for NativeArray<T> {
    fn type_expression(generics: &[&'static str]) -> TypeExpression {
        TypeExpression::MutableArray(Box::new(T::type_expression(generics)))
    }
    fn read(call: &NativeCall, value: Value, expected: &AbiType) -> NativeResult<Self> {
        let AbiType::Array(item, access) = expected else {
            return Err(invalid());
        };
        call.check(&value, expected)?;
        let Value::Array(id) = value else {
            return Err(invalid());
        };
        Ok(Self {
            call: call.clone(),
            id,
            item: (**item).clone(),
            access: *access,
            _root: call
                .heap
                .root_execution_values(vec![Value::Array(id)])
                .ok_or_else(invalid)?,
            _type: PhantomData,
        })
    }
    fn write(self, call: &NativeCall, expected: &AbiType) -> NativeResult<Value> {
        call.compatible(&self.call)?;
        if *expected != AbiType::Array(Box::new(self.item.clone()), self.access) {
            return Err(invalid());
        }
        call.check(&Value::Array(self.id), expected)?;
        call.retain(Value::Array(self.id))
    }
}
impl<T: NativeValue> Index<usize> for NativeArray<T> {
    type Output = T;
    fn index(&self, index: usize) -> NativeResult<T> {
        self.get(index)?.ok_or_else(|| {
            RuntimeError::new(
                RuntimeErrorKind::IndexOutOfBounds,
                format!("invalid index `{index}`"),
            )
        })
    }
}
