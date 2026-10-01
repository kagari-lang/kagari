//! Real Rust templates add defaults; the Rust trait declares only required members.
use kagari_native_macros::native_module;

#[native_module("game::typed_defaults")]
pub mod defaults {
    use kagari_runtime::{
        native::{NativeAction, NativeContext, NativeInvocationState},
        native_value::{
            NativeCall, NativeResult, NativeValue, continuation::NativeContinuation,
            selected::NativeSelected,
        },
        value::Value,
    };

    #[native_trait]
    pub trait Source<P: NativeValue> {
        type Output: NativeValue;
        fn read(&self, by: P) -> NativeResult<Self::Output>;
    }

    #[native_impl]
    impl Source<i32> for bool {
        type Output = i32;
        fn read(&self, by: i32) -> NativeResult<i32> {
            Ok(if *self { 40 + by } else { by })
        }
    }

    // Declare this first to prove dependency resolution is independent of order.
    /// An overridable default calling the checked final default.
    #[native_default(T: Source<P, Output = U>::alternate)]
    fn alternate_template<U: NativeValue, T: NativeValue, P: NativeValue>(
        #[context] call: &NativeCall,
        value: T,
        by: P,
        #[selected(T: Source<P, Output = U>::echo)] echo: NativeSelected<(T, P), U>,
    ) -> NativeContinuation<U> {
        invoke(call, value, by, echo)
    }

    /// Read through the registered template with the exact associated output.
    #[native_default(T: Source<P, Output = U>::echo, final)]
    fn echo_template<U: NativeValue, T: NativeValue, P: NativeValue>(
        #[context] call: &NativeCall,
        value: T,
        by: P,
        #[selected(T: Source<P, Output = U>::read)] read: NativeSelected<(T, P), U>,
    ) -> NativeContinuation<U> {
        invoke(call, value, by, read)
    }

    #[native]
    pub fn invoke_default<T: NativeValue, U: NativeValue, P: NativeValue>(
        #[context] call: &NativeCall,
        value: T,
        by: P,
        #[selected(T: Source<P, Output = U>::alternate)] alternate: NativeSelected<(T, P), U>,
    ) -> NativeContinuation<U> {
        invoke(call, value, by, alternate)
    }

    fn invoke<T: NativeValue, U: NativeValue, P: NativeValue>(
        call: &NativeCall,
        value: T,
        by: P,
        selected: NativeSelected<(T, P), U>,
    ) -> NativeContinuation<U> {
        NativeContinuation::new(Invoke {
            call: call.clone(),
            arguments: Some((value, by)),
            selected,
        })
    }

    struct Invoke<T: NativeValue, U: NativeValue, P: NativeValue> {
        call: NativeCall,
        arguments: Option<(T, P)>,
        selected: NativeSelected<(T, P), U>,
    }
    impl<T: NativeValue, U: NativeValue, P: NativeValue> NativeInvocationState for Invoke<T, U, P> {
        fn advance(&mut self, context: &mut NativeContext<'_>) -> NativeResult<NativeAction> {
            let arguments = self.arguments.take().expect("one request before receive");
            Ok(NativeAction::Callback(
                self.selected.request(context, arguments)?,
            ))
        }
        fn receive(
            &mut self,
            context: &mut NativeContext<'_>,
            value: Value,
        ) -> NativeResult<NativeAction> {
            context.heap().alloc_array(vec![])?;
            let result = self.selected.result(context, value)?;
            Ok(NativeAction::Complete(
                result.write(&self.call, self.call.result_type())?,
            ))
        }
    }
}
