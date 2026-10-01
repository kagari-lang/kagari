//! Compile-time adapters from checked Rust definitions to native API records.
mod author;
mod selected;
mod signature;

use proc_macro::TokenStream;
use syn::{Error as SyntaxError, ItemMod, parse_macro_input};

/// Export annotated Rust functions, native array types and trait implementations.
/// Signatures come from `NativeValue`/`NativeReturn`, including Rust type aliases.
/// The `catalog` option generates `native_api(&NativeCatalog)` for external trait
/// dependencies. A trait impl may declare an explicit `contract = "pkg::mod::Trait"`
/// mapping while preserving its actual Rust trait path and conformance checks.
#[proc_macro_attribute]
pub fn native_module(args: TokenStream, input: TokenStream) -> TokenStream {
    let args = parse_macro_input!(args as author::Arguments);
    let module = parse_macro_input!(input as ItemMod);
    author::expand(args, module)
        .unwrap_or_else(SyntaxError::into_compile_error)
        .into()
}
