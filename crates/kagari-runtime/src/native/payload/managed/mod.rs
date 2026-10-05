//! Declared private payload fields store graph edges instead of host root leases.
mod access;
mod application;
pub mod construction;

use crate::{
    error::RuntimeError,
    frame::types::arguments::TypeArgument,
    module::LoadedModule,
    native::{
        binding::NativeResult,
        conversion::KagariType,
        payload::{NativeType, data::NativeData},
        storage::{NativePayload, NativeStorage},
        storage_type::StorageType,
        types::Type,
    },
    value::Value,
};
use std::{marker::PhantomData, sync::Arc};

#[derive(Debug)]
pub(crate) struct ManagedSchema {
    identity: Arc<()>,
    fields: Vec<FieldDeclaration>,
}

#[derive(Debug)]
struct FieldDeclaration {
    name: String,
    ty: Type,
}

#[derive(Debug)]
pub(crate) struct FieldType {
    pub(crate) argument: TypeArgument,
    pub(crate) contract: StorageType,
}

#[derive(Debug)]
pub(crate) struct AppliedSchema {
    declaration: Arc<ManagedSchema>,
    argument: TypeArgument,
    owner: LoadedModule,
    pub(crate) fields: Vec<FieldType>,
}

/// One registration-time field identity. The Rust mapping is checked when bound
/// against an installed application; a token grants no unchecked slot access.
#[derive(Debug)]
pub struct Field<V> {
    identity: Arc<()>,
    slot: usize,
    mapping: PhantomData<fn() -> V>,
}

impl<V> Clone for Field<V> {
    fn clone(&self) -> Self {
        Self {
            identity: self.identity.clone(),
            slot: self.slot,
            mapping: PhantomData,
        }
    }
}

/// Author private native fields before installing the containing type. The schema
/// is immutable once finished; this does not add runtime type mutation to Kagari.
pub struct ManagedStorage<T> {
    schema: ManagedSchema,
    mapping: PhantomData<fn() -> T>,
}

impl<T: NativeData> Default for ManagedStorage<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: NativeData> ManagedStorage<T> {
    pub fn new() -> Self {
        Self {
            schema: ManagedSchema {
                identity: Arc::new(()),
                fields: Vec::new(),
            },
            mapping: PhantomData,
        }
    }

    pub fn field<V: KagariType>(
        &mut self,
        name: impl Into<String>,
        ty: Type,
    ) -> NativeResult<Field<V>> {
        let name = name.into();
        if name.is_empty() || self.schema.fields.iter().any(|field| field.name == name) {
            return Err(RuntimeError::metadata_conflict(
                "duplicate or empty managed field name",
            ));
        }
        let slot = self.schema.fields.len();
        self.schema
            .fields
            .try_reserve(1)
            .map_err(|_| RuntimeError::resource_limit("managed fields"))?;
        self.schema.fields.push(FieldDeclaration { name, ty });
        Ok(Field {
            identity: self.schema.identity.clone(),
            slot,
            mapping: PhantomData,
        })
    }

    pub fn finish(self) -> NativeStorage {
        NativeStorage::payload::<Managed<T>>().with_managed_schema(self.schema)
    }
}

/// Runtime-owned fixed data and private checked script fields. There is no public
/// constructor, Clone or mutable view of the stored fields. Build through NativeType.
///
/// The complete payload cannot be edited as ordinary data:
/// ```compile_fail
/// use kagari_runtime::native::{payload::{NativeObject, managed::Managed}, typed::NativeContext};
/// fn edit_fields(object: &NativeObject<Managed<i32>>, cx: &NativeContext<'_>) {
///     object.edit(cx, |_| Ok(())).unwrap();
/// }
/// ```
/// Nor can a data borrow escape the restricted edit:
/// ```compile_fail
/// use kagari_runtime::native::{payload::{NativeObject, managed::Managed}, typed::NativeContext};
/// fn escape<'a>(object: &NativeObject<Managed<i32>>, cx: &'a NativeContext<'_>) -> &'a mut i32 {
///     object.edit_data(cx, |data| Ok(data)).unwrap()
/// }
/// ```
#[derive(Debug)]
pub struct Managed<T> {
    pub(crate) data: T,
    pub(crate) schema: Arc<AppliedSchema>,
    pub(crate) values: Vec<Value>,
}

impl<T> Managed<T> {
    pub fn data(&self) -> &T {
        &self.data
    }
}

impl<T: NativeData> NativePayload for Managed<T> {
    fn trace<'payload>(&'payload self, visit: &mut dyn FnMut(&'payload Value)) {
        for value in &self.values {
            visit(value);
        }
    }

    fn units(&self) -> usize {
        self.data.units().saturating_add(self.values.len())
    }
}

/// A reusable, version-pinned mapping for one registered field.
#[derive(Debug)]
pub struct BoundField<T, V> {
    native_type: NativeType<Managed<T>>,
    slot: usize,
    mapping: PhantomData<fn() -> V>,
}

impl<T, V> Clone for BoundField<T, V> {
    fn clone(&self) -> Self {
        Self {
            native_type: self.native_type.clone(),
            slot: self.slot,
            mapping: PhantomData,
        }
    }
}

pub(crate) fn invalid() -> RuntimeError {
    RuntimeError::module_validation("managed payload field differs from its registered application")
}
