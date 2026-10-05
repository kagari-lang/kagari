use crate::{
    error::RuntimeError,
    gc::roots::RootedValue,
    native::{
        binding::NativeResult,
        conversion::{IntoKagari, context::ConversionContext},
        payload::{
            NativeObject, NativeType,
            data::NativeData,
            managed::{BoundField, Managed, invalid},
        },
        typed::NativeContext,
    },
    value::Value,
};

/// Unpublished field values own temporary roots until construction or replacement
/// commits. Dropping a builder releases them without publishing a partial object.
pub struct ManagedBuilder<T> {
    native_type: NativeType<Managed<T>>,
    data: T,
    fields: Vec<Option<RootedValue>>,
}

impl<T: NativeData> NativeType<Managed<T>> {
    pub fn build(&self, data: T) -> NativeResult<ManagedBuilder<T>> {
        let count = self.schema()?.fields.len();
        let mut fields = Vec::new();
        fields
            .try_reserve_exact(count)
            .map_err(|_| RuntimeError::resource_limit("managed initializers"))?;
        fields.resize(count, None);
        Ok(ManagedBuilder {
            native_type: self.clone(),
            data,
            fields,
        })
    }
}

impl<T: NativeData> ManagedBuilder<T> {
    pub fn set<V: IntoKagari>(
        &mut self,
        cx: &mut NativeContext<'_>,
        field: &BoundField<T, V>,
        value: V,
    ) -> NativeResult<()> {
        field.check(&self.native_type)?;
        let mut conversion = ConversionContext::new(cx.runtime(), self.native_type.owner())?;
        let value = conversion.encode_prepared(field.type_argument(), value)?;
        let root = cx.runtime().root_value(value).ok_or_else(invalid)?;
        self.fields[field.slot] = Some(root);
        Ok(())
    }

    fn prepare(&self, cx: &NativeContext<'_>) -> NativeResult<Managed<T>> {
        cx.runtime().gc().ensure_no_native_borrow()?;
        cx.runtime().resources().ensure_execution_allowed()?;
        cx.runtime()
            .validate_loaded_module(self.native_type.owner())?;
        cx.poll()?;
        let mut values = Vec::new();
        values
            .try_reserve_exact(self.fields.len())
            .map_err(|_| RuntimeError::resource_limit("managed initializers"))?;
        for field in &self.fields {
            values.push(
                field
                    .as_ref()
                    .and_then(|root| root.value(cx.runtime().gc()))
                    .ok_or_else(invalid)?,
            );
        }
        let schema = self.native_type.schema()?.clone();
        for (field, value) in schema.fields.iter().zip(&values) {
            if !value.is_default_heap_payload()
                || !field.contract.accepts_value(cx.runtime().gc(), value)
            {
                return Err(invalid());
            }
        }
        Ok(Managed {
            data: self.data,
            schema,
            values,
        })
    }

    pub fn finish(self, cx: &mut NativeContext<'_>) -> NativeResult<NativeObject<Managed<T>>> {
        let payload = self.prepare(cx)?;
        // self retains every initializer until the newly allocated object is rooted.
        self.native_type.create(cx, payload)
    }

    /// Replace data and all fields through the same checked commit boundary.
    /// Conversion failures leave the destination unchanged.
    pub fn replace(
        self,
        cx: &mut NativeContext<'_>,
        target: &NativeObject<Managed<T>>,
    ) -> NativeResult<()> {
        if !self
            .native_type
            .schema()?
            .matches(target.native_type.schema()?)
        {
            return Err(invalid());
        }
        let payload = self.prepare(cx)?;
        let Value::GcHandle(id) = target.value(cx.runtime().gc())? else {
            return Err(invalid());
        };
        cx.runtime().gc().replace_managed(id, payload)?;
        cx.runtime().gc_safepoint()
    }
}
