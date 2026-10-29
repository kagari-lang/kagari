//! Retained, library-owned HashMap storage and host access.
use crate::declarations::StandardDeclarations;
use crate::{
    collections::hash::{HashPayload, HashTable, input::Input, operations},
    identity,
};
use kagari_common::identity::DefinitionKind;
use kagari_runtime::{
    error::RuntimeError,
    frame::types::arguments::TypeArgument,
    native::{
        binding::NativeResult,
        catalog::DeclarationCatalog,
        conversion::{FromKagari, IntoKagari, KagariType, context::ConversionContext},
        payload::{NativeType, Opaque},
        schema::TypeRegistration,
        storage::{
            NativePayload,
            scan::{NativeScanKind, ScanBudget, ScanStatus},
        },
        typed::NativeContext,
        types::Type,
        value_handle::ScriptValue,
    },
    resource::HeapCharge,
    value::Value,
};
use std::marker::PhantomData;

/// Opaque HashMap payload. Reference slots and hash policy belong to the library.
#[derive(Debug)]
pub struct MapStorage {
    table: HashTable<Value>,
}

impl MapStorage {
    pub(crate) fn empty(cx: &NativeContext<'_>, ty: &NativeType<Self>) -> NativeResult<Self> {
        Ok(Self {
            table: HashTable::empty(cx, ty)?,
        })
    }
}

impl HashPayload for MapStorage {
    type Value = Value;

    fn table(&self) -> &HashTable<Value> {
        &self.table
    }

    fn table_mut(&mut self) -> &mut HashTable<Value> {
        &mut self.table
    }
}

// SAFETY: Every edge lives in indexed slots. Scalar bucket indexes carry no
// references. Each inspected field consumes bounded work; the
// continuation contains indices only; inserted and relocated edges are published.
unsafe impl NativePayload for MapStorage {
    type ScanState = (usize, usize);
    const SCAN_KIND: NativeScanKind = NativeScanKind::Resumable;

    fn begin_scan(&self) -> Self::ScanState {
        (0, self.table.slots.len() * 2)
    }

    fn scan<'a>(
        &'a self,
        state: &mut Self::ScanState,
        budget: &mut ScanBudget,
        visit: &mut dyn FnMut(&'a Value),
    ) -> ScanStatus {
        self.table.scan(state, budget, visit)
    }

    fn charge(&self) -> HeapCharge {
        self.table.charge()
    }
}

/// A retained HashMap preserving aliases and its selected script Hash/Eq protocol.
#[derive(Debug)]
pub struct ScriptMap<K, V> {
    pub(crate) object: Opaque<MapStorage>,
    mutable: bool,
    mapping: PhantomData<fn() -> (K, V)>,
}

impl<K, V> Clone for ScriptMap<K, V> {
    fn clone(&self) -> Self {
        Self {
            object: self.object.clone(),
            mutable: self.mutable,
            mapping: PhantomData,
        }
    }
}

impl<K: KagariType, V: KagariType> KagariType for ScriptMap<K, V> {
    fn register_type(registration: &mut TypeRegistration<'_>) -> NativeResult<Type>
    where
        Self: 'static,
    {
        registration.dependency::<K>()?;
        registration.dependency::<V>()?;
        Self::kagari_type(registration.catalog())
    }

    fn kagari_type(catalog: &DeclarationCatalog) -> NativeResult<Type> {
        catalog
            .type_reference(&identity::type_id(
                "HashMap",
                DefinitionKind::AssociatedType,
            ))?
            .apply([K::kagari_type(catalog)?, V::kagari_type(catalog)?])
    }

    fn check_type(cx: &ConversionContext<'_>, expected: &TypeArgument) -> NativeResult<()> {
        cx.check_type::<Opaque<MapStorage>>(expected)?;
        cx.check_type::<K>(&cx.parameter(expected, 0)?)?;
        cx.check_type::<V>(&cx.parameter(expected, 1)?)
    }
}

impl<K: KagariType, V: KagariType> FromKagari for ScriptMap<K, V> {
    const PRESERVES_IDENTITY: bool = true;

    fn from_kagari(
        cx: &mut ConversionContext<'_>,
        expected: &TypeArgument,
        value: &ScriptValue,
    ) -> NativeResult<Self> {
        cx.check_type::<Self>(expected)?;
        Ok(Self {
            object: Opaque::from_kagari(cx, expected, value)?,
            mutable: true,
            mapping: PhantomData,
        })
    }
}

impl<K: KagariType, V: KagariType> IntoKagari for ScriptMap<K, V> {
    fn into_kagari(
        self,
        cx: &mut ConversionContext<'_>,
        expected: &TypeArgument,
    ) -> NativeResult<ScriptValue> {
        self.check_mutable()?;
        cx.check_type::<Self>(expected)?;
        self.object.into_kagari(cx, expected)
    }
}

impl<K, V> ScriptMap<K, V> {
    pub fn is_mutable(&self) -> bool {
        self.mutable
    }

    pub fn read_only(&self) -> Self {
        let mut result = self.clone();
        result.mutable = false;
        result
    }

    fn check_mutable(&self) -> NativeResult<()> {
        if self.mutable {
            Ok(())
        } else {
            Err(RuntimeError::module_validation(
                "collection view is read-only",
            ))
        }
    }

    pub fn len(&self, cx: &NativeContext<'_>) -> NativeResult<usize> {
        operations::len(cx, &self.object.borrow(cx)?)
    }

    pub fn is_empty(&self, cx: &NativeContext<'_>) -> NativeResult<bool> {
        Ok(self.len(cx)? == 0)
    }

    pub fn clear(&self, cx: &mut NativeContext<'_>) -> NativeResult<()> {
        self.check_mutable()?;
        let object = self.object.borrow(cx)?;
        operations::clear(cx, &object)
    }
}

impl<K: KagariType + 'static, V: KagariType + 'static> ScriptMap<K, V> {
    /// Invoke the checked public constructor with concrete host type mappings.
    pub fn create(cx: &mut NativeContext<'_>) -> NativeResult<Self> {
        let (key, value) = {
            let conversion = cx.conversion_scope(cx.owner())?;
            (conversion.type_for::<K>()?, conversion.type_for::<V>()?)
        };
        Self::create_with_types(cx, &key, &value)
    }

    /// Construct using exact scoped arguments and verified Hash/Eq evidence.
    pub fn create_with_types(
        cx: &mut NativeContext<'_>,
        key: &TypeArgument,
        value: &TypeArgument,
    ) -> NativeResult<Self> {
        let declaration =
            StandardDeclarations::default()
                .catalog()?
                .type_reference(&identity::type_id(
                    "HashMap",
                    DefinitionKind::AssociatedType,
                ))?;
        let receiver = cx
            .conversion_scope(cx.owner())?
            .apply_type(&declaration, &[key.clone(), value.clone()])?;
        cx.runtime()
            .bind_constructor::<Self>(cx.owner(), &receiver)?
            .call(cx, ())
    }
}

impl<K: IntoKagari, V> ScriptMap<K, V> {
    pub fn contains_key(&self, cx: &mut NativeContext<'_>, key: K) -> NativeResult<bool> {
        let key = operations::encode(cx, &self.object, 0, key)?;
        let object = self.object.borrow(cx)?;
        Ok(operations::lookup(cx, &object, Input::Retained(&key))?
            .slot
            .is_some())
    }
}

impl<K: IntoKagari, V: FromKagari> ScriptMap<K, V> {
    pub fn get(&self, cx: &mut NativeContext<'_>, key: K) -> NativeResult<Option<V>> {
        let object = self.object.borrow(cx)?;
        let key = operations::encode(cx, &self.object, 0, key)?;
        let found = operations::lookup(cx, &object, Input::Retained(&key))?;
        let value = operations::get(cx, &object, &found)?;
        drop(found);
        value.map(|value| value.decode(cx)).transpose()
    }

    /// Commit removal before converting its retained result. Conversion errors
    /// preserve the successful removal and completed callback effects.
    pub fn remove(&self, cx: &mut NativeContext<'_>, key: K) -> NativeResult<Option<V>> {
        self.check_mutable()?;
        let object = self.object.borrow(cx)?;
        operations::ensure_mutable(cx, &object)?;
        let key = operations::encode(cx, &self.object, 0, key)?;
        let found = operations::lookup(cx, &object, Input::Retained(&key))?;
        let value = operations::get(cx, &object, &found)?;
        operations::remove(cx, &object, found)?;
        value.map(|value| value.decode(cx)).transpose()
    }
}

impl<K: IntoKagari, V: IntoKagari> ScriptMap<K, V> {
    pub fn insert(&self, cx: &mut NativeContext<'_>, key: K, value: V) -> NativeResult<()> {
        self.check_mutable()?;
        let object = self.object.borrow(cx)?;
        operations::ensure_mutable(cx, &object)?;
        let key = operations::encode(cx, &self.object, 0, key)?;
        let value = operations::encode(cx, &self.object, 1, value)?;
        operations::insert(
            cx,
            &object,
            Input::Retained(&key),
            Some(Input::Retained(&value)),
        )?;
        Ok(())
    }
}

impl<K: FromKagari, V: FromKagari> ScriptMap<K, V> {
    /// Copy a fully retained entry snapshot through the requested child adapters.
    pub fn to_vec(&self, cx: &mut NativeContext<'_>) -> NativeResult<Vec<(K, V)>> {
        let object = self.object.borrow(cx)?;
        let snapshot = operations::snapshot(cx, &object)?;
        snapshot
            .into_iter()
            .map(|(key, value)| Ok((key.decode(cx)?, value.expect("map value").decode(cx)?)))
            .collect()
    }

    /// Visit entries on demand, retaining only the current key/value pair.
    /// Replacements remain visible to later reads; structural callback edits
    /// invalidate traversal, including the final exhaustion check.
    pub fn for_each(
        &self,
        cx: &mut NativeContext<'_>,
        mut visit: impl FnMut(&mut NativeContext<'_>, K, V) -> NativeResult<()>,
    ) -> NativeResult<()> {
        let object = self.object.borrow(cx)?;
        let revision = operations::revision(cx, &object)?;
        let mut position = 0;
        loop {
            cx.poll()?;
            let Some((key, value)) = operations::retained_entry(cx, &object, revision, position)?
            else {
                return Ok(());
            };
            let key = key.decode(cx)?;
            let value = value.expect("map value").decode(cx)?;
            visit(cx, key, value)?;
            position += 1;
        }
    }
}
