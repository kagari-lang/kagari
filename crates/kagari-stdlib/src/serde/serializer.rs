//! Direct Serde value construction; child conversions use the normal rooted scope.
use crate::serde::serializer::compound::{Pack, Record};
pub(super) mod compound;
mod text;
use super::unsupported;
use kagari_runtime::{
    error::RuntimeError,
    frame::types::arguments::TypeArgument,
    native::{
        binding::NativeResult,
        catalog::DeclarationCatalog,
        conversion::{
            IntoKagari, KagariType, context::ConversionContext, structures::StructEncoder,
        },
        types::Type,
        value_handle::ScriptValue,
    },
    value::Value,
};
use serde::{
    Serialize,
    ser::{Impossible, Serializer as SerdeSerializer},
};
use std::fmt::Display;

pub(super) struct Serializer<'scope, 'runtime> {
    cx: &'scope mut ConversionContext<'runtime>,
    expected: &'scope TypeArgument,
}

struct SerializeValue<'a, T: ?Sized>(&'a T);

pub(super) fn retain_result(
    cx: &mut ConversionContext<'_>,
    expected: &TypeArgument,
    value: Value,
) -> NativeResult<ScriptValue> {
    let root = unsafe {
        // SAFETY: The private serializer transfers constructed transport directly; converted children remain rooted.
        cx.runtime().gc().root_value_transport(value)
    }
    .ok_or_else(|| RuntimeError::module_validation("Serde result retention"))?;
    cx.retain(expected, root)
}

impl<T: ?Sized> KagariType for SerializeValue<'_, T> {
    fn check_type(cx: &ConversionContext<'_>, expected: &TypeArgument) -> NativeResult<()> {
        cx.check_type::<ScriptValue>(expected)
    }

    fn kagari_type(_: &DeclarationCatalog) -> NativeResult<Type> {
        Err(unsupported("serialization proxy has no standalone schema"))
    }
}

impl<T: Serialize + ?Sized> IntoKagari for SerializeValue<'_, T> {
    fn into_kagari(
        self,
        cx: &mut ConversionContext<'_>,
        expected: &TypeArgument,
    ) -> NativeResult<ScriptValue> {
        self.0.serialize(Serializer::new(cx, expected))
    }
}

pub(super) fn encode<T: Serialize + ?Sized>(
    cx: &mut ConversionContext<'_>,
    expected: &TypeArgument,
    value: &T,
) -> NativeResult<ScriptValue> {
    // The enclosing registered schema supplies this child's type. The actual
    // serialization model and resulting value still check against it on access.
    cx.encode_value(expected, SerializeValue(value))
}

impl<'scope, 'runtime> Serializer<'scope, 'runtime> {
    pub(super) fn new(
        cx: &'scope mut ConversionContext<'runtime>,
        expected: &'scope TypeArgument,
    ) -> Self {
        Self { cx, expected }
    }

    fn scalar<T: IntoKagari>(self, value: T) -> NativeResult<ScriptValue> {
        self.cx.encode_value(self.expected, value)
    }
}

macro_rules! scalar {
    ($($method:ident: $ty:ty),+) => { $(
        fn $method(self, value: $ty) -> NativeResult<ScriptValue> { self.scalar(value) }
    )+ };
}

impl<'scope, 'runtime> SerdeSerializer for Serializer<'scope, 'runtime> {
    type Ok = ScriptValue;
    type Error = RuntimeError;
    type SerializeSeq = Pack<'scope, 'runtime>;
    type SerializeTuple = Pack<'scope, 'runtime>;
    type SerializeTupleStruct = Impossible<ScriptValue, RuntimeError>;
    type SerializeTupleVariant = Pack<'scope, 'runtime>;
    type SerializeMap = Impossible<ScriptValue, RuntimeError>;
    type SerializeStruct = Record<'scope, 'runtime>;
    type SerializeStructVariant = Impossible<ScriptValue, RuntimeError>;

    scalar!(serialize_bool: bool, serialize_i8: i8, serialize_i16: i16,
        serialize_i32: i32, serialize_i64: i64, serialize_u8: u8, serialize_u16: u16,
        serialize_u32: u32, serialize_u64: u64, serialize_f32: f32, serialize_f64: f64);

    fn serialize_i128(self, _: i128) -> NativeResult<ScriptValue> {
        Err(unsupported("i128"))
    }

    fn serialize_u128(self, _: u128) -> NativeResult<ScriptValue> {
        Err(unsupported("u128"))
    }

    fn serialize_char(self, value: char) -> NativeResult<ScriptValue> {
        self.scalar(value)
    }

    fn serialize_str(self, value: &str) -> NativeResult<ScriptValue> {
        self.cx.check_type::<String>(self.expected)?;
        self.cx.charge_string(value.len())?;
        let mut text = String::new();
        text.try_reserve_exact(value.len())
            .map_err(|_| RuntimeError::resource_limit("Serde string capacity"))?;
        text.push_str(value);
        let value = unsafe { self.cx.runtime().gc().alloc_string_transport(text) }?;
        retain_result(self.cx, self.expected, value)
    }

    fn collect_str<T: Display + ?Sized>(self, value: &T) -> NativeResult<ScriptValue> {
        self.cx.check_type::<String>(self.expected)?;
        let value = text::collect(self.cx, value)?;
        retain_result(self.cx, self.expected, value)
    }

    fn serialize_bytes(self, _: &[u8]) -> NativeResult<ScriptValue> {
        Err(unsupported("byte encoding; use a sequence schema"))
    }

    fn serialize_none(self) -> NativeResult<ScriptValue> {
        compound::check_standard_enum(self.cx, self.expected, false)?;
        Pack::variant(self.cx, self.expected, "None", 0)?.finish()
    }

    fn serialize_some<T: Serialize + ?Sized>(self, value: &T) -> NativeResult<ScriptValue> {
        compound::check_standard_enum(self.cx, self.expected, false)?;
        let mut pack = Pack::variant(self.cx, self.expected, "Some", 1)?;
        pack.push(value)?;
        pack.finish()
    }

    fn serialize_unit(self) -> NativeResult<ScriptValue> {
        self.scalar(())
    }

    fn serialize_unit_struct(self, _: &'static str) -> NativeResult<ScriptValue> {
        Err(unsupported("unit struct"))
    }

    fn serialize_unit_variant(
        self,
        _: &'static str,
        _: u32,
        variant: &'static str,
    ) -> NativeResult<ScriptValue> {
        Pack::variant(self.cx, self.expected, variant, 0)?.finish()
    }

    fn serialize_newtype_struct<T: Serialize + ?Sized>(
        self,
        _: &'static str,
        _: &T,
    ) -> NativeResult<ScriptValue> {
        Err(unsupported("newtype struct"))
    }

    fn serialize_newtype_variant<T: Serialize + ?Sized>(
        self,
        _: &'static str,
        _: u32,
        variant: &'static str,
        value: &T,
    ) -> NativeResult<ScriptValue> {
        let mut pack = Pack::variant(self.cx, self.expected, variant, 1)?;
        pack.push(value)?;
        pack.finish()
    }

    fn serialize_seq(self, len: Option<usize>) -> NativeResult<Self::SerializeSeq> {
        Pack::sequence(self.cx, self.expected, len)
    }

    fn serialize_tuple(self, len: usize) -> NativeResult<Self::SerializeTuple> {
        Pack::tuple(self.cx, self.expected, len)
    }

    fn serialize_tuple_struct(
        self,
        _: &'static str,
        _: usize,
    ) -> NativeResult<Self::SerializeTupleStruct> {
        Err(unsupported("tuple struct"))
    }

    fn serialize_tuple_variant(
        self,
        _: &'static str,
        _: u32,
        variant: &'static str,
        len: usize,
    ) -> NativeResult<Self::SerializeTupleVariant> {
        Pack::variant(self.cx, self.expected, variant, len)
    }

    fn serialize_map(self, _: Option<usize>) -> NativeResult<Self::SerializeMap> {
        Err(unsupported("map"))
    }

    fn serialize_struct(self, _: &'static str, len: usize) -> NativeResult<Self::SerializeStruct> {
        let encoder: StructEncoder<'scope, 'runtime> = self.cx.write_struct(self.expected)?;
        Ok(Record::new(encoder, len))
    }

    fn serialize_struct_variant(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: usize,
    ) -> NativeResult<Self::SerializeStructVariant> {
        Err(unsupported("record variant"))
    }
}
