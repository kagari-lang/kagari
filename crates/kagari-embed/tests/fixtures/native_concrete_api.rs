//! Real Rust implementations discharge a foreign default's concrete obligation.
use kagari_native_macros::native_module;
use kagari_runtime::{error::RuntimeError, native::api::NativeApi};

pub fn api() -> Result<NativeApi, RuntimeError> {
    let contract = contract::native_api()?;
    let provider = provider::native_api(&contract.catalog())?;
    let defaults = defaults::native_api(&provider.catalog())?;
    NativeApi::combine(vec![contract, provider, defaults])
}

#[native_module("game::fact_contract")]
pub mod contract {
    #[native_trait]
    pub trait Check {
        fn read(&self) -> i32;
        fn shift(&self) -> i32;
    }
}

#[native_module("game::fact_provider", catalog)]
pub mod provider {
    use super::contract::Check;

    #[native_impl(contract = "game::fact_contract::Check")]
    impl Check for bool {
        fn read(&self) -> i32 {
            if *self { 42 } else { 0 }
        }
        fn shift(&self) -> i32 {
            if *self { 43 } else { 1 }
        }
    }

    #[native]
    pub fn unrelated() -> i32 {
        0
    }
}

#[native_module("game::fact_provider", catalog)]
pub mod changed_provider {
    use super::contract::Check;

    #[native_impl(contract = "game::fact_contract::Check")]
    impl Check for usize {
        fn read(&self) -> i32 {
            42
        }
        fn shift(&self) -> i32 {
            43
        }
    }
}

#[native_module("game::concrete_defaults", catalog)]
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
    pub trait Run {
        fn marker(&self) -> i32;
    }
    #[native_impl]
    impl Run for usize {
        fn marker(&self) -> i32 {
            42
        }
    }

    #[native]
    pub fn __native_dependency_0() -> i32 {
        0
    }

    /// Invoke the concrete bool implementation from the required provider.
    #[native_default(T: Run::answer)]
    fn answer<T: NativeValue>(
        value: T,
        #[context] call: &NativeCall,
        #[selected(bool: game::fact_contract::Check::read)] read: NativeSelected<(bool,), i32>,
    ) -> NativeContinuation<i32> {
        NativeContinuation::new(Invoke {
            call: call.clone(),
            value: Some(value),
            read,
        })
    }

    #[native]
    pub fn invoke(
        value: usize,
        #[context] call: &NativeCall,
        #[selected(usize: Run::answer)] answer: NativeSelected<(usize,), i32>,
    ) -> NativeContinuation<i32> {
        NativeContinuation::new(CallDefault {
            call: call.clone(),
            value: Some(value),
            answer,
        })
    }

    struct Invoke<T: NativeValue> {
        call: NativeCall,
        value: Option<T>,
        read: NativeSelected<(bool,), i32>,
    }
    impl<T: NativeValue> NativeInvocationState for Invoke<T> {
        fn advance(&mut self, context: &mut NativeContext<'_>) -> NativeResult<NativeAction> {
            Ok(NativeAction::Callback(self.read.request(context, (true,))?))
        }
        fn receive(
            &mut self,
            context: &mut NativeContext<'_>,
            value: Value,
        ) -> NativeResult<NativeAction> {
            let value = self.read.result(context, value)?;
            drop(self.value.take());
            Ok(NativeAction::Complete(
                value.write(&self.call, self.call.result_type())?,
            ))
        }
    }
    struct CallDefault {
        call: NativeCall,
        value: Option<usize>,
        answer: NativeSelected<(usize,), i32>,
    }
    impl NativeInvocationState for CallDefault {
        fn advance(&mut self, context: &mut NativeContext<'_>) -> NativeResult<NativeAction> {
            Ok(NativeAction::Callback(self.answer.request(
                context,
                (self.value.take().expect("one callback"),),
            )?))
        }
        fn receive(
            &mut self,
            context: &mut NativeContext<'_>,
            value: Value,
        ) -> NativeResult<NativeAction> {
            let value = self.answer.result(context, value)?;
            Ok(NativeAction::Complete(
                value.write(&self.call, self.call.result_type())?,
            ))
        }
    }
}
