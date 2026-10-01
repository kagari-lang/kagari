//! String representation and primitive parsing from actual Rust trait impls.
macro_rules! string_module {
    ($(($integer:ident, $rust:ty)),*; $($other:ty),*) => {
        #[kagari_native_macros::native_module("std::string", runtime = crate)]
        pub mod string {
            use std::string::String as RustString;
            use crate::{
                native::numeric_api::numeric::{$($integer),*},
                native_value::{NativeCall, NativeResult, NativeValue,
                    parse::ParseFailure, result::NativeResultValue},
            };

            /// An immutable, checked UTF-8 script string.
            #[native_type]
            pub type String = RustString;
            /// Recoverable parser failures; these are values rather than VM traps.
            #[native_type]
            pub type ParseError = ParseFailure;

            #[native_trait]
            pub trait FromStr {
                type Err: NativeValue;
                fn from_str(#[context] call: &NativeCall, text: String)
                    -> NativeResult<NativeResultValue<Self, Self::Err>>;
            }

            $(
                #[native_impl]
                impl FromStr for $integer {
                    type Err = ParseError;
                    /// Parse decimal input without whitespace, prefixes or separators.
                    fn from_str(#[context] call: &NativeCall, text: String)
                        -> NativeResult<NativeResultValue<Self, Self::Err>> {
                        call.charge_work(text.len() as u64)?;
                        let parsed = text.parse::<$rust>().map(Self).map_err(ParseFailure::integer);
                        NativeResultValue::from_result(call, parsed)
                    }
                }
            )*
            $(
                #[native_impl]
                impl FromStr for $other {
                    type Err = ParseError;
                    /// Parse the full primitive spelling, with no implicit trimming.
                    fn from_str(#[context] call: &NativeCall, text: String)
                        -> NativeResult<NativeResultValue<Self, Self::Err>> {
                        call.charge_work(text.len() as u64)?;
                        let parsed = if text.is_empty() { Err(ParseFailure::Empty) }
                            else { text.parse::<Self>().map_err(|_| ParseFailure::InvalidSyntax) };
                        NativeResultValue::from_result(call, parsed)
                    }
                }
            )*
        }
    };
}

string_module! {
    (I8, i8), (I16, i16), (I32, i32), (I64, i64), (ISize, i64),
    (U8, u8), (U16, u16), (U32, u32), (U64, u64), (USize, u64);
    f32, f64, bool
}
