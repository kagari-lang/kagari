//! Opt-in format-independent Serde conversion against an explicitly registered schema.
mod deserializer;
mod schema;
mod serializer;
use crate::{
    error::RuntimeError,
    frame::types::arguments::TypeArgument,
    native::{
        binding::NativeResult,
        catalog::DeclarationCatalog,
        conversion::{FromKagari, IntoKagari, KagariType, context::ConversionContext},
        schema::TypeRegistration,
        types::Type,
    },
    value::Value,
};
use deserializer::Deserializer;
use serde::{Serialize, de, de::DeserializeOwned, ser};
use serializer::Serializer;
use std::fmt;

/// An explicit conversion choice. The wrapper does not appear in script types.
/// Outbound and inbound capabilities require Serialize and DeserializeOwned
/// independently; neither exposes Rust methods, traits, pointers or opaque IDs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Serde<T>(pub T);

/// A finite static serialization shape, available before any value is supplied.
/// Implementations describe the actual Serde representation, including names and
/// skipped fields. This is independent of Rust field layout and proves no loans.
///
/// Schema derives reject representations outside the finite supported matrix.
/// Conditional omission:
///
/// ```compile_fail
/// #[derive(kagari_derive::KagariSchema)]
/// struct Invalid { #[serde(skip_serializing_if = "Option::is_none")] value: Option<i32> }
/// ```
///
/// Flattening:
///
/// ```compile_fail
/// #[derive(kagari_derive::KagariSchema)]
/// struct Invalid { #[serde(flatten)] value: i32 }
/// ```
///
/// Asymmetric names:
///
/// ```compile_fail
/// #[derive(kagari_derive::KagariSchema)]
/// #[serde(rename(serialize = "Out", deserialize = "In"))]
/// struct Invalid { value: i32 }
/// ```
///
/// Tagged enum representations:
///
/// ```compile_fail
/// #[derive(kagari_derive::KagariSchema)]
/// #[serde(tag = "kind", content = "value")]
/// enum Invalid { Value(i32) }
/// ```
///
/// Custom attribute serializers:
///
/// ```compile_fail
/// #[derive(kagari_derive::KagariSchema)]
/// struct Invalid { #[serde(with = "custom")] value: i32 }
/// ```
///
/// Borrowed fields:
///
/// ```compile_fail
/// #[derive(kagari_derive::KagariSchema)]
/// struct Invalid { #[serde(borrow)] value: &'static str }
/// ```
///
/// Remote schemas:
///
/// ```compile_fail
/// #[derive(kagari_derive::KagariSchema)]
/// #[serde(remote = "Foreign")]
/// struct Invalid { value: i32 }
/// ```
///
/// Opaque object ids:
///
/// ```compile_fail
/// kagari_runtime::native_data! { struct Payload { value: i32 } }
/// #[derive(kagari_derive::KagariSchema)]
/// struct Invalid { value: kagari_runtime::native::payload::Opaque<Payload> }
/// ```
///
/// Unsupported scalar schemas:
///
/// ```compile_fail
/// #[derive(kagari_derive::KagariSchema)]
/// struct Invalid { value: i128 }
/// ```
///
/// Borrowed deserialization outputs:
///
/// ```compile_fail
/// use kagari_runtime::native::{
///     binding::NativeResult, catalog::DeclarationCatalog, conversion::{FromKagari, KagariType},
///     schema::TypeRegistration, serde::{KagariSchema, Serde}, types::Type,
/// };
/// #[derive(serde::Deserialize)]
/// struct Borrowed<'a> { #[serde(borrow)] value: &'a str }
/// impl KagariSchema for Borrowed<'static> {
///     fn schema_type(catalog: &DeclarationCatalog) -> NativeResult<Type> {
///         String::kagari_type(catalog)
///     }
///     fn register_schema(registration: &mut TypeRegistration<'_>) -> NativeResult<Type> {
///         String::register_type(registration)
///     }
/// }
/// fn require_owned<T: FromKagari>() {}
/// fn main() { require_owned::<Serde<Borrowed<'static>>>(); }
/// ```
///
pub trait KagariSchema: 'static {
    fn schema_type(catalog: &DeclarationCatalog) -> NativeResult<Type>;

    fn register_schema(registration: &mut TypeRegistration<'_>) -> NativeResult<Type>;
}

impl<T: KagariSchema> KagariType for Serde<T> {
    fn kagari_type(catalog: &DeclarationCatalog) -> NativeResult<Type> {
        T::schema_type(catalog)
    }

    fn register_type(registration: &mut TypeRegistration<'_>) -> NativeResult<Type> {
        T::register_schema(registration)
    }
}

impl<T: KagariSchema + Serialize> IntoKagari for Serde<T> {
    fn into_kagari(
        self,
        cx: &mut ConversionContext<'_>,
        expected: &TypeArgument,
    ) -> NativeResult<Value> {
        self.0.serialize(Serializer::new(cx, expected))
    }
}

impl<T: KagariSchema + DeserializeOwned> FromKagari for Serde<T> {
    fn from_kagari(
        cx: &mut ConversionContext<'_>,
        expected: &TypeArgument,
        value: &Value,
    ) -> NativeResult<Self> {
        T::deserialize(Deserializer::new(cx, expected, *value)).map(Self)
    }
}

impl ser::Error for RuntimeError {
    fn custom<T: fmt::Display>(message: T) -> Self {
        Self::module_validation(format!("Serde conversion: {message}"))
    }
}

impl de::Error for RuntimeError {
    fn custom<T: fmt::Display>(message: T) -> Self {
        Self::module_validation(format!("Serde conversion: {message}"))
    }
}

fn unsupported(model: &str) -> RuntimeError {
    RuntimeError::module_validation(format!("unsupported Serde representation: {model}"))
}
