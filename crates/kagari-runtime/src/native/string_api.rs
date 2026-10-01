//! String representation; the direct and iterator methods migrate in NR04.
#[kagari_native_macros::native_module("std::string", runtime = crate)]
pub mod string {
    use std::string::String as RustString;

    /// An immutable, checked UTF-8 script string.
    #[native_type]
    pub type String = RustString;
}
