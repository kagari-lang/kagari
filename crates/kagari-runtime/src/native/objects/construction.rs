use crate::{
    error::RuntimeError,
    gc::roots::RootedValue,
    native::{
        binding::NativeResult,
        conversion::IntoKagari,
        objects::{Object, ObjectType, fields::Field},
        typed::NativeContext,
    },
    value::Value,
};
use kagari_types::visibility::Visibility;
use std::marker::PhantomData;

/// Retains converted initializers until a complete object can be published.
/// Dropping a failed or unfinished builder releases all of its temporary roots.
pub struct ObjectBuilder {
    object_type: ObjectType,
    fields: Vec<Option<RootedValue>>,
}

impl ObjectType {
    pub fn builder(&self) -> NativeResult<ObjectBuilder> {
        if !self.0.public
            || self
                .0
                .layout
                .layout()
                .fields
                .iter()
                .any(|field| field.visibility != Visibility::Public)
        {
            return Err(RuntimeError::module_validation(
                "object constructor requires public fields",
            ));
        }
        let mut fields = Vec::new();
        fields
            .try_reserve_exact(self.0.layout.layout().fields.len())
            .map_err(|_| RuntimeError::resource_limit("object initializers"))?;
        fields.resize_with(self.0.layout.layout().fields.len(), || None);
        Ok(ObjectBuilder {
            object_type: self.clone(),
            fields,
        })
    }
}

impl ObjectBuilder {
    pub fn set<T: IntoKagari>(
        &mut self,
        cx: &mut NativeContext<'_>,
        field: &Field<T>,
        value: T,
    ) -> NativeResult<&mut Self> {
        self.object_type.validate(cx.runtime())?;
        let slot = field.check_owner(&self.object_type)?;
        if self.fields[slot].is_some() {
            return Err(RuntimeError::module_validation(
                "duplicate object initializer",
            ));
        }
        let root = cx.conversion.argument_scope(|cx| {
            let value = cx.encode_prepared(&field.record.argument, value)?;
            cx.runtime()
                .root_value(value)
                .ok_or_else(|| RuntimeError::module_validation("object initializer retention"))
        })?;
        self.fields[slot] = Some(root);
        Ok(self)
    }

    pub fn build(self, cx: &mut NativeContext<'_>) -> NativeResult<Object> {
        self.object_type.validate(cx.runtime())?;
        let mut values = Vec::new();
        values
            .try_reserve_exact(self.fields.len())
            .map_err(|_| RuntimeError::resource_limit("object fields"))?;
        for field in &self.fields {
            values.push(
                field
                    .as_ref()
                    .and_then(|root| root.value(cx.runtime().gc()))
                    .ok_or_else(|| {
                        RuntimeError::module_validation("missing or foreign object initializer")
                    })?,
            );
        }
        let layout = self.object_type.0.layout.clone();
        let id = cx.runtime().alloc_struct(layout.clone(), values)?;
        let root = cx
            .runtime()
            .root_value(Value::Struct(id))
            .ok_or_else(|| RuntimeError::module_validation("constructed object retention"))?;
        cx.runtime().gc_safepoint()?;
        Ok(Object {
            root,
            object_type: self.object_type,
            layout,
            schema: PhantomData,
        })
    }
}
