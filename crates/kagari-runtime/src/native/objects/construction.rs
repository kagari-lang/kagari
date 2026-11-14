use crate::{
    error::RuntimeError,
    native::{
        binding::NativeResult,
        conversion::{
            IntoKagari,
            context::{ConversionContext, inputs::ValueInput},
        },
        objects::{Object, ObjectType, fields::Field},
        typed::NativeContext,
    },
    value::carriers::RootedValue,
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
    pub(crate) fn constructor_len(&self) -> NativeResult<usize> {
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
        Ok(self.0.layout.layout().fields.len())
    }

    pub fn builder(&self) -> NativeResult<ObjectBuilder> {
        let count = self.constructor_len()?;
        let mut fields = Vec::new();
        fields
            .try_reserve_exact(count)
            .map_err(|_| RuntimeError::resource_limit("object initializers"))?;
        fields.resize_with(count, || None);
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
        self.set_in(&mut cx.conversion, field, value)
    }

    pub(crate) fn set_in<T: IntoKagari>(
        &mut self,
        cx: &mut ConversionContext<'_>,
        field: &Field<T>,
        value: T,
    ) -> NativeResult<&mut Self> {
        self.object_type.validate(cx.runtime())?;
        let slot = field.check_owner(&self.object_type)?;
        self.check_slot(slot)?;
        let root = cx.argument_scope(|cx| {
            let value = cx.encode_prepared(&field.record.argument, value)?;
            cx.retain(&value)
        })?;
        self.fields[slot] = Some(root);
        Ok(self)
    }

    fn check_slot(&self, slot: usize) -> NativeResult<()> {
        if self.fields[slot].is_some() {
            return Err(RuntimeError::module_validation(
                "duplicate object initializer",
            ));
        }
        Ok(())
    }

    pub fn build(self, cx: &mut NativeContext<'_>) -> NativeResult<Object> {
        self.build_in(&mut cx.conversion)
    }

    pub(crate) fn build_in(self, cx: &mut ConversionContext<'_>) -> NativeResult<Object> {
        self.object_type.validate(cx.runtime())?;
        if self.fields.iter().any(Option::is_none) {
            return Err(RuntimeError::module_validation(
                "missing object initializer",
            ));
        }
        let layout = self.object_type.0.layout.clone();
        let root = cx.argument_scope(|cx| {
            let value = cx.allocate_struct(
                layout.clone(),
                self.fields
                    .iter()
                    .map(|field| ValueInput::Root(field.as_ref().expect("checked initializer"))),
            )?;
            cx.retain(&value)
        })?;
        cx.runtime_mut().gc_safepoint()?;
        Ok(Object {
            root,
            object_type: self.object_type,
            layout,
            schema: PhantomData,
        })
    }
}
