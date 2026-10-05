use crate::{
    error::RuntimeError,
    frame::types::arguments::TypeArgument,
    native::{
        binding::NativeResult,
        catalog::DeclarationCatalog,
        collections::hash::HashHandle,
        conversion::{FromKagari, IntoKagari, KagariType, context::ConversionContext},
        typed::NativeContext,
        types::Type,
    },
    value::Value,
};
use kagari_types::{collection::CollectionAccess, ty::Ty};
use std::marker::PhantomData;

/// A retained set preserving the selected script key protocol and aliases.
#[derive(Debug)]
pub struct ScriptSet<T> {
    handle: HashHandle,
    mapping: PhantomData<fn() -> T>,
}

impl<T> Clone for ScriptSet<T> {
    fn clone(&self) -> Self {
        Self {
            handle: self.handle.clone(),
            mapping: PhantomData,
        }
    }
}

impl<T: KagariType> KagariType for ScriptSet<T> {
    fn kagari_type(catalog: &DeclarationCatalog) -> NativeResult<Type> {
        Ok(Type::from_semantic(Ty::Set(
            Box::new(T::kagari_type(catalog)?.abi().clone()),
            CollectionAccess::Mutable,
        )))
    }

    fn check_type(cx: &ConversionContext<'_>, expected: &TypeArgument) -> NativeResult<()> {
        if !matches!(expected.ty(), Ty::Set(..)) {
            return Err(RuntimeError::module_validation(
                "ScriptSet requires a set type",
            ));
        }
        cx.check_type::<T>(&cx.parameter(expected, 0)?)
    }
}

impl<T: KagariType> FromKagari for ScriptSet<T> {
    const PRESERVES_IDENTITY: bool = true;

    fn from_kagari(
        cx: &mut ConversionContext<'_>,
        expected: &TypeArgument,
        value: &Value,
    ) -> NativeResult<Self> {
        cx.check_type::<Self>(expected)?;
        cx.check_value(expected, value)?;
        let (Ty::Set(_, access), Value::Set(_)) = (expected.ty(), value) else {
            return Err(RuntimeError::module_validation("ScriptSet conversion"));
        };
        Ok(Self {
            handle: HashHandle::new(cx, value, cx.parameter(expected, 0)?, *access)?,
            mapping: PhantomData,
        })
    }
}

impl<T: KagariType> IntoKagari for ScriptSet<T> {
    fn into_kagari(
        self,
        cx: &mut ConversionContext<'_>,
        expected: &TypeArgument,
    ) -> NativeResult<Value> {
        cx.check_type::<Self>(expected)?;
        let Ty::Set(_, access) = expected.ty() else {
            unreachable!("checked set type");
        };
        self.handle.into_value(cx, expected, *access)
    }
}

impl<T> ScriptSet<T> {
    pub fn is_mutable(&self) -> bool {
        self.handle.access == CollectionAccess::Mutable
    }

    pub fn read_only(&self) -> Self {
        let mut view = self.clone();
        view.handle.access = CollectionAccess::ReadOnly;
        view
    }

    pub fn len(&self, cx: &NativeContext<'_>) -> NativeResult<usize> {
        let (_, id) = self.handle.value(cx, false)?;
        cx.runtime()
            .gc()
            .set_len(id)
            .ok_or_else(|| RuntimeError::module_validation("set handle length"))
    }

    pub fn is_empty(&self, cx: &NativeContext<'_>) -> NativeResult<bool> {
        Ok(self.len(cx)? == 0)
    }

    pub fn clear(&self, cx: &NativeContext<'_>) -> NativeResult<()> {
        let (_, id) = self.handle.value(cx, true)?;
        cx.runtime().gc().set_clear(id)
    }
}

impl<T: IntoKagari> ScriptSet<T> {
    pub fn contains(&self, cx: &NativeContext<'_>, key: T) -> NativeResult<bool> {
        let (collection, id) = self.handle.value(cx, false)?;
        let mut conversion = ConversionContext::new(cx.runtime(), cx.conversion.owner())?;
        let key = conversion.encode_prepared(&self.handle.key, key)?;
        match self.handle.lookup(cx, &collection, &key)? {
            Some((_, token)) => Ok(token != -1),
            None => cx
                .runtime()
                .gc()
                .set_contains(id, &key)
                .ok_or_else(|| RuntimeError::module_validation("set key lookup")),
        }
    }

    pub fn insert(&self, cx: &NativeContext<'_>, key: T) -> NativeResult<bool> {
        let (collection, id) = self.handle.value(cx, true)?;
        cx.runtime().gc().ensure_key_mutable(id)?;
        let mut conversion = ConversionContext::new(cx.runtime(), cx.conversion.owner())?;
        let key = conversion.encode_prepared(&self.handle.key, key)?;
        match self.handle.lookup(cx, &collection, &key)? {
            Some((hash, token)) => {
                cx.runtime()
                    .gc()
                    .custom_insert(&collection, hash, token, key, Value::Unit)?;
                Ok(token == -1)
            }
            None => cx.runtime().gc().set_insert(id, key),
        }
    }

    pub fn remove(&self, cx: &NativeContext<'_>, key: T) -> NativeResult<bool> {
        let (collection, id) = self.handle.value(cx, true)?;
        cx.runtime().gc().ensure_structure_mutable(id)?;
        let mut conversion = ConversionContext::new(cx.runtime(), cx.conversion.owner())?;
        let key = conversion.encode_prepared(&self.handle.key, key)?;
        match self.handle.lookup(cx, &collection, &key)? {
            Some((hash, token)) => {
                cx.runtime().gc().custom_remove(&collection, hash, token)?;
                Ok(token != -1)
            }
            None => cx.runtime().gc().set_remove(id, &key),
        }
    }
}

impl<T: FromKagari> ScriptSet<T> {
    /// Visit a rooted snapshot with structural mutation blocked across aliases.
    pub fn for_each(
        &self,
        cx: &mut NativeContext<'_>,
        mut visit: impl FnMut(&mut NativeContext<'_>, T) -> NativeResult<()>,
    ) -> NativeResult<()> {
        let (collection, id) = self.handle.value(cx, false)?;
        let heap = cx.runtime().gc();
        let _iteration = heap.begin_collection_iteration(&collection)?;
        cx.conversion.check_elements(self.len(cx)?)?;
        let entries = heap
            .set_snapshot(id)
            .ok_or_else(|| RuntimeError::module_validation("set snapshot"))?;
        let roots = heap
            .root_execution_values(entries)
            .ok_or_else(|| RuntimeError::module_validation("set snapshot retention"))?;
        for index in 0..self.len(cx)? {
            cx.poll()?;
            let value = roots
                .get(heap, index)
                .ok_or_else(|| RuntimeError::module_validation("set snapshot value"))?;
            let value = cx.conversion.decode_prepared(&self.handle.key, &value)?;
            visit(cx, value)?;
            cx.poll()?;
        }
        Ok(())
    }
}
