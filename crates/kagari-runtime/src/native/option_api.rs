//! Optional representation and direct queries authored as actual Rust methods.
#[kagari_native_macros::native_module("std::option", runtime = crate)]
pub mod option {
    use crate::native_value::{NativeResult, NativeValue, option::NativeOptionValue};

    /// A checked optional script value with the ordinary Some/None tags.
    #[native_type(export_variants)]
    pub struct Option<T: NativeValue>(NativeOptionValue<T>);

    #[native_impl]
    impl<T: NativeValue> Option<T> {
        /// Return whether this value contains a payload.
        pub fn is_some(&self) -> NativeResult<bool> {
            self.0.is_some()
        }
        /// Return whether this value has no payload.
        pub fn is_none(&self) -> NativeResult<bool> {
            Ok(!self.0.is_some()?)
        }
        /// Return the payload or the already evaluated fallback.
        pub fn unwrap_or(&self, fallback: T) -> NativeResult<T> {
            self.0.unwrap_or(fallback)
        }
    }
}
