//! Result direct queries preserve the rooted value and its original Err trace.
#[kagari_native_macros::native_module("std::result", runtime = crate)]
pub mod result {
    use crate::native_value::{NativeResult, NativeValue, result::NativeResultValue};
    /// A checked business Result, distinct from a native execution failure.
    #[native_type(export_variants)]
    pub struct Result<T: NativeValue, E: NativeValue>(NativeResultValue<T, E>);

    #[native_impl]
    impl<T: NativeValue, E: NativeValue> Result<T, E> {
        /// Return whether this value contains a success payload.
        pub fn is_ok(&self) -> NativeResult<bool> {
            self.0.is_ok()
        }
        /// Return whether this value contains an error payload.
        pub fn is_err(&self) -> NativeResult<bool> {
            Ok(!self.0.is_ok()?)
        }
        /// Return the success payload or the already evaluated fallback.
        pub fn unwrap_or(&self, fallback: T) -> NativeResult<T> {
            self.0.unwrap_or(fallback)
        }
    }
}
