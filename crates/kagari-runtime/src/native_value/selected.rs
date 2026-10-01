//! Typed selected dependencies retain their checked application and source generation.
use crate::{
    native::{NativeCallback, NativeContext},
    native_module::types::TypeExpression,
    native_value::{NativeCall, NativeResult, NativeValue, arguments::NativeArguments, invalid},
    value::Value,
};
use kagari_abi::native_import::callables::NativeCallableApplication;
use std::marker::PhantomData;

/// An injected trait-member dependency, declared with `#[selected(T: Trait::method)]`.
/// The outer tuple lists all arguments, including the receiver. This handle owns
/// the checked selection and pins its generation; it is not a script value.
pub struct NativeSelected<A: NativeArguments, R: NativeValue> {
    call: NativeCall,
    selected: NativeCallableApplication,
    _types: PhantomData<(A, R)>,
}

impl NativeCall {
    /// Used by generated adapters; ordinary Rust implementations receive the
    /// typed handle through an annotated parameter rather than choosing a slot.
    #[doc(hidden)]
    pub fn selected<A: NativeArguments, R: NativeValue>(
        &self,
        slot: usize,
    ) -> NativeResult<NativeSelected<A, R>> {
        Ok(NativeSelected {
            call: self.clone(),
            selected: self.callables.get(slot).cloned().ok_or_else(invalid)?,
            _types: PhantomData,
        })
    }
}

impl<A: NativeArguments, R: NativeValue> NativeSelected<A, R> {
    #[doc(hidden)]
    pub fn signature_expression(generics: &[&'static str]) -> TypeExpression {
        TypeExpression::Function {
            params: A::type_expressions(generics),
            result: Box::new(R::type_expression(generics)),
        }
    }

    pub fn request(
        &self,
        context: &NativeContext<'_>,
        arguments: A,
    ) -> NativeResult<NativeCallback> {
        let call = NativeCall::new(context)?;
        call.compatible(&self.call)?;
        let scope = self.call.conversion_scope();
        let arguments = arguments.into_values(&scope, &self.selected.signature.params)?;
        context.selected_application(&self.call.owner, &self.selected, arguments)
    }

    pub fn result(&self, context: &NativeContext<'_>, value: Value) -> NativeResult<R> {
        let call = NativeCall::new(context)?;
        call.compatible(&self.call)?;
        let scope = self.call.conversion_scope();
        scope.check(&value, &self.selected.signature.result)?;
        R::read(&scope, value, &self.selected.signature.result)
    }
}
