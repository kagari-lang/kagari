//! Generate finite DTO schemas and owned conversions at their defining Rust type.
mod value;
use proc_macro::TokenStream;
use syn::{DeriveInput, Error, parse_macro_input};

/// Derive ordinary named-struct or unit/tuple-enum schema and owned conversion.
/// Fields remain independent Rust values; no methods, traits or GC-to-Rust loans
/// are exposed. `#[kagari(name = "ShortName")]` overrides the short script name.
#[proc_macro_derive(KagariValue, attributes(kagari))]
pub fn kagari_value(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    value::expand(&input)
        .unwrap_or_else(Error::into_compile_error)
        .into()
}
