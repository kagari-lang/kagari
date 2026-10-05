use crate::{
    Runtime,
    error::RuntimeError,
    frame::types::arguments::TypeArgument,
    native::{
        binding::NativeResult,
        conversion::{FromKagari, IntoKagari, KagariType, context::ConversionContext},
        objects::{Object, ObjectType},
        typed::NativeContext,
    },
};
use kagari_types::visibility::Visibility;
use std::{
    any::TypeId,
    marker::PhantomData,
    slice,
    sync::{Arc, Weak},
};

/// A resolved declaration member in one applied object type, before choosing
/// its Rust conversion. It cannot be forged from an unchecked integer offset.
#[derive(Debug, Clone)]
pub struct ObjectField {
    object_type: ObjectType,
    slot: usize,
}

#[derive(Debug)]
pub struct Field<T> {
    pub(super) record: Arc<FieldRecord>,
    mapping: PhantomData<fn() -> T>,
}

impl<T> Clone for Field<T> {
    fn clone(&self) -> Self {
        Self {
            record: self.record.clone(),
            mapping: PhantomData,
        }
    }
}

#[derive(Debug)]
pub(crate) struct FieldRecord {
    pub(super) member: ObjectField,
    pub(super) argument: TypeArgument,
    rust_type: TypeId,
}

impl ObjectType {
    /// Resolve a public member once. Private and parent-module access never
    /// become public merely because the embedding host knows the field's name.
    pub fn field(&self, name: &str) -> NativeResult<ObjectField> {
        let slot = self
            .0
            .layout
            .layout()
            .fields
            .iter()
            .position(|field| field.name == name)
            .ok_or_else(|| RuntimeError::module_validation("unknown object field"))?;
        if !self.0.public || self.0.layout.layout().fields[slot].visibility != Visibility::Public {
            return Err(RuntimeError::module_validation(
                "object field is not public",
            ));
        }
        Ok(ObjectField {
            object_type: self.clone(),
            slot,
        })
    }
}

impl Runtime {
    pub fn bind_field<T: KagariType + 'static>(
        &self,
        object_type: &ObjectType,
        name: &str,
    ) -> NativeResult<Field<T>> {
        self.bind_field_declaration(&object_type.field(name)?)
    }

    pub fn bind_field_declaration<T: KagariType + 'static>(
        &self,
        member: &ObjectField,
    ) -> NativeResult<Field<T>> {
        member.object_type.validate(self)?;
        let layout = &member.object_type.0.layout;
        let (ty, _) = layout
            .field_type(member.slot)
            .ok_or_else(|| RuntimeError::module_validation("object field slot"))?;
        let argument = self
            .type_arguments(
                layout.module(),
                layout.environment.clone(),
                slice::from_ref(ty),
            )?
            .pop()
            .ok_or_else(|| RuntimeError::module_validation("object field scope"))?;
        let cx = ConversionContext::new(self, layout.module())?;
        cx.check_type::<T>(&argument)?;
        let mut cache = self
            .object_bindings
            .try_borrow_mut()
            .map_err(|_| RuntimeError::module_validation("field binding cache is borrowed"))?;
        cache.fields.retain(|record| record.strong_count() != 0);
        if let Some(record) = cache
            .fields
            .iter()
            .filter_map(Weak::upgrade)
            .find(|record| {
                Arc::ptr_eq(&record.member.object_type.0, &member.object_type.0)
                    && record.member.slot == member.slot
                    && record.rust_type == TypeId::of::<T>()
            })
        {
            return Ok(Field {
                record,
                mapping: PhantomData,
            });
        }
        let record = Arc::new(FieldRecord {
            member: member.clone(),
            argument,
            rust_type: TypeId::of::<T>(),
        });
        cache
            .fields
            .try_reserve(1)
            .map_err(|_| RuntimeError::resource_limit("field binding cache"))?;
        cache.fields.push(Arc::downgrade(&record));
        Ok(Field {
            record,
            mapping: PhantomData,
        })
    }
}

impl<T> Field<T> {
    pub fn is_mutable(&self) -> bool {
        self.record.member.object_type.0.layout.layout().fields[self.record.member.slot].mutable
    }

    pub(super) fn check_owner(&self, object_type: &ObjectType) -> NativeResult<usize> {
        if !Arc::ptr_eq(&self.record.member.object_type.0, &object_type.0) {
            return Err(RuntimeError::module_validation(
                "field binding belongs to another applied type or generation",
            ));
        }
        Ok(self.record.member.slot)
    }
}

impl<S> Object<S> {
    pub fn get<T: FromKagari>(
        &self,
        cx: &mut NativeContext<'_>,
        field: &Field<T>,
    ) -> NativeResult<T> {
        let slot = field.check_owner(&self.object_type)?;
        let id = self.id(cx)?;
        let value = cx
            .runtime()
            .gc()
            .struct_get_slot(id, &self.layout, slot)
            .ok_or_else(|| RuntimeError::module_validation("object field read"))?;
        cx.conversion
            .decode_prepared(&field.record.argument, &value)
    }

    pub fn set<T: IntoKagari>(
        &self,
        cx: &mut NativeContext<'_>,
        field: &Field<T>,
        value: T,
    ) -> NativeResult<()> {
        let slot = field.check_owner(&self.object_type)?;
        let id = self.id(cx)?;
        if !field.is_mutable() {
            return Err(RuntimeError::module_validation("object field is read-only"));
        }
        cx.conversion.argument_scope(|cx| {
            let value = cx.encode_prepared(&field.record.argument, value)?;
            cx.runtime()
                .gc()
                .struct_set_slot(id, &self.layout, slot, value)
        })
    }
}
