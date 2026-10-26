//! Shared traced Rust hash-table payloads. Script hashing never borrows a table.
use crate::native::storage::scan::{NativeScanKind, ScanBudget, ScanStatus};
use crate::{
    error::RuntimeError,
    native::{
        binding::NativeResult,
        hash_storage::{HashMapStorage, HashSetStorage},
        storage::{NativePayload, NativeStorage, StorageContext},
        storage_type::StorageType,
    },
    resource::HeapCharge,
    value::Value,
};
use kagari_contract::standard::RuntimePrimitive;
use kagari_types::{declaration::native::NativeStorageLayout, ty::Ty};
use std::sync::Arc;

#[derive(Debug)]
pub(crate) struct MapPayload {
    pub(crate) key: Arc<StorageType>,
    pub(crate) value: Arc<StorageType>,
    pub(crate) builtin_keys: bool,
    pub(crate) entries: HashMapStorage,
}

unsafe impl NativePayload for MapPayload {
    type ScanState = ();
    const SCAN_KIND: NativeScanKind = NativeScanKind::Indivisible;

    fn begin_scan(&self) {}

    fn scan<'payload>(
        &'payload self,
        _: &mut Self::ScanState,
        budget: &mut ScanBudget,
        visit: &mut dyn FnMut(&'payload Value),
    ) -> ScanStatus {
        // std hides inspected buckets/tombstones. Saturate unknown indivisible
        // work rather than claim reported entry capacity is a finite scan bound.
        budget.charge(usize::MAX);
        for (key, value) in self.entries.iter() {
            visit(key.value());
            visit(value);
        }

        ScanStatus::Complete
    }

    fn charge(&self) -> HeapCharge {
        HeapCharge::new(self.entries.len(), self.entries.pressure_bytes())
    }
}

#[derive(Debug)]
pub(crate) struct SetPayload {
    pub(crate) element: Arc<StorageType>,
    pub(crate) builtin_keys: bool,
    pub(crate) entries: HashSetStorage,
}

unsafe impl NativePayload for SetPayload {
    type ScanState = ();
    const SCAN_KIND: NativeScanKind = NativeScanKind::Indivisible;

    fn begin_scan(&self) {}

    fn scan<'payload>(
        &'payload self,
        _: &mut Self::ScanState,
        budget: &mut ScanBudget,
        visit: &mut dyn FnMut(&'payload Value),
    ) -> ScanStatus {
        // The indexed library layout will replace this unbounded std iterator.
        budget.charge(usize::MAX);
        for key in self.entries.iter() {
            visit(key.value());
        }

        ScanStatus::Complete
    }

    fn charge(&self) -> HeapCharge {
        HeapCharge::new(self.entries.len(), self.entries.pressure_bytes())
    }
}

fn builtin_keys(context: &StorageContext<'_>, key: &Ty) -> NativeResult<bool> {
    let hash = context.selected(0)?;
    let equal = context.selected(1)?;
    if hash.params.as_ref() != [key.clone()] || equal.params.as_ref() != [key.clone(), key.clone()]
    {
        return Err(RuntimeError::module_validation(
            "hash storage callable receiver differs from its key type",
        ));
    }
    Ok(hash.primitive == Some(RuntimePrimitive::ValueHash)
        && equal.primitive == Some(RuntimePrimitive::ValueEq))
}

impl NativeStorage {
    pub(crate) fn map(key: usize, value: usize) -> Self {
        Self::with_layout(NativeStorageLayout::Map { key, value }, move |context| {
            let key_contract = context.element_contract(key)?;
            let value_contract = context.element_contract(value)?;
            let builtin_keys = builtin_keys(context, &key_contract.ty)?;
            Ok(MapPayload {
                key: key_contract,
                value: value_contract,
                builtin_keys,
                entries: HashMapStorage::new(),
            })
        })
    }

    pub(crate) fn set(element: usize) -> Self {
        Self::with_layout(NativeStorageLayout::Set { element }, move |context| {
            let contract = context.element_contract(element)?;
            let builtin_keys = builtin_keys(context, &contract.ty)?;
            Ok(SetPayload {
                element: contract,
                builtin_keys,
                entries: HashSetStorage::new(),
            })
        })
    }
}
