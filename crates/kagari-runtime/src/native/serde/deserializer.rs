//! Owned Serde visitors never retain references into the script heap.
mod access;
use super::{
    serializer::compound::{check_sequence, check_standard_enum},
    unsupported,
};
use crate::{
    error::RuntimeError,
    frame::types::arguments::TypeArgument,
    native::{
        binding::NativeResult,
        conversion::{FromKagari, context::ConversionContext},
        serde::deserializer::access::{Fields, Sequence, Variant},
    },
    value::Value,
};
use kagari_types::{scalar::BuiltinType, ty::Ty};
use serde::{
    Deserializer as SerdeDeserializer,
    de::{DeserializeSeed, Visitor},
};

pub(super) struct Deserializer<'scope, 'runtime> {
    cx: &'scope mut ConversionContext<'runtime>,
    expected: &'scope TypeArgument,
    value: Value,
}

pub(super) fn decode<'de, S: DeserializeSeed<'de>>(
    cx: &mut ConversionContext<'_>,
    expected: &TypeArgument,
    value: &Value,
    seed: S,
) -> NativeResult<S::Value> {
    cx.decode_scope(expected, value, |cx| {
        seed.deserialize(Deserializer::new(cx, expected, *value))
    })
}

impl<'scope, 'runtime> Deserializer<'scope, 'runtime> {
    pub(super) fn new(
        cx: &'scope mut ConversionContext<'runtime>,
        expected: &'scope TypeArgument,
        value: Value,
    ) -> Self {
        Self {
            cx,
            expected,
            value,
        }
    }

    fn scalar<T: FromKagari>(&mut self) -> NativeResult<T> {
        self.cx.check_type::<T>(self.expected)?;
        T::from_kagari(self.cx, self.expected, &self.value)
    }
}

macro_rules! scalar {
    ($($method:ident: $ty:ty => $visit:ident),+) => { $(
        fn $method<V: Visitor<'de>>(mut self, visitor: V) -> NativeResult<V::Value> {
            visitor.$visit(self.scalar::<$ty>()?)
        }
    )+ };
}

impl<'de> SerdeDeserializer<'de> for Deserializer<'_, '_> {
    type Error = RuntimeError;

    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> NativeResult<V::Value> {
        match self.expected.ty() {
            Ty::Builtin(BuiltinType::Unit) => self.deserialize_unit(visitor),
            Ty::Builtin(BuiltinType::Bool) => self.deserialize_bool(visitor),
            Ty::Builtin(BuiltinType::I8) => self.deserialize_i8(visitor),
            Ty::Builtin(BuiltinType::I16) => self.deserialize_i16(visitor),
            Ty::Builtin(BuiltinType::I32) => self.deserialize_i32(visitor),
            Ty::Builtin(BuiltinType::I64) => self.deserialize_i64(visitor),
            Ty::Builtin(BuiltinType::U8) => self.deserialize_u8(visitor),
            Ty::Builtin(BuiltinType::U16) => self.deserialize_u16(visitor),
            Ty::Builtin(BuiltinType::U32) => self.deserialize_u32(visitor),
            Ty::Builtin(BuiltinType::U64) => self.deserialize_u64(visitor),
            Ty::Builtin(BuiltinType::F32) => self.deserialize_f32(visitor),
            Ty::Builtin(BuiltinType::F64) => self.deserialize_f64(visitor),
            Ty::Builtin(BuiltinType::String) => self.deserialize_string(visitor),
            Ty::Tuple(fields) => {
                let count = fields.len();
                self.deserialize_tuple(count, visitor)
            }
            Ty::Struct(_) => self.deserialize_struct("", &[], visitor),
            Ty::Enum(_) => self.deserialize_enum("", &[], visitor),
            Ty::NativeObject(_) => self.deserialize_seq(visitor),
            _ => Err(unsupported("dynamic value")),
        }
    }

    scalar!(deserialize_bool: bool => visit_bool, deserialize_i8: i8 => visit_i8,
        deserialize_i16: i16 => visit_i16, deserialize_i32: i32 => visit_i32,
        deserialize_i64: i64 => visit_i64, deserialize_u8: u8 => visit_u8,
        deserialize_u16: u16 => visit_u16, deserialize_u32: u32 => visit_u32,
        deserialize_u64: u64 => visit_u64, deserialize_f32: f32 => visit_f32,
        deserialize_f64: f64 => visit_f64);

    fn deserialize_i128<V: Visitor<'de>>(self, _: V) -> NativeResult<V::Value> {
        Err(unsupported("i128"))
    }

    fn deserialize_u128<V: Visitor<'de>>(self, _: V) -> NativeResult<V::Value> {
        Err(unsupported("u128"))
    }

    fn deserialize_char<V: Visitor<'de>>(self, _: V) -> NativeResult<V::Value> {
        Err(unsupported("char"))
    }

    fn deserialize_str<V: Visitor<'de>>(self, visitor: V) -> NativeResult<V::Value> {
        self.deserialize_string(visitor)
    }

    fn deserialize_string<V: Visitor<'de>>(mut self, visitor: V) -> NativeResult<V::Value> {
        visitor.visit_string(self.scalar::<String>()?)
    }

    fn deserialize_bytes<V: Visitor<'de>>(self, _: V) -> NativeResult<V::Value> {
        Err(unsupported("bytes"))
    }

    fn deserialize_byte_buf<V: Visitor<'de>>(self, _: V) -> NativeResult<V::Value> {
        Err(unsupported("byte buffer"))
    }

    fn deserialize_option<V: Visitor<'de>>(self, visitor: V) -> NativeResult<V::Value> {
        check_standard_enum(self.cx, self.expected, false)?;
        let variant = Variant::new(self.cx, self.expected, &self.value)?;
        match variant.name.as_str() {
            "None" if variant.count() == 0 => visitor.visit_none(),
            "Some" if variant.count() == 1 => variant.some(visitor),
            _ => Err(unsupported("Option payload")),
        }
    }

    fn deserialize_unit<V: Visitor<'de>>(mut self, visitor: V) -> NativeResult<V::Value> {
        self.scalar::<()>()?;
        visitor.visit_unit()
    }

    fn deserialize_unit_struct<V: Visitor<'de>>(
        self,
        _: &'static str,
        _: V,
    ) -> NativeResult<V::Value> {
        Err(unsupported("unit struct"))
    }

    fn deserialize_newtype_struct<V: Visitor<'de>>(
        self,
        _: &'static str,
        _: V,
    ) -> NativeResult<V::Value> {
        Err(unsupported("newtype struct"))
    }

    fn deserialize_seq<V: Visitor<'de>>(self, visitor: V) -> NativeResult<V::Value> {
        check_sequence(self.expected)?;
        let (roots, count) = self.cx.snapshot_sequence(&self.value)?;
        let field = self.cx.parameter(self.expected, 0)?;
        Sequence::visit_repeated(self.cx, roots, count, field, visitor)
    }

    fn deserialize_tuple<V: Visitor<'de>>(self, len: usize, visitor: V) -> NativeResult<V::Value> {
        if !matches!(self.expected.ty(), Ty::Tuple(fields) if fields.len() == len) {
            return Err(unsupported("tuple schema mismatch"));
        }
        self.cx.check_elements(len)?;
        let Value::Tuple(id) = self.value else {
            return Err(unsupported("tuple value"));
        };
        let values = self
            .cx
            .runtime()
            .gc()
            .tuple(id)
            .ok_or_else(|| unsupported("tuple identity"))?
            .to_vec();
        if values.len() != len {
            return Err(unsupported("tuple arity"));
        }
        let roots = self
            .cx
            .runtime()
            .gc()
            .root_execution_values(values)
            .ok_or_else(|| unsupported("tuple snapshot roots"))?;
        let fields = (0..len)
            .map(|i| self.cx.parameter(self.expected, i))
            .collect::<NativeResult<_>>()?;
        Sequence::visit(self.cx, roots, fields, visitor)
    }

    fn deserialize_tuple_struct<V: Visitor<'de>>(
        self,
        _: &'static str,
        _: usize,
        _: V,
    ) -> NativeResult<V::Value> {
        Err(unsupported("tuple struct"))
    }

    fn deserialize_map<V: Visitor<'de>>(self, _: V) -> NativeResult<V::Value> {
        Err(unsupported("map"))
    }

    fn deserialize_struct<V: Visitor<'de>>(
        self,
        _: &'static str,
        _: &'static [&'static str],
        visitor: V,
    ) -> NativeResult<V::Value> {
        let decoder = self.cx.read_struct(self.expected, &self.value)?;
        Fields::visit(decoder, visitor)
    }

    fn deserialize_enum<V: Visitor<'de>>(
        self,
        _: &'static str,
        _: &'static [&'static str],
        visitor: V,
    ) -> NativeResult<V::Value> {
        visitor.visit_enum(Variant::new(self.cx, self.expected, &self.value)?)
    }

    fn deserialize_identifier<V: Visitor<'de>>(self, visitor: V) -> NativeResult<V::Value> {
        self.deserialize_string(visitor)
    }

    fn deserialize_ignored_any<V: Visitor<'de>>(self, _: V) -> NativeResult<V::Value> {
        Err(unsupported("ignored serialized field"))
    }
}
