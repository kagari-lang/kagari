//! Immutable range proxies preserve the engine's checked integer endpoint domain.
use super::{NativeCall, NativeResult, NativeValue, invalid, representation::NativeRepresentation};
use crate::{native_module::types::TypeExpression, range::RangeValue, value::Value};
use kagari_abi::{
    standard::surface::StandardEnum,
    types::{AbiType, native::NativeTypeConstructor},
};
use kagari_common::range::RangeKind;
use std::{marker::PhantomData, ops::Bound};

pub trait RangeShape: 'static {
    const KIND: RangeKind;
}
macro_rules! shapes {
    ($($name:ident => $kind:ident),+ $(,)?) => {$(
        pub struct $name;
        impl RangeShape for $name { const KIND: RangeKind = RangeKind::$kind; }
    )+};
}
shapes!(Exclusive => Exclusive, Inclusive => Inclusive, From => From, To => To,
    ToInclusive => ToInclusive, Full => Full);

pub struct NativeRange<T: NativeValue, S: RangeShape> {
    call: NativeCall,
    value: RangeValue,
    ty: AbiType,
    _shape: PhantomData<(T, S)>,
}
impl<T: NativeValue, S: RangeShape> NativeRange<T, S> {
    pub fn start_bound(&self) -> NativeResult<Bound<T>> {
        self.bound(false)
    }
    pub fn end_bound(&self) -> NativeResult<Bound<T>> {
        self.bound(true)
    }
    fn bound(&self, upper: bool) -> NativeResult<Bound<T>> {
        let AbiType::Range(item, _) = &self.ty else {
            return Err(invalid());
        };
        let expected = AbiType::StandardEnum {
            kind: StandardEnum::Bound,
            args: vec![*item.clone()],
        };
        let value = self
            .value
            .bound(&self.call.heap, &self.ty, &expected, upper)?;
        Bound::<T>::read(&self.call, value, &expected)
    }
}
impl<T: NativeValue, S: RangeShape> NativeRepresentation for NativeRange<T, S> {
    const CONSTRUCTOR: NativeTypeConstructor = NativeTypeConstructor::Range(S::KIND);
}
impl<T: NativeValue, S: RangeShape> NativeValue for NativeRange<T, S> {
    fn type_expression(names: &[&'static str]) -> TypeExpression {
        TypeExpression::Range(Box::new(T::type_expression(names)), S::KIND)
    }
    fn read(call: &NativeCall, value: Value, expected: &AbiType) -> NativeResult<Self> {
        if !matches!(expected, AbiType::Range(_, kind) if *kind == S::KIND) {
            return Err(invalid());
        }
        call.check(&value, expected)?;
        let Value::Range(value) = value else {
            return Err(invalid());
        };
        Ok(Self {
            call: call.clone(),
            value,
            ty: expected.clone(),
            _shape: PhantomData,
        })
    }
    fn write(self, call: &NativeCall, expected: &AbiType) -> NativeResult<Value> {
        call.compatible(&self.call)?;
        if *expected != self.ty {
            return Err(invalid());
        }
        let value = Value::Range(self.value);
        call.check(&value, expected)?;
        Ok(value)
    }
}
