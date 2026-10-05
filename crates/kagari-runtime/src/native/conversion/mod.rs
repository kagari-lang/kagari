//! Fallible, rooted conversion shared by native calls and host object access.
pub mod arguments;
mod composites;
pub mod context;
mod scalars;
mod tuples;

use crate::{
    frame::types::arguments::TypeArgument,
    native::{
        binding::NativeResult, catalog::DeclarationCatalog, conversion::context::ConversionContext,
        types::Type,
    },
    value::Value,
};

/// Resolve the Rust boundary type through the installed declaration catalog.
/// This describes a type, never a sample value or a second declaration registry.
pub trait KagariType {
    fn kagari_type(catalog: &DeclarationCatalog) -> NativeResult<Type>;

    /// Check an installed expected type before conversion. Dynamic retained
    /// handles use its exact nominal scope instead of inventing an `Any` type.
    /// This never replaces validation of the converted value against that scope.
    fn check_type(cx: &ConversionContext<'_>, expected: &TypeArgument) -> NativeResult<()> {
        let ty = Self::kagari_type(&cx.runtime().native_entries.catalog)?;
        cx.check_declared_type(expected, ty)
    }
}

/// Convert owned Rust data. Composite adapters use `encode_value` for children,
/// so they share type checks, recursion/work bounds and temporary roots.
pub trait IntoKagari: KagariType + Sized {
    fn into_kagari(
        self,
        cx: &mut ConversionContext<'_>,
        expected: &TypeArgument,
    ) -> NativeResult<Value>;
}

/// Convert a checked script value into owned Rust data or an owning handle.
/// Input and output capabilities are independent, including for user adapters.
/// Borrowing a conversion input into the returned Rust value is forbidden:
///
/// ```compile_fail
/// use kagari_runtime::{
///     frame::types::arguments::TypeArgument,
///     native::{binding::NativeResult, catalog::DeclarationCatalog,
///         conversion::{KagariType, FromKagari, context::ConversionContext}, types::Type},
///     value::Value,
/// };
/// struct Borrowed<'a>(&'a str);
/// impl KagariType for Borrowed<'_> {
///     fn kagari_type(catalog: &DeclarationCatalog) -> NativeResult<Type> {
///         String::kagari_type(catalog)
///     }
/// }
/// impl<'a> FromKagari for Borrowed<'a> {
///     fn from_kagari(_: &mut ConversionContext<'_>, _: &TypeArgument,
///                    value: &Value) -> NativeResult<Self> {
///         let Value::Str(text) = value else { panic!("string") };
///         Ok(Self(text.as_str()))
///     }
/// }
/// ```
pub trait FromKagari: KagariType + Sized {
    /// Owning handle adapters retain the referent instead of recursively copying
    /// its graph. They may preserve a cycle back to an active owned conversion.
    const PRESERVES_IDENTITY: bool = false;

    fn from_kagari(
        cx: &mut ConversionContext<'_>,
        expected: &TypeArgument,
        value: &Value,
    ) -> NativeResult<Self>;
}
