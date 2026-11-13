use crate::{
    error::RuntimeError,
    native::{
        binding::NativeResult,
        conversion::{IntoKagari, context::ConversionContext},
        payload::{
            NativeType, Opaque,
            data::NativeData,
            managed::{BoundField, Managed, invalid},
        },
        typed::NativeContext,
    },
    value::carriers::RootedValue,
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
        let mut conversion = ConversionContext::new(cx.runtime_mut(), self.native_type.owner())?;
        let value = conversion.encode_prepared(field.type_argument(), value)?;
        let root = conversion.retain(&value)?;
        self.fields[field.slot] = Some(root);
        Ok(())
    }

    pub fn finish(self, cx: &mut NativeContext<'_>) -> NativeResult<Opaque<Managed<T>>> {
        let schema = self.native_type.schema()?.clone();
        if self.fields.iter().any(Option::is_none) {
            return Err(invalid());
        }
        let fields = self.fields;
        let data = self.data;
        // set() checks each field mapping and complete encoded value before
        // retaining it. Those immutable type/storability facts cannot be revoked
        // by mutating an alias; the owning roots protect all initializer targets.
        self.native_type.create_with(cx, move |init| {
            let mut values = Vec::new();
            values.try_reserve_exact(fields.len())?;
            for field in &fields {
                values.push(
                    field
                        .as_ref()
                        .expect("complete managed initializer")
                        .get(init.scope())?,
                );
            }
            Managed::alloc(init.scope(), data, schema, &values)
        })
    }

    /// Replace data and all fields through the same checked commit boundary.
    /// Conversion failures leave the destination unchanged.
    pub fn replace(
        self,
        cx: &mut NativeContext<'_>,
        target: &Opaque<Managed<T>>,
    ) -> NativeResult<()> {
        if !self
            .native_type
            .schema()?
            .matches(target.native_type.schema()?)
        {
            return Err(invalid());
        }
        let schema = self.native_type.schema()?.clone();
        if self.fields.iter().any(Option::is_none) {
            return Err(invalid());
        }
        let fields = self.fields;
        let data = self.data;
        self.native_type.replace_with(cx, target, move |init| {
            let mut values = Vec::new();
            values.try_reserve_exact(fields.len())?;
            for field in &fields {
                values.push(
                    field
                        .as_ref()
                        .expect("complete managed initializer")
                        .get(init.scope())?,
                );
            }
            Managed::alloc(init.scope(), data, schema, &values)
        })
    }
}
