//! Declared private payload fields store graph edges instead of host root leases.
mod access;
mod application;
pub mod construction;

use crate::{
    error::RuntimeError,
    frame::types::arguments::TypeArgument,
    module::ExecutableModule,
    native::{
        binding::NativeResult,
        conversion::KagariType,
        payload::{NativeType, data::NativeData},
        storage::{NativePayload, NativeStorage},
        storage_type::StorageType,
        types::Type,
    },
    value::{
        carriers::{ScopedValue, StoredValue},
        native::data::run_edit,
        properties::ValueProperties,
    },
};
use kagari_contract::representation::semantic_representation;
use kagari_gc::{
    access::Scope,
    error::GcError,
    object::{Gc, GcObject, ObjectCharge},
    scan::{EdgeVisitor, ScanBudget, ScanKind, ScanStatus},
};
use std::{marker::PhantomData, sync::Arc, thread};

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
    owner: ExecutableModule,
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
/// use kagari_runtime::native::{payload::{Opaque, managed::Managed}, typed::NativeContext};
/// fn edit_fields(object: &Opaque<Managed<i32>>, cx: &NativeContext<'_>) {
///     object.edit(cx, |_| Ok(())).unwrap();
/// }
/// ```
/// Nor can a data borrow escape the restricted edit:
/// ```compile_fail
/// use kagari_runtime::native::{payload::{Opaque, managed::Managed}, typed::NativeContext};
/// fn escape<'a>(object: &Opaque<Managed<i32>>, cx: &'a mut NativeContext<'_>) -> &'a mut i32 {
///     object.edit_data(cx, |data| Ok(data)).unwrap()
/// }
/// ```
#[derive(Debug)]
pub struct Managed<T> {
    pub(crate) data: T,
    pub(crate) schema: Arc<AppliedSchema>,
    pub(crate) values: Vec<StoredValue>,
}

impl<T> Managed<T> {
    pub fn data(&self) -> &T {
        &self.data
    }
}

impl<T: NativeData> Managed<T> {
    pub(crate) fn alloc<'gc>(
        cx: &mut Scope<'gc>,
        data: T,
        schema: Arc<AppliedSchema>,
        values: &[ScopedValue<'gc>],
    ) -> Result<Gc<'gc, Self>, GcError> {
        if values.len() != schema.fields.len() {
            return Err(GcError::Representation);
        }
        for (value, field) in values.iter().zip(&schema.fields) {
            if !value.has_representation(semantic_representation(&field.contract.ty))
                || !ValueProperties::of(cx, *value)?.storable
            {
                return Err(GcError::Representation);
            }
        }
        // SAFETY: the checked builder retains every input, imports each cell into
        // the returned provider and never exposes partially initialized storage.
        unsafe {
            cx.alloc_with(|mut init| {
                let mut fields = Vec::new();
                fields.try_reserve_exact(values.len())?;
                for value in values {
                    fields.push(StoredValue::initialize(&mut init, *value)?);
                }
                Ok(Self {
                    data,
                    schema,
                    values: fields,
                })
            })
        }
    }
}

impl<T: NativeData> Managed<T> {
    pub(crate) fn get<'gc>(
        cx: &Scope<'gc>,
        owner: Gc<'gc, Self>,
        slot: usize,
    ) -> Result<ScopedValue<'gc>, GcError> {
        StoredValue::get_field(cx, owner, |owner| owner.values.get(slot))
    }

    pub(crate) fn set<'gc>(
        cx: &mut Scope<'gc>,
        owner: Gc<'gc, Self>,
        slot: usize,
        value: ScopedValue<'gc>,
    ) -> Result<(), GcError> {
        if !ValueProperties::of(cx, value)?.storable {
            return Err(GcError::Representation);
        }
        let valid = cx.read(owner, |owner| {
            owner.schema.fields.get(slot).is_some_and(|field| {
                value.has_representation(semantic_representation(&field.contract.ty))
            })
        });
        if !valid {
            return Err(GcError::Representation);
        }
        // SAFETY: checked fixed cell and protected input. Publish before the
        // coupled replacement; stable positions/capacity allow continuation.
        unsafe {
            cx.with_object_mut(owner, |owner, mutation| {
                let value = StoredValue::publish_field(mutation, value)?;
                owner.values[slot] = value;
                mutation.continue_scan();
                Ok(())
            })
        }
    }

    pub(crate) fn edit_data<'gc, R>(
        cx: &mut Scope<'gc>,
        owner: Gc<'gc, Self>,
        edit: impl FnOnce(&mut T) -> R,
    ) -> Result<thread::Result<R>, GcError> {
        // SAFETY: only recursive fixed data is lent. Fields, owned capacity and
        // scan positions cannot change; user panic resumes after heap loans end.
        unsafe {
            cx.with_object_mut(owner, |owner, mutation| {
                let result = run_edit(&mut owner.data, edit);
                mutation.continue_scan();
                Ok(result)
            })
        }
    }
}

impl<T: NativeData> NativePayload for Managed<T> {
    fn registration_storage() -> NativeStorage {
        NativeStorage::payload::<Self>()
    }
}

// SAFETY: private, fixed fields are initialized by the checked builder, scanned
// completely and only mutated through publication-aware operations. Fixed NativeData
// cannot hide edges or backing; immutable schema/type facts are not execution roots.
unsafe impl<T: NativeData> GcObject for Managed<T> {
    type ScanState = usize;
    const SCAN_KIND: ScanKind = ScanKind::Resumable;

    fn begin_scan(&self) -> usize {
        0
    }

    fn scan(
        &self,
        next: &mut usize,
        budget: &mut ScanBudget,
        visitor: &mut EdgeVisitor<'_>,
    ) -> Result<ScanStatus, GcError> {
        while let Some(value) = self.values.get(*next) {
            if !budget.take(1) {
                return Ok(ScanStatus::Pending);
            }
            // SAFETY: this admitted provider retains the initialized field.
            if unsafe { value.scan(budget, visitor)? } == ScanStatus::Pending {
                return Ok(ScanStatus::Pending);
            }
            *next += 1;
        }
        Ok(ScanStatus::Complete)
    }

    fn charge(&self) -> ObjectCharge {
        ObjectCharge {
            units: size_of::<T>()
                .div_ceil(size_of::<usize>())
                .checked_add(self.values.len())
                .expect("allocated fixed payload"),
            bytes: self.values.capacity() * size_of::<StoredValue>(),
        }
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
