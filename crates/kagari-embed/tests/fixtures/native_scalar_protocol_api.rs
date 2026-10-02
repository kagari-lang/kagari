//! Application-owned protocols and checked selected standard members.
use kagari_native_macros::native_module;
use kagari_runtime::{
    error::RuntimeError,
    native::{
        api::NativeApi, cmp_api::cmp, fmt_api::fmt, hash_api::hash, option_api::option,
        result_api::result, string_api::string,
    },
};
pub fn api() -> Result<NativeApi, RuntimeError> {
    let hash = hash::native_api()?;
    let fmt = fmt::native_api()?;
    let cmp = cmp::native_api()?;
    let catalog = NativeApi::combine(vec![
        hash,
        fmt,
        cmp,
        option::native_api()?,
        result::native_api()?,
        string::native_api()?,
    ])?;
    let selection = selection::native_api(&catalog.catalog())?;
    NativeApi::combine(vec![catalog, selection, presentation::native_api()?])
}

#[native_module("game::presentation")]
pub mod presentation {
    use kagari_runtime::native_value::{
        NativeCall, NativeResult,
        scalar_protocol::{format_scalar, hash_scalar},
    };
    /// Application-owned text representation, independent of standard installation.
    #[native_type]
    pub type Text = String;
    /// Application-owned rendering with the same checked scalar operation.
    #[native_trait]
    pub trait Render {
        fn render(&self, #[context] call: &NativeCall) -> NativeResult<String>;
    }
    #[native_impl]
    impl Render for String {
        fn render(&self, #[context] call: &NativeCall) -> NativeResult<String> {
            format_scalar(call, self, true)
        }
    }
    /// An application hash function needs no installed standard package.
    #[native]
    pub fn fingerprint(value: String, #[context] call: &NativeCall) -> NativeResult<i64> {
        hash_scalar(call, &value)
    }
}

#[native_module("game::selection", catalog)]
pub mod selection {
    use super::forward;
    use kagari_runtime::native_value::{
        NativeCall, NativeValue, continuation::NativeContinuation, selected::NativeSelected,
    };
    #[native]
    pub fn stamp<T: NativeValue>(
        value: T,
        #[context] call: &NativeCall,
        #[selected(T: std::hash::Hash::hash)] hash: NativeSelected<(T,), i64>,
    ) -> NativeContinuation<i64> {
        forward(call, value, hash)
    }
    #[native]
    pub fn diagnostic<T: NativeValue>(
        value: T,
        #[context] call: &NativeCall,
        #[selected(T: std::fmt::Debug::debug)] debug: NativeSelected<(T,), String>,
    ) -> NativeContinuation<String> {
        forward(call, value, debug)
    }
    #[native]
    pub fn display<T: NativeValue>(
        value: T,
        #[context] call: &NativeCall,
        #[selected(T: std::fmt::Display::display)] display: NativeSelected<(T,), String>,
    ) -> NativeContinuation<String> {
        forward(call, value, display)
    }
}
fn forward<T: NativeValue, R: NativeValue>(
    call: &NativeCall,
    value: T,
    target: NativeSelected<(T,), R>,
) -> NativeContinuation<R> {
    NativeContinuation::new(Forward {
        call: call.clone(),
        values: Some((value,)),
        target,
    })
}
use kagari_runtime::{
    native::{NativeAction, NativeContext, NativeInvocationState},
    native_value::{
        NativeCall, NativeResult, NativeValue, continuation::NativeContinuation,
        selected::NativeSelected,
    },
    value::Value,
};
struct Forward<T: NativeValue, R: NativeValue> {
    call: NativeCall,
    values: Option<(T,)>,
    target: NativeSelected<(T,), R>,
}
impl<T: NativeValue, R: NativeValue> NativeInvocationState for Forward<T, R> {
    fn advance(&mut self, context: &mut NativeContext<'_>) -> NativeResult<NativeAction> {
        Ok(NativeAction::Callback(self.target.request(
            context,
            self.values.take().expect("one callback"),
        )?))
    }
    fn receive(
        &mut self,
        context: &mut NativeContext<'_>,
        value: Value,
    ) -> NativeResult<NativeAction> {
        Ok(NativeAction::Complete(
            self.target
                .result(context, value)?
                .write(&self.call, self.call.result_type())?,
        ))
    }
}
