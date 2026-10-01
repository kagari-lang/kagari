//! Projected selected receivers use real foreign associated implementations.
#[path = "native_selected_api.rs"]
pub mod base;
use kagari_runtime::{error::RuntimeError, native::api::NativeApi};

pub fn api() -> Result<NativeApi, RuntimeError> {
    let base = base::api()?;
    let consumer = projected::native_api(&base.catalog())?;
    NativeApi::combine(vec![base, consumer])
}

#[kagari_native_macros::native_module("game::projected", catalog)]
pub mod projected {
    use kagari_runtime::{
        native::{NativeAction, NativeContext, NativeInvocationState},
        native_value::{
            NativeCall, NativeResult, NativeValue, continuation::NativeContinuation,
            selected::NativeSelected,
        },
        value::Value,
    };

    /// Select a member on the exact associated output, then resume with its value.
    #[native]
    pub fn echo_twice<T: NativeValue, U: NativeValue>(
        value: T,
        #[context] call: &NativeCall,
        #[selected(T: game::selected::Echo<Output = U>::echo)] source: NativeSelected<(T,), U>,
        #[selected(<T as game::selected::Echo<Output = U>>::Output: game::selected::Echo<Output = i32>::echo)]
        output: NativeSelected<(U,), i32>,
    ) -> NativeContinuation<i32> {
        NativeContinuation::new(Twice {
            call: call.clone(),
            value: Some(value),
            source,
            output,
            second: false,
        })
    }

    /// The projection alone retains its base obligation, even without a source callback.
    #[native]
    pub fn inspect_output<T: NativeValue, U: NativeValue>(
        value: U,
        #[context] call: &NativeCall,
        #[selected(<T as game::selected::Echo<Output = U>>::Output: game::selected::Echo<Output = i32>::echo)]
        output: NativeSelected<(U,), i32>,
    ) -> NativeContinuation<i32> {
        NativeContinuation::new(Inspect {
            call: call.clone(),
            value: Some(value),
            output,
        })
    }

    struct Inspect<U: NativeValue> {
        call: NativeCall,
        value: Option<U>,
        output: NativeSelected<(U,), i32>,
    }
    impl<U: NativeValue> NativeInvocationState for Inspect<U> {
        fn advance(&mut self, context: &mut NativeContext<'_>) -> NativeResult<NativeAction> {
            Ok(NativeAction::Callback(self.output.request(
                context,
                (self.value.take().expect("one request"),),
            )?))
        }
        fn receive(
            &mut self,
            context: &mut NativeContext<'_>,
            value: Value,
        ) -> NativeResult<NativeAction> {
            let output = self.output.result(context, value)?;
            Ok(NativeAction::Complete(
                output.write(&self.call, self.call.result_type())?,
            ))
        }
    }

    struct Twice<T: NativeValue, U: NativeValue> {
        call: NativeCall,
        value: Option<T>,
        source: NativeSelected<(T,), U>,
        output: NativeSelected<(U,), i32>,
        second: bool,
    }
    impl<T: NativeValue, U: NativeValue> NativeInvocationState for Twice<T, U> {
        fn advance(&mut self, context: &mut NativeContext<'_>) -> NativeResult<NativeAction> {
            Ok(NativeAction::Callback(self.source.request(
                context,
                (self.value.take().expect("first request"),),
            )?))
        }
        fn receive(
            &mut self,
            context: &mut NativeContext<'_>,
            value: Value,
        ) -> NativeResult<NativeAction> {
            if !self.second {
                let output = self.source.result(context, value)?;
                context.heap().alloc_array(vec![])?;
                self.second = true;
                return Ok(NativeAction::Callback(
                    self.output.request(context, (output,))?,
                ));
            }
            let output = self.output.result(context, value)?;
            Ok(NativeAction::Complete(
                output.write(&self.call, self.call.result_type())?,
            ))
        }
    }
}
