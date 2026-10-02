//! Formatting declarations and scalar implementations from actual Rust methods.
macro_rules! formatting_module {
    ($($scalar:ty),*) => {
        #[kagari_native_macros::native_module("std::fmt", runtime = crate)]
        pub mod fmt {
            use std::cmp::Ordering;
            use crate::native_value::{NativeCall, NativeResult, scalar_protocol::format_scalar};
            /// A bounded diagnostic rendering; objects may retain identity previews.
            #[native_trait]
            pub trait Debug {
                /// Render this value with diagnostic quoting and escapes.
                fn debug(&self, #[context] call: &NativeCall) -> NativeResult<String>;
            }
            /// A bounded plain rendering for displayable values.
            #[native_trait]
            pub trait Display {
                /// Render this value without diagnostic string quoting.
                fn display(&self, #[context] call: &NativeCall) -> NativeResult<String>;
            }
            #[native_impl]
            impl Debug for Ordering {
                fn debug(&self, #[context] call: &NativeCall) -> NativeResult<String> {
                    format_scalar(call, self, true)
                }
            }
            $(
                #[native_impl]
                impl Debug for $scalar {
                    fn debug(&self, #[context] call: &NativeCall) -> NativeResult<String> {
                        format_scalar(call, self, true)
                    }
                }
                #[native_impl]
                impl Display for $scalar {
                    fn display(&self, #[context] call: &NativeCall) -> NativeResult<String> {
                        format_scalar(call, self, false)
                    }
                }
            )*
        }
    };
}
formatting_module!(
    i8,
    i16,
    i32,
    i64,
    isize,
    u8,
    u16,
    u32,
    u64,
    usize,
    bool,
    String,
    (),
    f32,
    f64
);
