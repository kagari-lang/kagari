//! Typed continuation results reuse the existing rooted invocation driver.
use super::{NativeCall, NativeResult, NativeReturn, NativeValue, invalid};
use crate::{
    gc::RootSet,
    native::{NativeCallback, NativeContext, NativeInvocationState},
    native_module::types::TypeExpression,
    value::Value,
};
use kagari_abi::types::AbiType;
use std::marker::PhantomData;

pub struct NativeContinuation<T: NativeValue> {
    state: Box<dyn NativeInvocationState>,
    _result: PhantomData<T>,
}
impl<T: NativeValue> NativeContinuation<T> {
    pub fn new(state: impl NativeInvocationState + 'static) -> Self {
        Self {
            state: Box::new(state),
            _result: PhantomData,
        }
    }
}
impl<T: NativeValue> NativeReturn for NativeContinuation<T> {
    const SCRATCH_SLOTS: usize = 2;
    fn type_expression(generics: &[&'static str]) -> TypeExpression {
        T::type_expression(generics)
    }
    fn into_state(
        self,
        _: &NativeCall,
        _: &mut NativeContext<'_>,
        _: bool,
    ) -> NativeResult<Box<dyn NativeInvocationState>> {
        Ok(self.state)
    }
}

pub struct NativeFn<A: NativeValue, R: NativeValue> {
    call: NativeCall,
    root: RootSet,
    signature: AbiType,
    _types: PhantomData<(A, R)>,
}
impl<A: NativeValue, R: NativeValue> NativeValue for NativeFn<A, R> {
    fn type_expression(generics: &[&'static str]) -> TypeExpression {
        let params = match A::type_expression(generics) {
            TypeExpression::Tuple(items) => items,
            other => vec![other],
        };
        TypeExpression::Function {
            params,
            result: Box::new(R::type_expression(generics)),
        }
    }
    fn read(call: &NativeCall, value: Value, expected: &AbiType) -> NativeResult<Self> {
        if !matches!(expected, AbiType::Function { .. }) {
            return Err(invalid());
        }
        call.check(&value, expected)?;
        Ok(Self {
            call: call.clone(),
            root: call
                .heap
                .root_execution_values(vec![value])
                .ok_or_else(invalid)?,
            signature: expected.clone(),
            _types: PhantomData,
        })
    }
    fn write(self, call: &NativeCall, expected: &AbiType) -> NativeResult<Value> {
        call.compatible(&self.call)?;
        if *expected != self.signature {
            return Err(invalid());
        }
        call.retain(self.root.get(0).ok_or_else(invalid)?)
    }
}
impl<R: NativeValue> NativeFn<usize, R> {
    pub fn request(
        &self,
        context: &NativeContext<'_>,
        index: usize,
    ) -> NativeResult<NativeCallback> {
        let call = NativeCall::new(context)?;
        call.compatible(&self.call)?;
        let AbiType::Function { params, .. } = &self.signature else {
            return Err(invalid());
        };
        let argument = index.write(
            &call,
            params
                .first()
                .filter(|_| params.len() == 1)
                .ok_or_else(invalid)?,
        )?;
        context.callback(
            &self.root.get(0).ok_or_else(invalid)?,
            &self.signature,
            vec![argument],
        )
    }
}
