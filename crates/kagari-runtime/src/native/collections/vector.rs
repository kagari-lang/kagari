mod edits;
use crate::{
    error::RuntimeError,
    frame::types::arguments::TypeArgument,
    gc::{HeapObjectId, roots::RootedValue},
    native::{
        binding::NativeResult,
        catalog::DeclarationCatalog,
        conversion::{FromKagari, IntoKagari, KagariType, context::ConversionContext},
        storage_type::StorageType,
        typed::NativeContext,
        types::Type,
    },
    value::Value,
};
use kagari_types::{collection::CollectionAccess, ty::Ty};
use std::{marker::PhantomData, sync::Arc};

/// A retained view with the exact element scope and outer access capability.
#[derive(Debug)]
pub struct ScriptVec<T> {
    root: RootedValue,
    element: TypeArgument,
    access: CollectionAccess,
    mapping: PhantomData<fn() -> T>,
}

impl<T> Clone for ScriptVec<T> {
    fn clone(&self) -> Self {
        Self {
            root: self.root.clone(),
            element: self.element.clone(),
            access: self.access,
            mapping: PhantomData,
        }
    }
}

impl<T: KagariType> KagariType for ScriptVec<T> {
    fn kagari_type(catalog: &DeclarationCatalog) -> NativeResult<Type> {
        Vec::<T>::kagari_type(catalog)
    }

    fn check_type(cx: &ConversionContext<'_>, expected: &TypeArgument) -> NativeResult<()> {
        if !matches!(expected.ty(), Ty::Array(_, _)) {
            return Err(RuntimeError::module_validation(
                "ScriptVec requires an array type",
            ));
        }
        cx.check_type::<T>(&cx.parameter(expected, 0)?)
    }
}

impl<T: KagariType> FromKagari for ScriptVec<T> {
    const PRESERVES_IDENTITY: bool = true;

    fn from_kagari(
        cx: &mut ConversionContext<'_>,
        expected: &TypeArgument,
        value: &Value,
    ) -> NativeResult<Self> {
        cx.check_type::<Self>(expected)?;
        cx.check_value(expected, value)?;
        let (Ty::Array(_, access), Value::Array(_)) = (expected.ty(), value) else {
            return Err(RuntimeError::module_validation("ScriptVec conversion"));
        };
        let element = cx.parameter(expected, 0)?;
        let root = cx
            .runtime()
            .root_value(value.clone())
            .ok_or_else(|| RuntimeError::module_validation("array handle retention"))?;
        Ok(Self {
            root,
            element,
            access: *access,
            mapping: PhantomData,
        })
    }
}

impl<T: KagariType> IntoKagari for ScriptVec<T> {
    fn into_kagari(
        self,
        cx: &mut ConversionContext<'_>,
        expected: &TypeArgument,
    ) -> NativeResult<Value> {
        cx.check_type::<Self>(expected)?;
        let Ty::Array(_, access) = expected.ty() else {
            unreachable!("checked array type")
        };
        if *access == CollectionAccess::Mutable && self.access != CollectionAccess::Mutable {
            return Err(RuntimeError::module_validation(
                "read-only collection cannot become mutable",
            ));
        }
        let value = self
            .root
            .value(cx.runtime().gc())
            .ok_or_else(|| RuntimeError::module_validation("foreign or expired collection"))?;
        cx.check_value(expected, &value)?;
        Ok(value)
    }
}

impl NativeContext<'_> {
    pub fn create_vec<T: IntoKagari>(&mut self, values: Vec<T>) -> NativeResult<ScriptVec<T>> {
        let element = self.conversion.type_for::<T>()?;
        self.create_vec_with_type(element, values)
    }

    /// Explicit element scope supports dynamic object handles and old generic
    /// applications without guessing a declaration from the first element.
    pub fn create_vec_with_type<T: IntoKagari>(
        &mut self,
        element: TypeArgument,
        values: Vec<T>,
    ) -> NativeResult<ScriptVec<T>> {
        self.conversion.check_type::<T>(&element)?;
        self.conversion.check_elements(values.len())?;
        let contract = Arc::new(StorageType::prepare_scoped(
            element.clone(),
            self.conversion.owner(),
        )?);
        let root = self.conversion.argument_scope(|cx| {
            let mut converted = Vec::new();
            converted
                .try_reserve_exact(values.len())
                .map_err(|_| RuntimeError::resource_limit("array initializers"))?;
            for value in values {
                converted.push(cx.encode_prepared(&element, value)?);
            }
            let id = cx
                .runtime()
                .gc()
                .alloc_array_with_contract(contract.clone(), converted)?;
            let root = cx
                .runtime()
                .root_value(Value::Array(id))
                .ok_or_else(|| RuntimeError::module_validation("array construction retention"))?;
            cx.runtime().gc_safepoint()?;
            Ok(root)
        })?;
        Ok(ScriptVec {
            root,
            element,
            access: CollectionAccess::Mutable,
            mapping: PhantomData,
        })
    }
}

impl<T> ScriptVec<T> {
    pub fn is_mutable(&self) -> bool {
        self.access == CollectionAccess::Mutable
    }

    pub fn read_only(&self) -> Self {
        let mut view = self.clone();
        view.access = CollectionAccess::ReadOnly;
        view
    }

    fn id(&self, cx: &NativeContext<'_>, write: bool) -> NativeResult<HeapObjectId> {
        cx.runtime().gc().ensure_no_native_borrow()?;
        cx.runtime().resources().ensure_execution_allowed()?;
        cx.poll()?;
        if write && !self.is_mutable() {
            return Err(RuntimeError::module_validation(
                "collection view is read-only",
            ));
        }
        match self.root.value(cx.runtime().gc()) {
            Some(Value::Array(id)) => Ok(id),
            _ => Err(RuntimeError::module_validation(
                "foreign or expired collection handle",
            )),
        }
    }

    pub fn len(&self, cx: &NativeContext<'_>) -> NativeResult<usize> {
        cx.runtime()
            .gc()
            .array_len(self.id(cx, false)?)
            .ok_or_else(|| RuntimeError::module_validation("array handle length"))
    }

    pub fn is_empty(&self, cx: &NativeContext<'_>) -> NativeResult<bool> {
        Ok(self.len(cx)? == 0)
    }

    pub fn clear(&self, cx: &NativeContext<'_>) -> NativeResult<()> {
        cx.runtime().gc().array_clear(self.id(cx, true)?)
    }

    pub fn truncate(&self, cx: &NativeContext<'_>, length: usize) -> NativeResult<()> {
        cx.runtime().gc().array_truncate(self.id(cx, true)?, length)
    }
}

impl<T: FromKagari> ScriptVec<T> {
    pub fn get(&self, cx: &mut NativeContext<'_>, index: usize) -> NativeResult<Option<T>> {
        let id = self.id(cx, false)?;
        // A detached edit buffer is unavailable, not an empty collection.
        if index >= self.len(cx)? {
            return Ok(None);
        }
        let value = cx
            .runtime()
            .gc()
            .array_get(id, index)
            .ok_or_else(|| RuntimeError::module_validation("array handle access"))?;
        cx.conversion
            .decode_prepared(&self.element, &value)
            .map(Some)
    }

    pub fn pop(&self, cx: &mut NativeContext<'_>) -> NativeResult<Option<T>> {
        let value = cx.runtime().gc().array_pop(self.id(cx, true)?)?;
        value
            .map(|value| cx.conversion.decode_prepared(&self.element, &value))
            .transpose()
    }

    pub fn remove(&self, cx: &mut NativeContext<'_>, index: usize) -> NativeResult<Option<T>> {
        let value = cx.runtime().gc().array_remove(self.id(cx, true)?, index)?;
        value
            .map(|value| cx.conversion.decode_prepared(&self.element, &value))
            .transpose()
    }

    /// Each callback runs without a storage borrow. The iteration lease rejects
    /// structural changes through every alias and releases on error or unwind.
    pub fn for_each(
        &self,
        cx: &mut NativeContext<'_>,
        mut visit: impl FnMut(&mut NativeContext<'_>, T) -> NativeResult<()>,
    ) -> NativeResult<()> {
        let id = self.id(cx, false)?;
        let _iteration = cx
            .runtime()
            .gc()
            .begin_collection_iteration(&Value::Array(id))?;
        let count = self.len(cx)?;
        cx.conversion.check_elements(count)?;
        for index in 0..count {
            cx.poll()?;
            let value = self.get(cx, index)?.ok_or_else(|| {
                RuntimeError::module_validation("collection changed during iteration")
            })?;
            visit(cx, value)?;
            cx.poll()?;
        }
        Ok(())
    }
}

impl<T: IntoKagari> ScriptVec<T> {
    pub fn set(&self, cx: &mut NativeContext<'_>, index: usize, value: T) -> NativeResult<()> {
        let id = self.id(cx, true)?;
        cx.runtime().gc().ensure_callback_mutable(id)?;
        if index >= self.len(cx)? {
            return Err(RuntimeError::module_validation("array set index"));
        }
        cx.conversion.argument_scope(|cx| {
            let value = cx.encode_prepared(&self.element, value)?;
            cx.runtime().gc().array_set(id, index, value)
        })
    }

    pub fn push(&self, cx: &mut NativeContext<'_>, value: T) -> NativeResult<()> {
        let id = self.id(cx, true)?;
        cx.runtime().gc().ensure_structure_mutable(id)?;
        cx.conversion.argument_scope(|cx| {
            let value = cx.encode_prepared(&self.element, value)?;
            cx.runtime().gc().array_push(id, value)
        })
    }

    pub fn insert(&self, cx: &mut NativeContext<'_>, index: usize, value: T) -> NativeResult<()> {
        let id = self.id(cx, true)?;
        cx.runtime().gc().ensure_structure_mutable(id)?;
        if index > self.len(cx)? {
            return Err(RuntimeError::module_validation("array insert index"));
        }
        cx.conversion.argument_scope(|cx| {
            let value = cx.encode_prepared(&self.element, value)?;
            cx.runtime().gc().array_insert(id, index, value)
        })
    }
}
