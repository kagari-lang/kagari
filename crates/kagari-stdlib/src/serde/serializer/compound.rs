//! Prepared compound descriptors retain normal roots until publication.
use crate::{
    collections::vector::VecValues,
    serde::{serializer::encode, unsupported},
};
use kagari_runtime::{
    error::RuntimeError,
    frame::types::arguments::TypeArgument,
    module::EnumVariantRef,
    native::{
        binding::NativeResult,
        conversion::{
            FromKagari, IntoKagari, context::ConversionContext, structures::StructEncoder,
        },
        value_handle::ScriptValue,
    },
    value::{EnumTag, Value},
};
use kagari_types::{language::binding, ty::Ty};
use serde::{
    Serialize,
    ser::{SerializeSeq, SerializeStruct, SerializeTuple, SerializeTupleVariant},
};

pub(crate) fn check_standard_enum(
    cx: &ConversionContext<'_>,
    expected: &TypeArgument,
    result: bool,
) -> NativeResult<()> {
    let declaration = if result {
        binding::result_declaration()
    } else {
        binding::option_declaration()
    };
    cx.type_reference(&declaration)?;
    if matches!(expected.ty(), Ty::Enum(nominal) if expected.definitions().lookup(&declaration) == Some(nominal.declaration) && nominal.arguments.len() == if result { 2 } else { 1 })
    {
        Ok(())
    } else {
        Err(unsupported("standard enum schema mismatch"))
    }
}

pub(crate) fn check_sequence(expected: &TypeArgument) -> NativeResult<()> {
    if matches!(expected.ty(), Ty::NativeObject(nominal) if binding::matches(&nominal.declaration, &crate::identity::type_id("Vec", kagari_common::identity::DefinitionKind::AssociatedType), expected.definitions()) && nominal.arguments.len() == 1)
    {
        Ok(())
    } else {
        Err(unsupported("sequence schema mismatch"))
    }
}

enum PackKind {
    Sequence(TypeArgument),
    Tuple(Vec<TypeArgument>),
    Variant(EnumVariantRef, Vec<TypeArgument>),
}

pub(crate) struct Pack<'scope, 'runtime> {
    cx: &'scope mut ConversionContext<'runtime>,
    expected: TypeArgument,
    kind: PackKind,
    values: Vec<Value>,
    length: Option<usize>,
}

impl<'scope, 'runtime> Pack<'scope, 'runtime> {
    fn new(
        cx: &'scope mut ConversionContext<'runtime>,
        expected: &TypeArgument,
        kind: PackKind,
        length: Option<usize>,
    ) -> NativeResult<Self> {
        cx.check_elements(length.unwrap_or(0))?;
        let mut values = Vec::new();
        values
            .try_reserve(length.unwrap_or(0))
            .map_err(|_| RuntimeError::resource_limit("Serde compound capacity"))?;
        Ok(Self {
            cx,
            expected: expected.clone(),
            kind,
            values,
            length,
        })
    }

    pub(crate) fn sequence(
        cx: &'scope mut ConversionContext<'runtime>,
        expected: &TypeArgument,
        length: Option<usize>,
    ) -> NativeResult<Self> {
        check_sequence(expected)?;
        let element = cx.parameter(expected, 0)?;
        Self::new(cx, expected, PackKind::Sequence(element), length)
    }

    pub(crate) fn tuple(
        cx: &'scope mut ConversionContext<'runtime>,
        expected: &TypeArgument,
        length: usize,
    ) -> NativeResult<Self> {
        if !matches!(expected.ty(), Ty::Tuple(fields) if fields.len() == length) {
            return Err(unsupported("tuple schema mismatch"));
        }
        cx.check_elements(length)?;
        let fields = (0..length)
            .map(|i| cx.parameter(expected, i))
            .collect::<NativeResult<_>>()?;
        Self::new(cx, expected, PackKind::Tuple(fields), Some(length))
    }

    pub(crate) fn variant(
        cx: &'scope mut ConversionContext<'runtime>,
        expected: &TypeArgument,
        variant: &str,
        length: usize,
    ) -> NativeResult<Self> {
        let layout = cx.declared_variant(expected, variant)?;
        let fields = cx.variant_fields(&layout)?;
        if fields.len() != length {
            return Err(unsupported("variant payload schema mismatch"));
        }
        Self::new(
            cx,
            expected,
            PackKind::Variant(layout, fields),
            Some(length),
        )
    }

    pub(crate) fn push<T: Serialize + ?Sized>(&mut self, value: &T) -> NativeResult<()> {
        let slot = self.values.len();
        if self.length.is_some_and(|length| slot >= length) {
            return Err(unsupported("compound has extra elements"));
        }
        let expected = match &self.kind {
            PackKind::Sequence(element) => element,
            PackKind::Tuple(fields) | PackKind::Variant(_, fields) => fields
                .get(slot)
                .ok_or_else(|| unsupported("payload has extra elements"))?,
        };
        let value = encode(self.cx, expected, value)
            .map_err(|error| error.at_conversion(format_args!("element {slot}")))?;
        self.values
            .try_reserve(1)
            .map_err(|_| RuntimeError::resource_limit("Serde compound capacity"))?;
        self.values.push(value);
        Ok(())
    }

    pub(crate) fn finish(self) -> NativeResult<Value> {
        if self
            .length
            .is_some_and(|length| length != self.values.len())
        {
            return Err(unsupported("compound has missing elements"));
        }
        match self.kind {
            PackKind::Sequence(_) => {
                let item = self.cx.parameter(&self.expected, 0)?;
                let values = self
                    .values
                    .iter()
                    .map(|value| ScriptValue::from_kagari(self.cx, &item, value))
                    .collect::<NativeResult<Vec<_>>>()?;
                VecValues(values).into_kagari(self.cx, &self.expected)
            }
            PackKind::Tuple(_) => self.cx.runtime().gc().alloc_tuple(self.values),
            PackKind::Variant(layout, _) => self
                .cx
                .runtime()
                .alloc_enum(EnumTag::Declared(layout), self.values)
                .map(Value::Enum),
        }
    }
}

macro_rules! pack_trait {
    ($trait:ident, $method:ident) => {
        impl $trait for Pack<'_, '_> {
            type Ok = Value;
            type Error = RuntimeError;

            fn $method<T: Serialize + ?Sized>(&mut self, value: &T) -> NativeResult<()> {
                self.push(value)
            }

            fn end(self) -> NativeResult<Value> {
                self.finish()
            }
        }
    };
}

pack_trait!(SerializeSeq, serialize_element);
pack_trait!(SerializeTuple, serialize_element);
pack_trait!(SerializeTupleVariant, serialize_field);

pub(crate) struct Record<'scope, 'runtime> {
    encoder: StructEncoder<'scope, 'runtime>,
    remaining: usize,
}

impl<'scope, 'runtime> Record<'scope, 'runtime> {
    pub(crate) fn new(encoder: StructEncoder<'scope, 'runtime>, length: usize) -> Self {
        Self {
            encoder,
            remaining: length,
        }
    }
}

impl SerializeStruct for Record<'_, '_> {
    type Ok = Value;
    type Error = RuntimeError;

    fn serialize_field<T: Serialize + ?Sized>(
        &mut self,
        key: &'static str,
        value: &T,
    ) -> NativeResult<()> {
        if self.remaining == 0 {
            return Err(unsupported("struct has extra fields"));
        }
        self.encoder
            .field_with(key, |cx, expected| encode(cx, expected, value))?;
        self.remaining -= 1;
        Ok(())
    }

    fn end(self) -> NativeResult<Value> {
        if self.remaining != 0 {
            return Err(unsupported("struct has missing fields"));
        }
        self.encoder.finish()
    }
}
