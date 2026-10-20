//! DTO field conversion reuses checked object preparation and rooted construction.
use crate::{
    error::RuntimeError,
    frame::types::arguments::TypeArgument,
    gc::roots::RootSet,
    native::{
        binding::NativeResult,
        conversion::{FromKagari, IntoKagari, context::ConversionContext},
        objects::{ObjectType, construction::ObjectBuilder},
    },
    value::Value,
};

/// One unpublished DTO. All initializers use the same object builder and roots
/// as host-authored script objects; failure releases them without publication.
pub struct StructEncoder<'scope, 'runtime> {
    cx: &'scope mut ConversionContext<'runtime>,
    expected: TypeArgument,
    object_type: ObjectType,
    builder: ObjectBuilder,
}

/// A rooted snapshot of a struct's fields before any child adapter can reenter.
/// Reading a field decodes independent Rust data, never a Rust reference to GC data.
pub struct StructDecoder<'scope, 'runtime> {
    cx: &'scope mut ConversionContext<'runtime>,
    object_type: ObjectType,
    values: RootSet,
}

impl<'runtime> ConversionContext<'runtime> {
    /// Prepare an ordinary nominal struct for schema-directed owned conversion.
    pub fn write_struct<'scope>(
        &'scope mut self,
        expected: &TypeArgument,
    ) -> NativeResult<StructEncoder<'scope, 'runtime>> {
        self.poll()?;
        let layout = self
            .runtime()
            .prepare_struct_layout(self.owner(), expected)?;
        self.check_elements(layout.layout().fields.len())?;
        let object_type = self.runtime().retain_object_type(layout)?;
        let builder = object_type.builder()?;
        Ok(StructEncoder {
            cx: self,
            expected: expected.clone(),
            object_type,
            builder,
        })
    }

    /// Snapshot a checked input's complete field pack before decoding children.
    /// Nominal scope, field visibility and every child mapping remain checked.
    pub fn read_struct<'scope>(
        &'scope mut self,
        expected: &TypeArgument,
        value: &Value,
    ) -> NativeResult<StructDecoder<'scope, 'runtime>> {
        self.poll()?;
        expected.validate(self.runtime())?;
        self.check_value(expected, value)?;
        let Value::Struct(id) = value else {
            return Err(RuntimeError::module_validation(
                "struct conversion requires a struct value",
            ));
        };
        let layout = self
            .runtime()
            .gc()
            .struct_layout(*id)
            .ok_or_else(|| RuntimeError::module_validation("struct conversion input"))?;
        self.check_elements(layout.layout().fields.len())?;
        let object_type = self.runtime().retain_object_type(layout.clone())?;
        let mut fields = Vec::new();
        fields
            .try_reserve_exact(layout.layout().fields.len())
            .map_err(|_| {
                self.runtime()
                    .resources()
                    .limit("struct conversion snapshot")
            })?;
        for slot in 0..layout.layout().fields.len() {
            self.poll()?;
            fields.push(
                self.runtime()
                    .gc()
                    .struct_get_slot(*id, &layout, slot)
                    .ok_or_else(|| {
                        RuntimeError::module_validation("struct conversion snapshot field")
                    })?,
            );
        }
        let values = self
            .runtime()
            .gc()
            .root_execution_values(fields)
            .ok_or_else(|| RuntimeError::module_validation("struct conversion snapshot roots"))?;
        Ok(StructDecoder {
            cx: self,
            object_type,
            values,
        })
    }
}

impl StructEncoder<'_, '_> {
    #[cfg(feature = "serde")]
    pub(crate) fn field_with(
        &mut self,
        name: &str,
        encode: impl FnOnce(&mut ConversionContext<'_>, &TypeArgument) -> NativeResult<Value>,
    ) -> NativeResult<()> {
        let field = self.object_type.field(name)?;
        let argument = field.argument(self.cx.runtime())?;
        self.builder.check_member(&field)?;
        let value = encode(self.cx, &argument)
            .map_err(|error| error.at_conversion(format_args!("field {name}")))?;
        self.builder.set_value_in(self.cx, &field, value)
    }

    /// Encode one named public field. Duplicate/missing fields reject; writing
    /// does not mutate an existing script object or the original Rust input.
    pub fn field<T: IntoKagari + 'static>(&mut self, name: &str, value: T) -> NativeResult<()> {
        let field = self.cx.runtime().bind_field::<T>(&self.object_type, name)?;
        self.builder
            .set_in(self.cx, &field, value)
            .map(|_| ())
            .map_err(|error| error.at_conversion(format_args!("field {name}")))
    }

    /// Publish only after every initializer succeeds. The raw result must stay
    /// in the enclosing conversion scope until its normal result root is retained.
    pub fn finish(self) -> NativeResult<Value> {
        let object = self.builder.build_in(self.cx)?;
        object.into_kagari(self.cx, &self.expected)
    }
}

impl StructDecoder<'_, '_> {
    #[cfg(feature = "serde")]
    pub(crate) fn names(&self) -> Vec<String> {
        self.object_type.field_names()
    }

    #[cfg(feature = "serde")]
    pub(crate) fn field_with<R>(
        &mut self,
        name: &str,
        decode: impl FnOnce(&mut ConversionContext<'_>, &TypeArgument, &Value) -> NativeResult<R>,
    ) -> NativeResult<R> {
        let field = self.object_type.field(name)?;
        let slot = field.check_owner(&self.object_type)?;
        let argument = field.argument(self.cx.runtime())?;
        let value = self
            .values
            .get(self.cx.runtime().gc(), slot)
            .ok_or_else(|| RuntimeError::module_validation("struct conversion snapshot field"))?;
        decode(self.cx, &argument, &value)
            .map_err(|error| error.at_conversion(format_args!("field {name}")))
    }

    /// Decode a field from the original snapshot. A child converter may collect
    /// or edit the input's aliases without changing later snapshot fields.
    pub fn field<T: FromKagari + 'static>(&mut self, name: &str) -> NativeResult<T> {
        let field = self.cx.runtime().bind_field::<T>(&self.object_type, name)?;
        field
            .read_snapshot(self.cx, &self.object_type, &self.values)
            .map_err(|error| error.at_conversion(format_args!("field {name}")))
    }
}
