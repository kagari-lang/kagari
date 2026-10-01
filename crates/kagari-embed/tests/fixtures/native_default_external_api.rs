//! Cross-package defaults retain actual parent and private template contracts.
use crate::fixture_api::game::parent;
use kagari_native_macros::native_module;
use kagari_runtime::{
    error::RuntimeError,
    native::{api::NativeApi, catalog::NativeCatalog},
};

pub fn api(changed: bool) -> Result<NativeApi, RuntimeError> {
    let parent = if changed {
        changed_parent::native_api()?
    } else {
        parent::native_api()?
    };
    let catalog = parent.catalog();
    let child = child::native_api(&catalog)?;
    let plain = plain::native_api(&catalog)?;
    let values = values::native_api(&NativeCatalog::from_apis(&[&parent, &child])?)?;
    NativeApi::combine(vec![parent, values, child, plain])
}

pub mod game {
    use super::native_module;
    #[native_module("game::parent")]
    pub mod parent {
        use crate::fixture_api::forward;
        use kagari_runtime::native_value::{
            NativeCall, NativeResult, NativeValue, continuation::NativeContinuation,
            selected::NativeSelected,
        };

        #[native_trait]
        pub trait Parent<P: NativeValue> {
            fn read(&self, by: P) -> NativeResult<P>;
            fn shift(&self, by: P) -> NativeResult<P>;
        }
        #[native_default(T: Parent<P>::echo, final)]
        fn echo_template<T: NativeValue, P: NativeValue>(
            #[context] call: &NativeCall,
            value: T,
            by: P,
            #[selected(T: Parent<P>::read)] read: NativeSelected<(T, P), P>,
        ) -> NativeContinuation<P> {
            forward(call, value, by, read)
        }
    }
}

#[native_module("game::parent")]
pub mod changed_parent {
    use super::forward;
    use kagari_runtime::native_value::{
        NativeCall, NativeResult, NativeValue, continuation::NativeContinuation,
        selected::NativeSelected,
    };

    #[native_trait]
    pub trait Parent<P: NativeValue> {
        fn read(&self, by: P) -> NativeResult<P>;
        fn shift(&self, by: P) -> NativeResult<P>;
    }
    #[native_default(T: Parent<P>::echo, final)]
    fn echo_template<T: NativeValue, P: NativeValue>(
        #[context] call: &NativeCall,
        value: T,
        by: P,
        #[selected(T: Parent<P>::shift)] read: NativeSelected<(T, P), P>,
    ) -> NativeContinuation<P> {
        forward(call, value, by, read)
    }
    impl Parent<i32> for bool {
        fn read(&self, by: i32) -> NativeResult<i32> {
            Ok(if *self { 40 + by } else { by })
        }
        fn shift(&self, by: i32) -> NativeResult<i32> {
            let _ = self;
            Ok(by)
        }
    }
}

#[native_module("game::parent_values", catalog)]
pub mod values {
    use super::{child, game::parent};
    use kagari_runtime::native_value::NativeResult;

    #[native_impl(contract = "game::parent::Parent")]
    impl parent::Parent<i32> for bool {
        fn read(&self, by: i32) -> NativeResult<i32> {
            Ok(if *self { 40 + by } else { by })
        }
        fn shift(&self, by: i32) -> NativeResult<i32> {
            let _ = self;
            Ok(by)
        }
    }
    #[native_impl(contract = "game::child::Child")]
    impl child::Child<i32> for bool {
        fn mark(&self) -> i32 {
            42
        }
    }
}

#[native_module("game::child", catalog)]
pub mod child {
    use super::{forward, game::parent};
    use kagari_runtime::native_value::{
        NativeCall, NativeValue, continuation::NativeContinuation, selected::NativeSelected,
    };

    #[native_trait(parents("game::parent::Parent"))]
    pub trait Child<P: NativeValue>: parent::Parent<P> {
        fn mark(&self) -> i32;
    }
    #[native_default(T: Child<P>::extra)]
    fn extra_template<T: NativeValue, P: NativeValue>(
        #[context] call: &NativeCall,
        value: T,
        by: P,
        #[selected(T: game::parent::Parent<P>::echo)] echo: NativeSelected<(T, P), P>,
    ) -> NativeContinuation<P> {
        forward(call, value, by, echo)
    }
    #[native]
    pub fn invoke<T: NativeValue, P: NativeValue>(
        #[context] call: &NativeCall,
        value: T,
        by: P,
        #[selected(T: Child<P>::extra)] extra: NativeSelected<(T, P), P>,
    ) -> NativeContinuation<P> {
        forward(call, value, by, extra)
    }
}

#[native_module("game::plain_default", catalog)]
pub mod plain {
    use super::game::parent;
    use kagari_runtime::native_value::NativeValue;
    #[native_trait(parents("game::parent::Parent"))]
    pub trait Plain<P: NativeValue>: parent::Parent<P> {
        fn mark(&self) -> i32;
    }
    impl Plain<i32> for bool {
        fn mark(&self) -> i32 {
            42
        }
    }
    #[native_default(T: Plain<P>::constant)]
    fn constant_template<T: NativeValue, P: NativeValue>(value: T, by: P) -> i32 {
        let _ = (value, by);
        42
    }
}

use kagari_runtime::{
    native::{NativeAction, NativeContext, NativeInvocationState},
    native_value::{
        NativeCall, NativeResult, NativeValue, continuation::NativeContinuation,
        selected::NativeSelected,
    },
    value::Value,
};

fn forward<T: NativeValue, P: NativeValue>(
    call: &NativeCall,
    value: T,
    by: P,
    selected: NativeSelected<(T, P), P>,
) -> NativeContinuation<P> {
    NativeContinuation::new(Forward {
        call: call.clone(),
        arguments: Some((value, by)),
        selected,
    })
}
struct Forward<T: NativeValue, P: NativeValue> {
    call: NativeCall,
    arguments: Option<(T, P)>,
    selected: NativeSelected<(T, P), P>,
}
impl<T: NativeValue, P: NativeValue> NativeInvocationState for Forward<T, P> {
    fn advance(&mut self, context: &mut NativeContext<'_>) -> NativeResult<NativeAction> {
        Ok(NativeAction::Callback(self.selected.request(
            context,
            self.arguments.take().expect("one callback before receive"),
        )?))
    }
    fn receive(
        &mut self,
        context: &mut NativeContext<'_>,
        value: Value,
    ) -> NativeResult<NativeAction> {
        context.heap().alloc_array(vec![])?;
        let value = self.selected.result(context, value)?;
        Ok(NativeAction::Complete(
            value.write(&self.call, self.call.result_type())?,
        ))
    }
}
