//! Retained, library-owned HashSet storage and host access.
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
            value::StoredValue,
        },
        typed::NativeContext,
        types::Type,
        value_handle::ScriptValue,
    },
    resource::HeapCharge,
};
use std::{marker::PhantomData, slice};

/// Opaque HashSet payload. Reference slots and hash policy belong to the library.
#[derive(Debug)]
pub struct SetStorage {
    table: HashTable<()>,
}

impl SetStorage {
    pub(crate) fn empty(cx: &NativeContext<'_>, ty: &NativeType<Self>) -> NativeResult<Self> {
        Ok(Self {
            table: HashTable::empty(cx, ty)?,
        })
    }
}

impl HashPayload for SetStorage {
    type Value = ();

    fn table(&self) -> &HashTable<()> {
        &self.table
    }

    fn table_mut(&mut self) -> &mut HashTable<()> {
        &mut self.table
    }
}

// SAFETY: Every edge lives in indexed slots. Scalar bucket indexes carry no
// references. Each inspected field consumes bounded work; the
// continuation contains indices only; inserted and relocated edges are published.
unsafe impl NativePayload for SetStorage {
    type ScanState = (usize, usize);
    const SCAN_KIND: NativeScanKind = NativeScanKind::Resumable;

    fn begin_scan(&self) -> Self::ScanState {
        (0, self.table.slots.len())
    }

    fn scan<'a>(
        &'a self,
        state: &mut Self::ScanState,
        budget: &mut ScanBudget,
        visit: &mut dyn FnMut(&'a StoredValue),
    ) -> ScanStatus {
        self.table.scan(state, budget, visit)
    }

    fn charge(&self) -> HeapCharge {
        self.table.charge()
    }
}

/// A retained HashSet preserving aliases and its selected script Hash/Eq protocol.
#[derive(Debug)]
pub struct ScriptSet<T> {
    pub(crate) object: Opaque<SetStorage>,
    mutable: bool,
    mapping: PhantomData<fn() -> T>,
}

impl<T> Clone for ScriptSet<T> {
    fn clone(&self) -> Self {
        Self {
            object: self.object.clone(),
            mutable: self.mutable,
            mapping: PhantomData,
        }
    }
}

impl<T: KagariType> KagariType for ScriptSet<T> {
    fn register_type(registration: &mut TypeRegistration<'_>) -> NativeResult<Type>
    where
        Self: 'static,
    {
        registration.dependency::<T>()?;
        Self::kagari_type(registration.catalog())
    }

    fn kagari_type(catalog: &DeclarationCatalog) -> NativeResult<Type> {
        catalog
            .type_reference(&identity::type_id(
                "HashSet",
                DefinitionKind::AssociatedType,
            ))?
            .apply([T::kagari_type(catalog)?])
    }

    fn check_type(cx: &ConversionContext<'_>, expected: &TypeArgument) -> NativeResult<()> {
        cx.check_type::<Opaque<SetStorage>>(expected)?;
        cx.check_type::<T>(&cx.parameter(expected, 0)?)
    }
}

impl<T: KagariType> FromKagari for ScriptSet<T> {
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

impl<T: KagariType> IntoKagari for ScriptSet<T> {
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

impl<T> ScriptSet<T> {
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

impl<T: KagariType + 'static> ScriptSet<T> {
    /// Invoke the checked public constructor with a concrete host element mapping.
    pub fn create(cx: &mut NativeContext<'_>) -> NativeResult<Self> {
        let element = cx.conversion_scope(cx.owner())?.type_for::<T>()?;
        Self::create_with_type(cx, &element)
    }

    /// Construct with the exact element scope and verified Hash/Eq evidence.
    pub fn create_with_type(
        cx: &mut NativeContext<'_>,
        element: &TypeArgument,
    ) -> NativeResult<Self> {
        let declaration =
            StandardDeclarations::default()
                .catalog()?
                .type_reference(&identity::type_id(
                    "HashSet",
                    DefinitionKind::AssociatedType,
                ))?;
        let receiver = cx
            .conversion_scope(cx.owner())?
            .apply_type(&declaration, slice::from_ref(element))?;
        cx.runtime()
            .bind_constructor::<Self>(cx.owner(), &receiver)?
            .call(cx, ())
    }
}

impl<T: IntoKagari> ScriptSet<T> {
    pub fn contains(&self, cx: &mut NativeContext<'_>, key: T) -> NativeResult<bool> {
        let key = operations::encode(cx, &self.object, 0, key)?;
        let object = self.object.borrow(cx)?;
        Ok(operations::lookup(cx, &object, Input::Retained(&key))?
            .slot
            .is_some())
    }

    pub fn insert(&self, cx: &mut NativeContext<'_>, key: T) -> NativeResult<bool> {
        self.check_mutable()?;
        let object = self.object.borrow(cx)?;
        operations::ensure_mutable(cx, &object)?;
        let key = operations::encode(cx, &self.object, 0, key)?;
        operations::insert(cx, &object, Input::Retained(&key), None)
    }

    pub fn remove(&self, cx: &mut NativeContext<'_>, key: T) -> NativeResult<bool> {
        self.check_mutable()?;
        let object = self.object.borrow(cx)?;
        operations::ensure_mutable(cx, &object)?;
        let key = operations::encode(cx, &self.object, 0, key)?;
        let found = operations::lookup(cx, &object, Input::Retained(&key))?;
        operations::remove(cx, &object, found)
    }
}

impl<T: FromKagari> ScriptSet<T> {
    /// Copy a fully retained member snapshot through the requested child adapter.
    pub fn to_vec(&self, cx: &mut NativeContext<'_>) -> NativeResult<Vec<T>> {
        let object = self.object.borrow(cx)?;
        operations::snapshot(cx, &object)?
            .into_iter()
            .map(|(key, _)| key.decode(cx))
            .collect()
    }

    /// Visit members on demand, retaining only the current member. Structural
    /// callback edits invalidate traversal, including the final exhaustion check.
    pub fn for_each(
        &self,
        cx: &mut NativeContext<'_>,
        mut visit: impl FnMut(&mut NativeContext<'_>, T) -> NativeResult<()>,
    ) -> NativeResult<()> {
        let object = self.object.borrow(cx)?;
        let revision = operations::revision(cx, &object)?;
        let mut position = 0;
        loop {
            cx.poll()?;
            let Some((key, _)) = operations::retained_entry(cx, &object, revision, position)?
            else {
                return Ok(());
            };
            let key = key.decode(cx)?;
            visit(cx, key)?;
            position += 1;
        }
    }
}
