//! Compile-time native API authoring. No Kagari parser or runtime dependency.
mod expand;
mod parse;

use proc_macro::TokenStream;
use syn::{Error as SyntaxError, parse_macro_input};

/// Declare a native module and bind its methods to low-level factory functions.
/// Returns `Result<NativeApi, RuntimeError>`; generated text never defines semantics.
#[proc_macro]
pub fn native_module(input: TokenStream) -> TokenStream {
    let module = parse_macro_input!(input as parse::Module);
    expand::module(module)
        .unwrap_or_else(SyntaxError::into_compile_error)
        .into()
}
