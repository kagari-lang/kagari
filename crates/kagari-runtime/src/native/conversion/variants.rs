//! Ordinary enum payloads use the same complete-pack conversion as typed calls.
use crate::{
    error::RuntimeError,
    frame::types::arguments::TypeArgument,
    module::EnumVariantRef,
    native::{
        binding::NativeResult,
        context::arguments::ArgumentView,
        conversion::{
            arguments::{FromKagariArguments, IntoKagariArguments, encode_arguments},
            context::ConversionContext,
        },
        value_handle::ScriptValue,
    },
    value::{EnumTag, Value},
};
use std::slice;

impl ConversionContext<'_> {
    /// Resolve an exact declared enum variant before converting its payload fields.
    pub fn declared_variant(
        &self,
        expected: &TypeArgument,
        member: &str,
    ) -> NativeResult<EnumVariantRef> {
        self.poll()?;
        expected.validate(self.runtime())?;
        self.runtime()
            .declared_enum_variant(self.owner(), expected, member)
    }

    /// Encode a declared unit/tuple variant without serializing bytes. Payload
    /// tuple elements are fields; a tuple-valued field uses an additional tuple.
    pub fn encode_variant<A: IntoKagariArguments>(
        &mut self,
        expected: &TypeArgument,
        member: &str,
        payload: A,
    ) -> NativeResult<ScriptValue> {
        self.poll()?;
        let layout = self
            .runtime()
            .declared_enum_variant(self.owner(), expected, member)?;
        let fields = self.variant_fields(&layout)?;
        let (_roots, values) = encode_arguments(self, &fields, payload)
            .map_err(|error| error.at_conversion(format_args!("variant {member}")))?;
        let value = self
            .runtime()
            .alloc_enum(EnumTag::Declared(layout), values)?;
        self.retain_converted(expected, Value::Enum(value))
    }

    /// Inspect only the checked nominal variant. No payload is decoded or exposed
    /// as a Rust borrow; generated converters select their matching tuple shape.
    pub fn variant_name(
        &self,
        expected: &TypeArgument,
        value: &ScriptValue,
    ) -> NativeResult<String> {
        let layout = self.variant_layout(expected, value)?;
        layout
            .module()
            .definition_name(layout.variant().declaration)
            .map(str::to_owned)
            .ok_or_else(|| RuntimeError::module_validation("enum conversion variant identity"))
    }

    /// Decode a selected variant's complete, rooted payload pack. Type checks
    /// precede child converters; failed conversion does not publish partial data.
    pub fn decode_variant<A: FromKagariArguments>(
        &mut self,
        expected: &TypeArgument,
        value: &ScriptValue,
        member: &str,
    ) -> NativeResult<A> {
        let raw = value.call_argument(self.runtime(), expected, self.owner())?;
        self.poll()?;
        let actual = self.variant_layout(expected, value)?;
        let required = self
            .runtime()
            .declared_enum_variant(self.owner(), expected, member)?;
        if !actual.matches_layout(&required) {
            return Err(RuntimeError::module_validation(
                "enum conversion variant mismatch",
            ));
        }
        let Value::Enum(id) = raw else {
            unreachable!("checked enum value")
        };
        self.check_elements(actual.variant().payload.len())?;
        let snapshot = self
            .runtime()
            .gc()
            .enum_snapshot(id)
            .ok_or_else(|| RuntimeError::module_validation("enum conversion snapshot"))?;
        let fields = self.variant_fields(&actual)?;
        let snapshot = ArgumentView::from_values(self.runtime().gc(), snapshot.fields)?;
        A::from_arguments(self, &fields, &snapshot)
            .map_err(|error| error.at_conversion(format_args!("variant {member}")))
    }

    /// Check an enum value against its declared type and return its exact member layout.
    pub fn variant_layout(
        &self,
        expected: &TypeArgument,
        value: &ScriptValue,
    ) -> NativeResult<EnumVariantRef> {
        let raw = value.call_argument(self.runtime(), expected, self.owner())?;
        let value = &raw;
        self.poll()?;
        expected.validate(self.runtime())?;
        self.check_value(expected, value)?;
        let Value::Enum(id) = value else {
            return Err(RuntimeError::module_validation(
                "enum conversion requires an enum value",
            ));
        };
        self.runtime()
            .gc()
            .enum_view(*id)
            .map(|view| {
                let EnumTag::Declared(layout) = &view.tag;
                layout.clone()
            })
            .ok_or_else(|| RuntimeError::module_validation("enum conversion identity"))
    }

    /// Resolve member payload types in the layout's original lexical scope.
    pub fn variant_fields(&self, layout: &EnumVariantRef) -> NativeResult<Vec<TypeArgument>> {
        self.check_elements(layout.variant().payload.len())?;
        let mut fields = Vec::new();
        fields
            .try_reserve_exact(layout.variant().payload.len())
            .map_err(|_| {
                self.runtime()
                    .resources()
                    .limit("enum conversion payload types")
            })?;
        for index in 0..layout.variant().payload.len() {
            let (ty, _) = layout
                .payload_type(index)
                .ok_or_else(|| RuntimeError::module_validation("enum conversion payload type"))?;
            fields.push(
                self.runtime()
                    .type_arguments(
                        layout.module(),
                        layout.type_bindings().cloned(),
                        slice::from_ref(ty),
                    )?
                    .pop()
                    .ok_or_else(|| {
                        RuntimeError::module_validation("enum conversion payload scope")
                    })?,
            );
        }
        Ok(fields)
    }
}
