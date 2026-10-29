//! Fallible, rooted conversion shared by native calls and host object access.
pub mod arguments;
mod composites;
pub mod context;
mod scalars;
pub mod structures;
mod tuples;
mod variants;

use crate::{
    error::RuntimeError,
    frame::types::arguments::TypeArgument,
    native::{
        binding::NativeResult, catalog::DeclarationCatalog, conversion::context::ConversionContext,
        projection::GeneratedField, schema::TypeRegistration, types::Type,
        value_handle::ScriptValue,
    },
};

/// Resolve the Rust boundary type through the installed declaration catalog.
/// This describes a type, never a sample value or a second declaration registry.
pub trait KagariType {
    fn kagari_type(catalog: &DeclarationCatalog) -> NativeResult<Type>;

    /// Read an owned scalar or retained handle from an inline borrowed field.
    /// The result owns its retention beyond the field loan and conversion context.
    /// Structured data is exposed as a location, never implicitly copied here.
    #[doc(hidden)]
    fn snapshot_field(
        &self,
        _: &mut ConversionContext<'_>,
        _: &TypeArgument,
    ) -> NativeResult<ScriptValue> {
        Err(RuntimeError::module_validation(
            "Rust field has no owned leaf reader",
        ))
    }

    /// Local layout proof emitted by the dedicated inline-struct derive. An
    /// ordinary conversion or serialization schema supplies no projection proof.
    #[doc(hidden)]
    fn generated_fields() -> Option<Vec<GeneratedField>> {
        None
    }

    /// Visit a schema dependency. Identity-free/manual mappings reuse their
    /// catalog mapping; nominal derives reserve a header before visiting fields.
    fn register_type(registration: &mut TypeRegistration<'_>) -> NativeResult<Type>
    where
        Self: 'static,
    {
        Self::kagari_type(registration.catalog())
    }

    /// Check an installed expected type before conversion. Dynamic retained
    /// handles use its exact nominal scope instead of inventing an `Any` type.
    /// This never replaces validation of the converted value against that scope.
    fn check_type(cx: &ConversionContext<'_>, expected: &TypeArgument) -> NativeResult<()> {
        let ty = Self::kagari_type(&cx.runtime().native_entries.catalog)?;
        cx.check_declared_type(expected, ty)
    }
}

/// Convert owned Rust data into a retained script value.
/// The result keeps its declared type and managed data alive independently of
/// the conversion context; scalar results need no GC root entry. Composite
/// adapters use `encode_value` for children to share type checks, recursion/work
/// bounds and temporary roots. External adapters compose converters or checked
/// builders rather than returning copied managed transport.
pub trait IntoKagari: KagariType + Sized {
    fn into_kagari(
        self,
        cx: &mut ConversionContext<'_>,
        expected: &TypeArgument,
    ) -> NativeResult<ScriptValue>;
}

/// Convert a retained script value into owned Rust data or an owning handle.
/// The input stays live through collection and reentry during conversion.
/// Input and output capabilities are independent, including for user adapters.
/// Borrowing a conversion input into the returned Rust value is forbidden:
///
/// ```compile_fail
/// use kagari_runtime::{
///     frame::types::arguments::TypeArgument,
///     native::{binding::NativeResult, catalog::DeclarationCatalog,
///         conversion::{KagariType, FromKagari, context::ConversionContext},
///         types::Type, value_handle::ScriptValue},
/// };
/// struct Borrowed<'a>(&'a ScriptValue);
/// impl KagariType for Borrowed<'_> {
///     fn kagari_type(catalog: &DeclarationCatalog) -> NativeResult<Type> {
///         String::kagari_type(catalog)
///     }
/// }
/// impl<'a> FromKagari for Borrowed<'a> {
///     fn from_kagari(_: &mut ConversionContext<'_>, _: &TypeArgument,
///                    value: &ScriptValue) -> NativeResult<Self> {
///         Ok(Self(value))
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
        value: &ScriptValue,
    ) -> NativeResult<Self>;
}
