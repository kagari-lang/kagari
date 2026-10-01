//! Independent typed authoring for selected trait dependencies and heap results.
use kagari_native_macros::native_module;
use kagari_runtime::{error::RuntimeError, native::api::NativeApi};

pub fn api() -> Result<NativeApi, RuntimeError> {
    let provider = selected::native_api()?;
    let consumer = consumer::native_api(&provider.catalog())?;
    NativeApi::combine(vec![provider, consumer])
}

#[native_module("game::external_selected", catalog)]
pub mod consumer {
    use super::selected;
    use kagari_runtime::native_value::{
        NativeCall, NativeResult, NativeValue,
        array::{NativeArray, NativeIndex},
        continuation::NativeContinuation,
        selected::NativeSelected,
    };

    #[native_type]
    pub struct ExternalBag<T: NativeValue>(NativeArray<T>);

    #[native_impl(contract = "game::selected::Echo")]
    impl selected::Echo for bool {
        type Output = i32;
        fn echo(&self) -> NativeResult<Self::Output> {
            Ok(if *self { 42 } else { 0 })
        }
        fn offset(&self, _: i32) -> NativeResult<Self::Output> {
            Ok(if *self { 42 } else { 0 })
        }
    }

    #[native]
    pub fn external_scalar<T: NativeValue>(
        #[context] call: &NativeCall,
        value: T,
        #[selected(T: game::selected::Echo<Output = i32>::echo)] echo: NativeSelected<(T,), i32>,
    ) -> NativeContinuation<i32> {
        selected::invoke_checked(call, value, echo)
    }

    #[native]
    pub fn external_echo<T: NativeValue>(
        #[context] call: &NativeCall,
        value: T,
        #[selected(T: game::selected::Echo<Output = NativeArray<i32>>::echo)] echo: NativeSelected<
            (T,),
            NativeArray<i32>,
        >,
    ) -> NativeContinuation<NativeArray<i32>> {
        selected::invoke(call, value, echo)
    }

    #[native_impl]
    impl<T: NativeValue> ExternalBag<T> {
        pub fn external_first(
            &self,
            #[context] call: &NativeCall,
            #[selected(T: game::selected::Echo<Output = NativeArray<i32>>::echo)]
            echo: NativeSelected<(T,), NativeArray<i32>>,
        ) -> NativeResult<NativeContinuation<NativeArray<i32>>> {
            Ok(selected::invoke(call, self.0.index(0)?, echo))
        }
    }
}

#[native_module("game::selected")]
pub mod selected {
    use kagari_runtime::{
        native::{NativeAction, NativeContext, NativeInvocationState},
        native_value::{
            NativeCall, NativeResult, NativeValue,
            array::{NativeArray, NativeIndex},
            continuation::NativeContinuation,
            selected::NativeSelected,
        },
        value::Value,
    };

    #[native_type]
    pub struct Bag<T: NativeValue>(NativeArray<T>);

    #[native_trait]
    pub trait Echo {
        type Output: NativeValue;
        fn echo(&self) -> NativeResult<Self::Output>;
        fn offset(&self, by: i32) -> NativeResult<Self::Output>;
    }

    #[native_impl]
    impl<T: NativeValue> Echo for Bag<T> {
        type Output = T;
        fn echo(&self) -> NativeResult<T> {
            self.0.index(0)
        }
        fn offset(&self, _: i32) -> NativeResult<T> {
            self.0.index(0)
        }
    }

    /// Invoke the checked Echo method. The injected parameter adds the script
    /// bound; it is absent from the script argument list.
    #[native]
    pub fn invoke<T: NativeValue>(
        #[context] call: &NativeCall,
        value: T,
        #[selected(T: Echo<Output = NativeArray<i32>>::echo)] echo: NativeSelected<
            (T,),
            NativeArray<i32>,
        >,
    ) -> NativeContinuation<NativeArray<i32>> {
        invoke_checked(call, value, echo)
    }

    pub fn invoke_checked<T: NativeValue, R: NativeValue>(
        call: &NativeCall,
        value: T,
        echo: NativeSelected<(T,), R>,
    ) -> NativeContinuation<R> {
        NativeContinuation::new(Invoke {
            call: call.clone(),
            value: Some(value),
            echo,
        })
    }

    #[native]
    pub fn invoke_both<T: NativeValue>(
        #[context] call: &NativeCall,
        value: T,
        #[selected(T: Echo<Output = NativeArray<i32>>::echo)] echo: NativeSelected<
            (T,),
            NativeArray<i32>,
        >,
        #[selected(T: Echo<Output = NativeArray<i32>>::offset)] offset: NativeSelected<
            (T, i32),
            NativeArray<i32>,
        >,
    ) -> NativeContinuation<NativeArray<i32>> {
        NativeContinuation::new(Both {
            call: call.clone(),
            value: Some(value),
            echo,
            offset,
            first: None,
        })
    }

    #[native_impl]
    impl<T: NativeValue> Bag<T> {
        pub fn invoke_first(
            &self,
            #[context] call: &NativeCall,
            #[selected(T: Echo<Output = NativeArray<i32>>::echo)] echo: NativeSelected<
                (T,),
                NativeArray<i32>,
            >,
        ) -> NativeResult<NativeContinuation<NativeArray<i32>>> {
            Ok(invoke(call, self.0.index(0)?, echo))
        }
    }

    struct Invoke<T: NativeValue, R: NativeValue> {
        call: NativeCall,
        value: Option<T>,
        echo: NativeSelected<(T,), R>,
    }

    struct Both<T: NativeValue> {
        call: NativeCall,
        value: Option<T>,
        echo: NativeSelected<(T,), NativeArray<i32>>,
        offset: NativeSelected<(T, i32), NativeArray<i32>>,
        first: Option<NativeArray<i32>>,
    }
    impl<T: NativeValue> NativeInvocationState for Both<T> {
        fn advance(&mut self, context: &mut NativeContext<'_>) -> NativeResult<NativeAction> {
            let value = self
                .value
                .take()
                .expect("one selected request before receive");
            Ok(NativeAction::Callback(
                self.echo.request(context, (value,))?,
            ))
        }
        fn receive(
            &mut self,
            context: &mut NativeContext<'_>,
            value: Value,
        ) -> NativeResult<NativeAction> {
            context.heap().alloc_array(vec![])?;
            if let Some(first) = self.first.take() {
                first.index(0)?;
                let output = self.offset.result(context, value)?;
                Ok(NativeAction::Complete(
                    output.write(&self.call, self.call.result_type())?,
                ))
            } else {
                self.first = Some(self.echo.result(context, value)?);
                Ok(NativeAction::Callback(
                    self.offset
                        .request(context, (self.call.argument(0)?, 1i32))?,
                ))
            }
        }
    }
    impl<T: NativeValue, R: NativeValue> NativeInvocationState for Invoke<T, R> {
        fn advance(&mut self, context: &mut NativeContext<'_>) -> NativeResult<NativeAction> {
            let value = self
                .value
                .take()
                .expect("one selected request before receive");
            Ok(NativeAction::Callback(
                self.echo.request(context, (value,))?,
            ))
        }
        fn receive(
            &mut self,
            context: &mut NativeContext<'_>,
            value: Value,
        ) -> NativeResult<NativeAction> {
            context.heap().alloc_array(vec![])?;
            let value = self.echo.result(context, value)?;
            let output = value.write(&self.call, self.call.result_type())?;
            Ok(NativeAction::Complete(output))
        }
    }
}
