//! A compound owns one local suffix; finish promotes only its completed value.
use crate::{
    error::{RuntimeError, context::ErrorLocation},
    frame::types::arguments::TypeArgument,
    module::EnumVariantRef,
    native::{
        binding::NativeResult,
        conversion::{
            context::{ConversionContext, Frame, inputs::ValueInput},
            structures::StructEncoder,
        },
        serde::{serializer::encode, unsupported},
    },
    value::view::ValueView,
};

use kagari_types::{language::binding, ty::Ty};
use serde::{
    Serialize,
    ser::{SerializeStruct, SerializeTuple, SerializeTupleVariant},
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
    cx.runtime()
        .native_entries
        .catalog
        .type_reference(&declaration)?;
    if matches!(expected.ty(), Ty::Enum(nominal) if expected.definitions().lookup(&declaration) == Some(nominal.declaration) && nominal.arguments.len() == if result { 2 } else { 1 })
    {
        Ok(())
    } else {
        Err(unsupported("standard enum schema mismatch"))
    }
}

enum PackKind {
    Tuple(Vec<TypeArgument>),
    Variant(EnumVariantRef, Vec<TypeArgument>),
}

impl PackKind {
    fn fields(&self) -> &[TypeArgument] {
        match self {
            Self::Tuple(fields) | Self::Variant(_, fields) => fields,
        }
    }
}

pub(crate) struct Pack<'scope, 'runtime> {
    cx: Frame<'scope, 'runtime>,
    kind: PackKind,
    values: Vec<ValueView<'runtime>>,
}

impl<'scope, 'runtime> Pack<'scope, 'runtime> {
    fn new(cx: &'scope mut ConversionContext<'runtime>, kind: PackKind) -> NativeResult<Self> {
        cx.check_elements(kind.fields().len())?;
        let mut values = Vec::new();
        values
            .try_reserve_exact(kind.fields().len())
            .map_err(|_| RuntimeError::resource_limit("Serde compound capacity"))?;
        Ok(Self {
            cx: cx.scratch()?,
            kind,
            values,
        })
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
        Self::new(cx, PackKind::Tuple(fields))
    }

    pub(crate) fn variant(
        cx: &'scope mut ConversionContext<'runtime>,
        expected: &TypeArgument,
        variant: &str,
        length: usize,
    ) -> NativeResult<Self> {
        let layout = cx
            .runtime()
            .declared_enum_variant(cx.owner(), expected, variant)?;
        let fields = cx.variant_fields(&layout)?;
        if fields.len() != length {
            return Err(unsupported("variant payload schema mismatch"));
        }
        Self::new(cx, PackKind::Variant(layout, fields))
    }

    pub(crate) fn push<T: Serialize + ?Sized>(&mut self, value: &T) -> NativeResult<()> {
        let slot = self.values.len();
        let expected = self
            .kind
            .fields()
            .get(slot)
            .ok_or_else(|| unsupported("compound has extra elements"))?;
        let value = encode(&mut self.cx, expected, value)
            .map_err(|error| error.at(ErrorLocation::Element(slot)))?;
        self.values
            .try_reserve(1)
            .map_err(|_| RuntimeError::resource_limit("Serde compound capacity"))?;
        self.values.push(value);
        Ok(())
    }

    pub(crate) fn finish(mut self) -> NativeResult<ValueView<'runtime>> {
        if self.kind.fields().len() != self.values.len() {
            return Err(unsupported("compound has missing elements"));
        }
        let value = match self.kind {
            PackKind::Tuple(_) => self.cx.allocate_tuple_values(&self.values)?,
            PackKind::Variant(layout, _) => self
                .cx
                .allocate_enum_inputs(layout, self.values.into_iter().map(ValueInput::View))?,
        };
        self.cx.preserve(value)
    }
}

macro_rules! pack_trait {
    ($trait:ident, $method:ident) => {
        impl<'runtime> $trait for Pack<'_, 'runtime> {
            type Ok = ValueView<'runtime>;
            type Error = RuntimeError;

            fn $method<T: Serialize + ?Sized>(&mut self, value: &T) -> NativeResult<()> {
                self.push(value)
            }

            fn end(self) -> NativeResult<ValueView<'runtime>> {
                self.finish()
            }
        }
    };
}

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

impl<'runtime> SerializeStruct for Record<'_, 'runtime> {
    type Ok = ValueView<'runtime>;
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

    fn end(self) -> NativeResult<ValueView<'runtime>> {
        if self.remaining != 0 {
            return Err(unsupported("struct has missing fields"));
        }
        self.encoder.finish()
    }
}
