//! Independent application functions use closed numeric adapters without library IDs.
use kagari_native_macros::native_module;

#[native_module("game::numbers")]
pub mod numbers {
    use kagari_runtime::native_value::{
        NativeResult, NativeValue,
        number::{NativeNumber, NativeSignedNumber},
    };
    #[cfg(test)]
    use std::mem;
    use std::{cell::RefCell, cmp::Ordering};

    thread_local! { static EVENTS: RefCell<Vec<i32>> = const { RefCell::new(Vec::new()) }; }

    #[native]
    pub fn record(value: i32) -> i32 {
        EVENTS.with(|events| events.borrow_mut().push(value));
        value
    }

    #[cfg(test)]
    pub fn take_events() -> Vec<i32> {
        EVENTS.with(|events| mem::take(&mut *events.borrow_mut()))
    }

    /// Application-owned numeric comparison, sharing the engine's sealed bound.
    #[native]
    pub fn smaller<T: NativeValue>(
        a: NativeNumber<T>,
        b: NativeNumber<T>,
    ) -> NativeResult<NativeNumber<T>> {
        Ok(if a.compare(&b)? == Ordering::Greater {
            b
        } else {
            a
        })
    }

    #[native]
    pub fn magnitude<T: NativeValue>(
        value: NativeSignedNumber<T>,
    ) -> NativeResult<NativeSignedNumber<T>> {
        value.checked_abs()
    }
}
