//! Optional value representation; methods and prelude exports follow in NR04.
#[kagari_native_macros::native_module("std::option", runtime = crate)]
pub mod option {
    use std::option::Option as RustOption;

    /// A checked optional script value with the ordinary Some/None tags.
    #[native_type]
    pub type Option<T: NativeValue> = RustOption<T>;
}
