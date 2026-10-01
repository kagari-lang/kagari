//! An application consumes ordinary comparison contracts and selected targets.
use kagari_native_macros::native_module;
use kagari_runtime::{
    error::RuntimeError,
    native::{
        api::NativeApi, cmp_api::cmp, option_api::option, result_api::result, string_api::string,
    },
};

pub fn api() -> Result<NativeApi, RuntimeError> {
    let cmp = cmp::native_api()?;
    let rank = rank::native_api(&cmp.catalog())?;
    NativeApi::combine(vec![
        cmp,
        rank,
        option::native_api()?,
        result::native_api()?,
        string::native_api()?,
    ])
}

#[native_module("game::rank", catalog)]
pub mod rank {
    use kagari_runtime::{
        native::{NativeAction, NativeContext, NativeInvocationState},
        native_value::{
            NativeCall, NativeResult, NativeValue, continuation::NativeContinuation,
            selected::NativeSelected,
        },
        value::Value,
    };
    use std::cmp::Ordering;

    #[native]
    pub fn echo_u8(value: u8) -> u8 {
        value
    }
    #[native]
    pub fn echo_u16(value: u16) -> u16 {
        value
    }
    #[native]
    pub fn echo_u32(value: u32) -> u32 {
        value
    }
    #[native]
    pub fn nan() -> f64 {
        f64::NAN
    }

    /// Compare through the caller's checked total-order implementation.
    #[native]
    pub fn compare<T: NativeValue>(
        left: T,
        right: T,
        #[context] call: &NativeCall,
        #[selected(T: std::cmp::Ord::cmp)] cmp: NativeSelected<(T, T), Ordering>,
    ) -> NativeContinuation<Ordering> {
        NativeContinuation::new(Compare {
            call: call.clone(),
            values: Some((left, right)),
            cmp,
        })
    }
    struct Compare<T: NativeValue> {
        call: NativeCall,
        values: Option<(T, T)>,
        cmp: NativeSelected<(T, T), Ordering>,
    }
    impl<T: NativeValue> NativeInvocationState for Compare<T> {
        fn advance(&mut self, context: &mut NativeContext<'_>) -> NativeResult<NativeAction> {
            Ok(NativeAction::Callback(self.cmp.request(
                context,
                self.values.take().expect("one comparison"),
            )?))
        }
        fn receive(
            &mut self,
            context: &mut NativeContext<'_>,
            value: Value,
        ) -> NativeResult<NativeAction> {
            Ok(NativeAction::Complete(
                self.cmp
                    .result(context, value)?
                    .write(&self.call, self.call.result_type())?,
            ))
        }
    }
}
