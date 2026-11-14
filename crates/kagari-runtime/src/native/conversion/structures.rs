//! Owned DTO conversion uses checked layouts and one execution-local suffix.
use crate::{
    error::{RuntimeError, context::ErrorLocation},
    frame::types::arguments::TypeArgument,
    native::{
        binding::NativeResult,
        conversion::{
            FromKagari, IntoKagari,
            context::{ConversionContext, Frame, inputs::ValueInput, structures::StructSnapshot},
        },
        objects::ObjectType,
    },
    value::view::ValueView,
};

/// One unpublished DTO. Initializers remain in the conversion source; failure
/// retires that suffix, and finish promotes only the completed aggregate.
pub struct StructEncoder<'scope, 'runtime> {
    cx: Frame<'scope, 'runtime>,
    expected: TypeArgument,
    object_type: ObjectType,
    fields: Vec<Option<ValueView<'runtime>>>,
}

/// A source-owned snapshot of all fields before any child adapter can reenter.
/// Reading a field decodes independent Rust data, never a Rust reference to GC data.
pub struct StructDecoder<'scope, 'runtime> {
    cx: Frame<'scope, 'runtime>,
    object_type: ObjectType,
    values: StructSnapshot,
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
        let origin = self.owner_origin();
        let object_type = self.runtime_mut().retain_object_type(layout, origin)?;
        let count = object_type.constructor_len()?;
        let mut fields = Vec::new();
        fields
            .try_reserve_exact(count)
            .map_err(|_| RuntimeError::resource_limit("struct initializers"))?;
        fields.resize(count, None);
        Ok(StructEncoder {
            cx: self.scratch()?,
            expected: expected.clone(),
            object_type,
            fields,
        })
    }

    /// Snapshot a checked input's complete field pack before decoding children.
    /// Nominal scope, field visibility and every child mapping remain checked.
    pub fn read_struct<'scope>(
        &'scope mut self,
        expected: &TypeArgument,
        value: &ValueView<'_>,
    ) -> NativeResult<StructDecoder<'scope, 'runtime>> {
        self.poll()?;
        expected.validate(self.runtime())?;
        self.check_value(expected, value)?;
        let layout = self
            .runtime()
            .prepare_struct_layout(self.owner(), expected)?;
        self.check_elements(layout.layout().fields.len())?;
        let origin = self.owner_origin();
        let object_type = self
            .runtime_mut()
            .retain_object_type(layout.clone(), origin)?;
        let mut cx = self.scratch()?;
        let values = cx.snapshot_struct(value, &layout)?;
        Ok(StructDecoder {
            cx,
            object_type,
            values,
        })
    }
}

impl<'runtime> StructEncoder<'_, 'runtime> {
    #[cfg(feature = "serde")]
    pub(crate) fn field_with(
        &mut self,
        name: &str,
        encode: impl FnOnce(
            &mut ConversionContext<'runtime>,
            &TypeArgument,
        ) -> NativeResult<ValueView<'runtime>>,
    ) -> NativeResult<()> {
        let field = self.object_type.field(name)?;
        let argument = field.argument(self.cx.runtime())?;
        let slot = field.check_owner(&self.object_type)?;
        self.check_slot(slot)?;
        let value = encode(&mut self.cx, &argument)
            .map_err(|error| error.at(ErrorLocation::Field(name.to_owned())))?;
        self.cx.check_value(&argument, &value)?;
        self.fields[slot] = Some(value);
        Ok(())
    }

    /// Encode one named public field. Duplicate/missing fields reject; writing
    /// does not mutate an existing script object or the original Rust input.
    pub fn field<T: IntoKagari + 'static>(&mut self, name: &str, value: T) -> NativeResult<()> {
        let field = self
            .cx
            .runtime_mut()
            .bind_field::<T>(&self.object_type, name)?;
        let slot = field.check_owner(&self.object_type)?;
        self.check_slot(slot)?;
        let value = self
            .cx
            .encode_prepared(field.argument(), value)
            .map_err(|error| error.at(ErrorLocation::Field(name.to_owned())))?;
        self.fields[slot] = Some(value);
        Ok(())
    }

    fn check_slot(&self, slot: usize) -> NativeResult<()> {
        if self.fields[slot].is_some() {
            return Err(RuntimeError::module_validation(
                "duplicate object initializer",
            ));
        }
        Ok(())
    }

    /// Publish the complete object into the parent local suffix before retiring
    /// child temporaries. Owned DTO conversion creates no escaping Host lease.
    pub fn finish(mut self) -> NativeResult<ValueView<'runtime>> {
        if self.fields.iter().any(Option::is_none) {
            return Err(RuntimeError::module_validation(
                "missing object initializer",
            ));
        }
        let value = self.cx.allocate_struct(
            self.object_type.layout().clone(),
            self.fields
                .iter()
                .map(|value| ValueInput::View(value.expect("checked initializer"))),
        )?;
        self.cx.check_value(&self.expected, &value)?;
        self.cx.preserve(value)
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
        decode: impl FnOnce(
            &mut ConversionContext<'_>,
            &TypeArgument,
            &ValueView<'_>,
        ) -> NativeResult<R>,
    ) -> NativeResult<R> {
        let field = self.object_type.field(name)?;
        let slot = field.check_owner(&self.object_type)?;
        let argument = field.argument(self.cx.runtime())?;
        let value = self.values.element(&mut self.cx, slot)?;
        decode(&mut self.cx, &argument, &value)
            .map_err(|error| error.at(ErrorLocation::Field(name.to_owned())))
    }

    /// Decode a field from the original snapshot. A child converter may collect
    /// or edit the input's aliases without changing later snapshot fields.
    pub fn field<T: FromKagari + 'static>(&mut self, name: &str) -> NativeResult<T> {
        let field = self
            .cx
            .runtime_mut()
            .bind_field::<T>(&self.object_type, name)?;
        field
            .read_snapshot(&mut self.cx, &self.object_type, &self.values)
            .map_err(|error| error.at(ErrorLocation::Field(name.to_owned())))
    }
}
