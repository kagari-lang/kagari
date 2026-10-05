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

/// A retained map using the Hash/Eq implementation selected at construction.
#[derive(Debug)]
pub struct ScriptMap<K, V> {
    handle: HashHandle,
    value: TypeArgument,
    mapping: PhantomData<fn() -> (K, V)>,
}

impl<K, V> Clone for ScriptMap<K, V> {
    fn clone(&self) -> Self {
        Self {
            handle: self.handle.clone(),
            value: self.value.clone(),
            mapping: PhantomData,
        }
    }
}

impl<K: KagariType, V: KagariType> KagariType for ScriptMap<K, V> {
    fn kagari_type(catalog: &DeclarationCatalog) -> NativeResult<Type> {
        Ok(Type::from_semantic(Ty::Map {
            key: Box::new(K::kagari_type(catalog)?.abi().clone()),
            value: Box::new(V::kagari_type(catalog)?.abi().clone()),
            access: CollectionAccess::Mutable,
        }))
    }

    fn check_type(cx: &ConversionContext<'_>, expected: &TypeArgument) -> NativeResult<()> {
        if !matches!(expected.ty(), Ty::Map { .. }) {
            return Err(RuntimeError::module_validation(
                "ScriptMap requires a map type",
            ));
        }
        cx.check_type::<K>(&cx.parameter(expected, 0)?)?;
        cx.check_type::<V>(&cx.parameter(expected, 1)?)
    }
}

impl<K: KagariType, V: KagariType> FromKagari for ScriptMap<K, V> {
    const PRESERVES_IDENTITY: bool = true;

    fn from_kagari(
        cx: &mut ConversionContext<'_>,
        expected: &TypeArgument,
        value: &Value,
    ) -> NativeResult<Self> {
        cx.check_type::<Self>(expected)?;
        cx.check_value(expected, value)?;
        let (Ty::Map { access, .. }, Value::Map(_)) = (expected.ty(), value) else {
            return Err(RuntimeError::module_validation("ScriptMap conversion"));
        };
        Ok(Self {
            handle: HashHandle::new(cx, value, cx.parameter(expected, 0)?, *access)?,
            value: cx.parameter(expected, 1)?,
            mapping: PhantomData,
        })
    }
}

impl<K: KagariType, V: KagariType> IntoKagari for ScriptMap<K, V> {
    fn into_kagari(
        self,
        cx: &mut ConversionContext<'_>,
        expected: &TypeArgument,
    ) -> NativeResult<Value> {
        cx.check_type::<Self>(expected)?;
        let Ty::Map { access, .. } = expected.ty() else {
            unreachable!("checked map type");
        };
        self.handle.into_value(cx, expected, *access)
    }
}

impl<K, V> ScriptMap<K, V> {
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
            .map_len(id)
            .ok_or_else(|| RuntimeError::module_validation("map handle length"))
    }

    pub fn is_empty(&self, cx: &NativeContext<'_>) -> NativeResult<bool> {
        Ok(self.len(cx)? == 0)
    }

    pub fn clear(&self, cx: &NativeContext<'_>) -> NativeResult<()> {
        let (_, id) = self.handle.value(cx, true)?;
        cx.runtime().gc().map_clear(id)
    }
}

impl<K: IntoKagari, V> ScriptMap<K, V> {
    fn read_value(&self, cx: &NativeContext<'_>, key: K) -> NativeResult<Option<Value>> {
        let (collection, id) = self.handle.value(cx, false)?;
        let mut conversion = ConversionContext::new(cx.runtime(), cx.conversion.owner())?;
        let key = conversion.encode_prepared(&self.handle.key, key)?;
        match self.handle.lookup(cx, &collection, &key)? {
            Some((hash, token)) => cx.runtime().gc().custom_get(&collection, hash, token),
            None => Ok(cx.runtime().gc().map_get(id, &key)),
        }
    }

    pub fn contains_key(&self, cx: &NativeContext<'_>, key: K) -> NativeResult<bool> {
        Ok(self.read_value(cx, key)?.is_some())
    }
}

impl<K: IntoKagari, V: FromKagari> ScriptMap<K, V> {
    pub fn get(&self, cx: &mut NativeContext<'_>, key: K) -> NativeResult<Option<V>> {
        self.read_value(cx, key)?
            .map(|value| cx.conversion.decode_prepared(&self.value, &value))
            .transpose()
    }

    /// Removal commits before result conversion; a conversion failure preserves
    /// completed script effects and the successful removal.
    pub fn remove(&self, cx: &mut NativeContext<'_>, key: K) -> NativeResult<Option<V>> {
        let (collection, id) = self.handle.value(cx, true)?;
        let heap = cx.runtime().gc();
        heap.ensure_structure_mutable(id)?;
        let mut conversion = ConversionContext::new(cx.runtime(), cx.conversion.owner())?;
        let key = conversion.encode_prepared(&self.handle.key, key)?;
        let value = match self.handle.lookup(cx, &collection, &key)? {
            Some((hash, token)) => {
                let value = heap.custom_get(&collection, hash, token)?;
                heap.custom_remove(&collection, hash, token)?;
                value
            }
            None => heap.map_remove(id, &key)?,
        };
        value
            .map(|value| conversion.decode_prepared(&self.value, &value))
            .transpose()
    }
}

impl<K: IntoKagari, V: IntoKagari> ScriptMap<K, V> {
    pub fn insert(&self, cx: &NativeContext<'_>, key: K, value: V) -> NativeResult<()> {
        let (collection, id) = self.handle.value(cx, true)?;
        cx.runtime().gc().ensure_key_mutable(id)?;
        let mut conversion = ConversionContext::new(cx.runtime(), cx.conversion.owner())?;
        let key = conversion.encode_prepared(&self.handle.key, key)?;
        let value = conversion.encode_prepared(&self.value, value)?;
        match self.handle.lookup(cx, &collection, &key)? {
            Some((hash, token)) => {
                cx.runtime()
                    .gc()
                    .custom_insert(&collection, hash, token, key, value)
            }
            None => cx.runtime().gc().map_insert(id, key, value),
        }
    }
}

impl<K: FromKagari, V: FromKagari> ScriptMap<K, V> {
    /// Visit a rooted snapshot. Existing values may be replaced by callbacks;
    /// structural changes through any alias remain forbidden until return.
    pub fn for_each(
        &self,
        cx: &mut NativeContext<'_>,
        mut visit: impl FnMut(&mut NativeContext<'_>, K, V) -> NativeResult<()>,
    ) -> NativeResult<()> {
        let (collection, id) = self.handle.value(cx, false)?;
        let heap = cx.runtime().gc();
        let _iteration = heap.begin_collection_iteration(&collection)?;
        cx.conversion.check_elements(self.len(cx)?)?;
        let entries = heap
            .map_snapshot(id)
            .ok_or_else(|| RuntimeError::module_validation("map snapshot"))?;
        let roots = heap
            .root_execution_values(
                entries
                    .into_iter()
                    .flat_map(|(key, value)| [key, value])
                    .collect(),
            )
            .ok_or_else(|| RuntimeError::module_validation("map snapshot retention"))?;
        for index in 0..self.len(cx)? {
            cx.poll()?;
            let key = roots
                .get(heap, index * 2)
                .ok_or_else(|| RuntimeError::module_validation("map snapshot key"))?;
            let value = roots
                .get(heap, index * 2 + 1)
                .ok_or_else(|| RuntimeError::module_validation("map snapshot value"))?;
            let key = cx.conversion.decode_prepared(&self.handle.key, &key)?;
            let value = cx.conversion.decode_prepared(&self.value, &value)?;
            visit(cx, key, value)?;
            cx.poll()?;
        }
        Ok(())
    }
}
