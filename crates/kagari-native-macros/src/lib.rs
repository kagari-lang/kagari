//! Compile-time adapters from checked Rust definitions to native API records.
mod author;
mod defaults;
mod parents;
mod selected;
mod signature;

use proc_macro::TokenStream;
use syn::{Error as SyntaxError, ItemMod, parse_macro_input};

/// Export annotated Rust functions, checked native types and trait implementations.
/// Signatures come from `NativeValue`/`NativeReturn`, including Rust type aliases.
/// `#[native_type]` aliases and tuple wrappers derive their closed representation
/// from the resolved Rust adapter's `NativeRepresentation` implementation.
/// `#[native_type(export_variants)]` also publishes an enum's checked variants
/// at module scope; owner validation rejects missing enums and name collisions.
/// The `catalog` option generates `native_api(&NativeCatalog)` for external trait
/// dependencies. A trait impl may declare an explicit `contract = "pkg::mod::Trait"`
/// mapping while preserving its actual Rust trait path and conformance checks.
/// `#[native_trait(parents("pkg::mod::Parent"))]` maps imported Rust supertraits
/// in declaration order; their arguments still come from the actual Rust bounds.
/// `#[native_default(T: Trait<Output = U>::member, final)]` derives a new owned
/// script trait member from an actual Rust template, with explicit binder roles.
/// The helper is private; omitting `final` permits an explicit script override.
#[proc_macro_attribute]
pub fn native_module(args: TokenStream, input: TokenStream) -> TokenStream {
    let args = parse_macro_input!(args as author::Arguments);
    let module = parse_macro_input!(input as ItemMod);
    author::expand(args, module)
        .unwrap_or_else(SyntaxError::into_compile_error)
        .into()
}
