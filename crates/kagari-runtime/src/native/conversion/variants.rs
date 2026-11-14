//! Ordinary enum payloads use the same complete-pack conversion as typed calls.
use crate::{
    error::{RuntimeError, context::ErrorLocation},
    frame::types::arguments::TypeArgument,
    module::EnumVariantRef,
    native::{
        binding::NativeResult,
        conversion::{
            arguments::{FromKagariArguments, IntoKagariArguments, PreparedArguments},
            context::ConversionContext,
        },
    },
    value::view::ValueView,
};

use std::slice;

impl<'call> ConversionContext<'call> {
    /// Encode a declared unit/tuple variant without serializing bytes. Payload
    /// tuple elements are fields; a tuple-valued field uses an additional tuple.
    pub fn encode_variant<A: IntoKagariArguments>(
        &mut self,
        expected: &TypeArgument,
        member: &str,
        payload: A,
    ) -> NativeResult<ValueView<'call>> {
        self.poll()?;
        let layout = self
            .runtime()
            .declared_enum_variant(self.owner(), expected, member)?;
        let fields = self.variant_fields(&layout)?;
        let mut frame = self.scratch()?;
        let mapping = PreparedArguments::<A>::new(&frame, &fields)?;
        let values = mapping
            .encode(&mut frame, payload)
            .map_err(|error| error.at(ErrorLocation::Variant(member.to_owned())))?;
        let value = frame.allocate_enum_pack(layout, values)?;
        frame.preserve(value)
    }

    /// Inspect only the checked nominal variant. No payload is decoded or exposed
    /// as a Rust borrow; generated converters select their matching tuple shape.
    pub fn variant_name(
        &mut self,
        expected: &TypeArgument,
        value: &ValueView<'_>,
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
        value: &ValueView<'_>,
        member: &str,
    ) -> NativeResult<A> {
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
        self.check_elements(actual.variant().payload.len())?;
        let fields = self.variant_fields(&actual)?;
        let mut frame = self.scratch()?;
        let values = frame.enum_arguments(value)?;
        PreparedArguments::<A>::new(&frame, &fields)?
            .decode(&mut frame, values)
            .map_err(|error| error.at(ErrorLocation::Variant(member.to_owned())))
    }

    pub(crate) fn variant_layout(
        &mut self,
        expected: &TypeArgument,
        value: &ValueView<'_>,
    ) -> NativeResult<EnumVariantRef> {
        self.poll()?;
        self.check_value(expected, value)?;
        self.enum_shape(value).map(|(layout, _)| layout)
    }

    pub(crate) fn variant_fields(
        &self,
        layout: &EnumVariantRef,
    ) -> NativeResult<Vec<TypeArgument>> {
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
