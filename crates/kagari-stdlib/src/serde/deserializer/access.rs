//! Immediate child snapshots remain rooted while arbitrary Serde visitors run.
use crate::serde::{
    deserializer::{Deserializer, decode},
    unsupported,
};
use kagari_runtime::{
    error::RuntimeError,
    frame::types::arguments::TypeArgument,
    gc::roots::RootSet,
    native::{
        binding::NativeResult,
        conversion::{context::ConversionContext, structures::StructDecoder},
    },
    value::Value,
};
use serde::de::{
    DeserializeSeed, EnumAccess, MapAccess, SeqAccess, VariantAccess, Visitor,
    value::StringDeserializer,
};

pub(super) struct Fields<'scope, 'runtime> {
    decoder: StructDecoder<'scope, 'runtime>,
    names: Vec<String>,
    next: usize,
    pending: bool,
}

impl<'scope, 'runtime> Fields<'scope, 'runtime> {
    pub(super) fn visit<'de, V: Visitor<'de>>(
        decoder: StructDecoder<'scope, 'runtime>,
        visitor: V,
    ) -> NativeResult<V::Value> {
        let names = decoder.names();
        let mut fields = Self {
            decoder,
            names,
            next: 0,
            pending: false,
        };
        let result = visitor.visit_map(&mut fields)?;
        if fields.pending || fields.next != fields.names.len() {
            return Err(unsupported("visitor left unread struct fields"));
        }
        Ok(result)
    }
}

impl<'de> MapAccess<'de> for Fields<'_, '_> {
    type Error = RuntimeError;

    fn next_key_seed<K: DeserializeSeed<'de>>(
        &mut self,
        seed: K,
    ) -> NativeResult<Option<K::Value>> {
        if self.pending {
            return Err(unsupported(
                "struct visitor requested a key before its value",
            ));
        }
        let Some(name) = self.names.get(self.next) else {
            return Ok(None);
        };
        self.pending = true;
        seed.deserialize(StringDeserializer::<RuntimeError>::new(name.clone()))
            .map(Some)
    }

    fn next_value_seed<V: DeserializeSeed<'de>>(&mut self, seed: V) -> NativeResult<V::Value> {
        if !self.pending {
            return Err(unsupported(
                "struct visitor requested a value without a key",
            ));
        }
        let name = &self.names[self.next];
        let result = self.decoder.field_with(name, |cx, expected, value| {
            decode(cx, expected, value, seed)
        })?;
        self.next += 1;
        self.pending = false;
        Ok(result)
    }

    fn size_hint(&self) -> Option<usize> {
        Some(self.names.len() - self.next)
    }
}

enum SequenceTypes {
    Repeated(TypeArgument, usize),
    Fields(Vec<TypeArgument>),
}

pub(super) struct Sequence<'scope, 'runtime> {
    cx: &'scope mut ConversionContext<'runtime>,
    roots: RootSet,
    fields: SequenceTypes,
    next: usize,
}

impl<'scope, 'runtime> Sequence<'scope, 'runtime> {
    fn count(&self) -> usize {
        match &self.fields {
            SequenceTypes::Repeated(_, count) => *count,
            SequenceTypes::Fields(fields) => fields.len(),
        }
    }

    fn run<'de, V: Visitor<'de>>(mut self, visitor: V) -> NativeResult<V::Value> {
        let result = visitor.visit_seq(&mut self)?;
        if self.next != self.count() {
            return Err(unsupported("visitor left unread sequence elements"));
        }
        Ok(result)
    }

    pub(super) fn visit<'de, V: Visitor<'de>>(
        cx: &'scope mut ConversionContext<'runtime>,
        roots: RootSet,
        fields: Vec<TypeArgument>,
        visitor: V,
    ) -> NativeResult<V::Value> {
        Self {
            cx,
            roots,
            fields: SequenceTypes::Fields(fields),
            next: 0,
        }
        .run(visitor)
    }

    pub(super) fn visit_repeated<'de, V: Visitor<'de>>(
        cx: &'scope mut ConversionContext<'runtime>,
        roots: RootSet,
        count: usize,
        field: TypeArgument,
        visitor: V,
    ) -> NativeResult<V::Value> {
        Self {
            cx,
            roots,
            fields: SequenceTypes::Repeated(field, count),
            next: 0,
        }
        .run(visitor)
    }
}

impl<'de> SeqAccess<'de> for Sequence<'_, '_> {
    type Error = RuntimeError;

    fn next_element_seed<S: DeserializeSeed<'de>>(
        &mut self,
        seed: S,
    ) -> NativeResult<Option<S::Value>> {
        if self.next == self.count() {
            return Ok(None);
        }
        let field = match &self.fields {
            SequenceTypes::Repeated(field, _) => field,
            SequenceTypes::Fields(fields) => &fields[self.next],
        };
        let value = self
            .roots
            .get(self.cx.runtime().gc(), self.next)
            .ok_or_else(|| unsupported("expired child snapshot"))?;
        let result = decode(self.cx, field, &value, seed)
            .map_err(|error| error.at_conversion(format_args!("element {}", self.next)))?;
        self.next += 1;
        Ok(Some(result))
    }

    fn size_hint(&self) -> Option<usize> {
        Some(self.count() - self.next)
    }
}

pub(super) struct Variant<'scope, 'runtime> {
    cx: &'scope mut ConversionContext<'runtime>,
    pub(super) name: String,
    roots: RootSet,
    fields: Vec<TypeArgument>,
}

impl<'scope, 'runtime> Variant<'scope, 'runtime> {
    pub(super) fn new(
        cx: &'scope mut ConversionContext<'runtime>,
        expected: &TypeArgument,
        value: &Value,
    ) -> NativeResult<Self> {
        let layout = cx.variant_layout(expected, value)?;
        let name = cx.variant_name(expected, value)?;
        let fields = cx.variant_fields(&layout)?;
        let Value::Enum(id) = value else {
            return Err(unsupported("enum value"));
        };
        let snapshot = cx
            .runtime()
            .gc()
            .enum_snapshot(*id)
            .ok_or_else(|| unsupported("enum snapshot"))?;
        if fields.len() != snapshot.fields.len() {
            return Err(unsupported("enum payload arity"));
        }
        let roots = cx
            .runtime()
            .gc()
            .root_execution_values(snapshot.fields)
            .ok_or_else(|| unsupported("enum snapshot roots"))?;
        Ok(Self {
            cx,
            name,
            roots,
            fields,
        })
    }

    pub(super) fn count(&self) -> usize {
        self.fields.len()
    }

    pub(super) fn some<'de, V: Visitor<'de>>(self, visitor: V) -> NativeResult<V::Value> {
        let value = self
            .roots
            .get(self.cx.runtime().gc(), 0)
            .ok_or_else(|| unsupported("Option snapshot"))?;
        let field = &self.fields[0];
        self.cx.decode_scope(field, &value, |cx| {
            visitor.visit_some(Deserializer::new(cx, field, value))
        })
    }
}

impl<'de, 'scope, 'runtime> EnumAccess<'de> for Variant<'scope, 'runtime> {
    type Error = RuntimeError;
    type Variant = Self;

    fn variant_seed<S: DeserializeSeed<'de>>(self, seed: S) -> NativeResult<(S::Value, Self)> {
        let name = seed.deserialize(StringDeserializer::<RuntimeError>::new(self.name.clone()))?;
        Ok((name, self))
    }
}

impl<'de> VariantAccess<'de> for Variant<'_, '_> {
    type Error = RuntimeError;

    fn unit_variant(self) -> NativeResult<()> {
        if self.fields.is_empty() {
            Ok(())
        } else {
            Err(unsupported("nonunit variant payload"))
        }
    }

    fn newtype_variant_seed<S: DeserializeSeed<'de>>(self, seed: S) -> NativeResult<S::Value> {
        if self.fields.len() != 1 {
            return Err(unsupported("newtype variant arity"));
        }
        let value = self
            .roots
            .get(self.cx.runtime().gc(), 0)
            .ok_or_else(|| unsupported("variant snapshot"))?;
        decode(self.cx, &self.fields[0], &value, seed)
    }

    fn tuple_variant<V: Visitor<'de>>(self, len: usize, visitor: V) -> NativeResult<V::Value> {
        if self.fields.len() != len {
            return Err(unsupported("tuple variant arity"));
        }
        Sequence::visit(self.cx, self.roots, self.fields, visitor)
    }

    fn struct_variant<V: Visitor<'de>>(
        self,
        _: &'static [&'static str],
        _: V,
    ) -> NativeResult<V::Value> {
        Err(unsupported("record variant"))
    }
}
