//! Ordinary structs borrow initialized inline bytes under the heap storage guard.
mod codec;

use crate::{
    error::{RuntimeError, RuntimeErrorKind},
    gc::{
        GcHeap, HeapObjectId,
        directory::descriptors::Descriptor,
        weak_cache::{WeakCache, WeakCacheToken},
    },
    module::{
        StructLayoutRef,
        layout_storage::{StoredField, StructStorageKey, StructStorageLayout},
    },
    native::storage::scan::{ScanBudget, ScanStatus},
    value::{StructValueField, Value, scalar},
    value_check::matches_type_in,
};
use kagari_contract::representation::builtin_representation;
use kagari_types::{scalar::BuiltinType, ty::Ty};
use std::sync::Arc;

#[derive(Debug, Default)]
pub(super) struct StructStorageLayouts {
    entries: WeakCache<StructStorageKey, Descriptor>,
}

impl StructStorageLayouts {
    /// Unlink the exact admission retired by the original descriptor directory.
    /// The directory establishes deadness; token identity protects replacements.
    /// The cache precharges its fixed structural work before inspecting a slot;
    /// no semantic key components or collision chains are visited. On Pending the
    /// caller must retain the incoming token for retry. On Complete, destroy both
    /// incoming and detached tokens outside the cache borrow.
    pub(super) fn release(
        &mut self,
        token: &WeakCacheToken<StructStorageKey, Descriptor>,
        budget: &mut ScanBudget,
    ) -> (
        ScanStatus,
        Option<WeakCacheToken<StructStorageKey, Descriptor>>,
    ) {
        self.entries.remove(token, budget)
    }
}

/// Unpublished values already admitted by the allocation owner.
#[derive(Debug)]
pub(super) struct StructInitializer {
    pub(super) descriptor: Arc<Descriptor>,
    pub(super) values: Vec<Value>,
}

/// Views cannot escape the heap's storage borrow; sweep may destroy their allocation.
#[derive(Debug, Clone, Copy)]
pub(super) struct StructView<'a> {
    pub(super) description: &'a StructStorageLayout,
    pub(super) bytes: &'a [u8],
}

#[derive(Debug)]
pub(super) struct StructViewMut<'a> {
    pub(super) description: &'a StructStorageLayout,
    pub(super) bytes: &'a mut [u8],
}

impl StructView<'_> {
    pub(super) fn get(&self, slot: usize) -> Option<Value> {
        match *self.description.fields.get(slot)? {
            StoredField::Scalar { offset, encoding } => scalar::decode(
                encoding.representation,
                codec::read_scalar(self.bytes, offset, encoding)?,
            ),
            StoredField::Managed { offset } => codec::read_managed(self.bytes, offset),
        }
    }

    pub(super) fn scalar(&self, slot: usize, expected: BuiltinType) -> Option<u64> {
        let StoredField::Scalar { offset, encoding } = *self.description.fields.get(slot)? else {
            return None;
        };
        (encoding.ty == expected)
            .then(|| codec::read_scalar(self.bytes, offset, encoding))
            .flatten()
    }

    pub(super) fn trace(&self, visit: &mut dyn FnMut(Value)) -> Option<()> {
        for slot in self.description.references.iter().rev() {
            visit(self.get(*slot)?);
        }
        Some(())
    }
}

impl StructViewMut<'_> {
    pub(super) fn set(&mut self, slot: usize, value: Value) -> Option<()> {
        match *self.description.fields.get(slot)? {
            StoredField::Scalar { offset, encoding } => {
                if !value.has_representation(encoding.representation) {
                    return None;
                }
                codec::write_scalar(self.bytes, offset, encoding, scalar::encode(&value)?)
            }
            StoredField::Managed { offset } => codec::write_managed(self.bytes, offset, value),
        }
    }

    pub(super) fn set_scalar(
        &mut self,
        slot: usize,
        expected: BuiltinType,
        bits: u64,
    ) -> Option<()> {
        let StoredField::Scalar { offset, encoding } = *self.description.fields.get(slot)? else {
            return None;
        };
        if encoding.ty != expected {
            return None;
        }
        codec::write_scalar(self.bytes, offset, encoding, bits)
    }
}

impl GcHeap {
    pub(super) fn prepare_struct_storage(
        &self,
        layout: StructLayoutRef,
        values: Vec<Value>,
    ) -> Result<StructInitializer, RuntimeError> {
        if values.len() != layout.layout().fields.len() {
            return Err(field_error("invalid heap target, index, or payload"));
        }
        let descriptor = self.struct_storage_description(layout)?;
        let description = descriptor.structure_layout();
        // Closed scalar domains need no lexical type resolution. Managed fields
        // still require heap admission and nominal/environment compatibility.
        for (slot, (field, value)) in description.fields.iter().zip(&values).enumerate() {
            let admitted = match *field {
                StoredField::Scalar { encoding, .. } => {
                    value.has_representation(encoding.representation)
                        && scalar::encode(value).is_some_and(|bits| {
                            scalar::admits_payload(encoding.representation, Some(encoding.ty), bits)
                        })
                }
                StoredField::Managed { .. } => {
                    self.valid_payload(value)
                        && description
                            .semantic
                            .field_type(slot)
                            .is_some_and(|(ty, environment)| {
                                matches_type_in(
                                    self,
                                    value,
                                    ty,
                                    description.semantic.module(),
                                    environment,
                                )
                            })
                }
            };
            if !admitted {
                return Err(field_error("invalid heap target, index, or payload"));
            }
        }
        Ok(StructInitializer { descriptor, values })
    }

    fn struct_storage_description(
        &self,
        layout: StructLayoutRef,
    ) -> Result<Arc<Descriptor>, RuntimeError> {
        let key = layout.storage_key();
        let mut layouts = self
            .struct_layouts
            .try_borrow_mut()
            .map_err(|_| RuntimeError::module_validation("struct descriptions borrowed"))?;
        if let Some(layout) = layouts.entries.get(&key) {
            return Ok(layout);
        }
        let layout = Descriptor::structure(
            StructStorageLayout::prepare(layout)
                .ok_or_else(|| RuntimeError::module_validation("closed struct storage layout"))?,
        )?;
        let insertion = layouts.entries.prepare_insert(key, &layout)?;
        let token = insertion.token();
        // Reservation finishes semantic lookup before the descriptor association
        // is installed. Commit cannot fail, including when replacing a dead hint.
        let registered = self
            .objects
            .try_borrow_mut()
            .map_err(|_| RuntimeError::module_validation("object storage borrowed"))
            .and_then(|mut objects| objects.register_struct_descriptor(&layout, token.clone()));
        if let Err(error) = registered {
            // Keep the token alive until both the insertion guard and cache borrow
            // end, so abandoning an unpublished key cannot destroy it under either.
            drop(insertion);
            drop(layouts);
            drop(token);
            return Err(error);
        }
        let displaced = insertion.commit();
        drop(layouts);
        drop(displaced);
        drop(token);
        Ok(layout)
    }
}

impl GcHeap {
    /// Query checked heap metadata for protected backend transport.
    ///
    /// # Safety
    /// Keep managed inputs live through this query. Type/owner validation cannot
    /// establish the lifetime of an expired direct reference.
    /// With the current checked-ID representation, invalid-ID rejection probes
    /// must remain non-dereferencing; replace them with inert identity or retained
    /// wrapper checks before converting these paths to direct references.
    #[doc(hidden)]
    pub unsafe fn struct_layout_transport(&self, id: HeapObjectId) -> Option<StructLayoutRef> {
        self.struct_layout(id)
    }

    pub(crate) fn struct_layout(&self, id: HeapObjectId) -> Option<StructLayoutRef> {
        self.with_struct(id, |storage| storage.description.semantic.clone())
    }

    pub(crate) fn struct_name(&self, id: HeapObjectId) -> Option<String> {
        self.with_struct(id, |storage| {
            let layout = &storage.description.semantic;
            layout
                .module()
                .definition_name(layout.layout().declaration)
                .expect("verified struct definition")
                .to_owned()
        })
    }

    /// Read a checked backend snapshot without adding independent retention.
    ///
    /// # Safety
    /// Keep source objects live through the read. Protect copied managed edges
    /// or identities before callbacks, collection or source mutation can remove
    /// their last traced owner. A returned snapshot is not a root.
    /// With the current checked-ID representation, invalid-ID rejection probes
    /// must remain non-dereferencing; replace them with inert identity or retained
    /// wrapper checks before converting these paths to direct references.
    #[doc(hidden)]
    pub unsafe fn struct_snapshot_transport(
        &self,
        id: HeapObjectId,
    ) -> Option<(String, Vec<StructValueField>)> {
        self.struct_snapshot(id)
    }

    pub(crate) fn struct_snapshot(
        &self,
        id: HeapObjectId,
    ) -> Option<(String, Vec<StructValueField>)> {
        self.with_struct(id, |storage| {
            let layout = &storage.description.semantic;
            let fields = layout
                .layout()
                .fields
                .iter()
                .enumerate()
                .map(|(slot, field)| {
                    Some(StructValueField {
                        name: field.name.clone(),
                        value: storage.get(slot)?,
                    })
                })
                .collect::<Option<Vec<_>>>()?;
            Some((
                layout
                    .module()
                    .definition_name(layout.layout().declaration)
                    .expect("verified struct definition")
                    .to_owned(),
                fields,
            ))
        })
        .flatten()
    }

    /// Read a checked backend snapshot without adding independent retention.
    ///
    /// # Safety
    /// Keep source objects live through the read. Protect copied managed edges
    /// or identities before callbacks, collection or source mutation can remove
    /// their last traced owner. A returned snapshot is not a root.
    /// With the current checked-ID representation, invalid-ID rejection probes
    /// must remain non-dereferencing; replace them with inert identity or retained
    /// wrapper checks before converting these paths to direct references.
    #[doc(hidden)]
    pub unsafe fn struct_get_slot_transport(
        &self,
        id: HeapObjectId,
        expected: &StructLayoutRef,
        slot: usize,
    ) -> Option<Value> {
        self.struct_get_slot(id, expected, slot)
    }

    pub(crate) fn struct_get_slot(
        &self,
        id: HeapObjectId,
        expected: &StructLayoutRef,
        slot: usize,
    ) -> Option<Value> {
        self.with_struct(id, |storage| {
            storage
                .description
                .semantic
                .matches(expected)
                .then(|| storage.get(slot))
                .flatten()
        })
        .flatten()
    }

    /// Mutate backend storage through existing type, bounds and publication checks.
    ///
    /// # Safety
    /// Keep the target and every managed input live through checked publication
    /// and commit. Preserve source protection across preparation; an unrooted copy
    /// cannot be used after callbacks or mutation remove its last traced owner.
    /// With the current checked-ID representation, invalid-ID rejection probes
    /// must remain non-dereferencing; replace them with inert identity or retained
    /// wrapper checks before converting these paths to direct references.
    #[doc(hidden)]
    pub unsafe fn struct_set_slot_transport(
        &self,
        id: HeapObjectId,
        expected: &StructLayoutRef,
        slot: usize,
        next_value: Value,
    ) -> Result<(), RuntimeError> {
        self.struct_set_slot(id, expected, slot, next_value)
    }

    pub(crate) fn struct_set_slot(
        &self,
        id: HeapObjectId,
        expected: &StructLayoutRef,
        slot: usize,
        next_value: Value,
    ) -> Result<(), RuntimeError> {
        self.ensure_execution_allowed()?;
        if !self.valid_payload(&next_value) {
            return Err(field_error("invalid heap payload"));
        }
        let field = expected
            .layout()
            .fields
            .get(slot)
            .ok_or_else(|| field_error("invalid field slot"))?;
        if !field.mutable
            || !expected.field_type(slot).is_some_and(|(ty, environment)| {
                matches_type_in(self, &next_value, ty, expected.module(), environment)
            })
        {
            return Err(field_error(
                "field is read-only or value has the wrong concrete type",
            ));
        }
        self.publish_value(next_value)?;
        self.with_struct_mut(id, |mut storage| {
            if !storage.description.semantic.matches(expected) {
                return Err(field_error("struct layout mismatch"));
            }
            storage.set(slot, next_value).ok_or_else(|| {
                self.resources
                    .quarantine("struct storage does not match its layout")
            })
        })
        .ok_or_else(|| field_error("invalid heap target"))?
    }
}

fn field_error(message: &'static str) -> RuntimeError {
    RuntimeError::new(RuntimeErrorKind::ScriptTrap, message)
}

impl GcHeap {
    /// Read a checked scalar field without constructing a transport Value.
    pub(crate) fn struct_scalar(
        &self,
        id: HeapObjectId,
        expected: &StructLayoutRef,
        slot: usize,
        ty: BuiltinType,
    ) -> Option<u64> {
        self.with_struct(id, |storage| {
            storage
                .description
                .semantic
                .matches(expected)
                .then(|| storage.scalar(slot, ty))
                .flatten()
        })
        .flatten()
    }

    /// Preserve write admission and commit order while accepting scalar-bank bits.
    pub(crate) fn set_struct_scalar(
        &self,
        id: HeapObjectId,
        expected: &StructLayoutRef,
        slot: usize,
        ty: BuiltinType,
        bits: u64,
    ) -> Result<(), RuntimeError> {
        self.ensure_execution_allowed()?;
        if !scalar::admits_payload(builtin_representation(ty), Some(ty), bits) {
            return Err(field_error("invalid heap payload"));
        }
        let field = expected
            .layout()
            .fields
            .get(slot)
            .ok_or_else(|| field_error("invalid field slot"))?;
        if !field.mutable || field.ty != Ty::Builtin(ty) {
            return Err(field_error(
                "field is read-only or value has the wrong concrete type",
            ));
        }
        self.with_struct_mut(id, |mut storage| {
            if !storage.description.semantic.matches(expected) {
                return Err(field_error("struct layout mismatch"));
            }
            storage.set_scalar(slot, ty, bits).ok_or_else(|| {
                self.resources
                    .quarantine("struct storage does not match its layout")
            })
        })
        .ok_or_else(|| field_error("invalid heap target"))?
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Runtime,
        gc::{storage::HeapObject, weak_cache::WEAK_CACHE_REMOVE_WORK},
        test_support::layouts::layout,
    };

    #[test]
    fn failed_struct_preparation_and_allocation_leave_no_weak_cache_entry_after_collection() {
        for allocation in [false, true] {
            let mut runtime = Runtime::default();
            let semantic = layout(
                &mut runtime,
                "Unused",
                &[("value", Ty::Builtin(BuiltinType::I32), true)],
            );
            let key = semantic.storage_key();
            let heap = runtime.gc();
            if allocation {
                let storage = heap
                    .prepare_struct_storage(semantic, vec![Value::I32(1)])
                    .unwrap();
                let _borrow = heap.objects.borrow();
                assert!(heap.alloc_object(HeapObject::Struct { storage }).is_err());
            } else {
                assert!(
                    heap.prepare_struct_storage(semantic, vec![Value::Bool(false)])
                        .is_err()
                );
            }
            assert!(heap.struct_layouts.borrow().entries.token(&key).is_some());
            assert!(heap.struct_layouts.borrow().entries.get(&key).is_none());
            runtime.collect_garbage().unwrap();
            assert!(heap.struct_layouts.borrow().entries.token(&key).is_none());
            assert!(!runtime.is_quarantined());
        }
    }

    #[test]
    fn rejected_descriptor_registration_leaves_cache_admission_uncommitted() {
        for replacement in [false, true] {
            let mut runtime = Runtime::default();
            let semantic = layout(
                &mut runtime,
                "Uncommitted",
                &[("value", Ty::Builtin(BuiltinType::I32), true)],
            );
            let key = semantic.storage_key();
            let heap = runtime.gc();
            let original = replacement.then(|| {
                let descriptor = heap.struct_storage_description(semantic.clone()).unwrap();
                let token = heap.struct_layouts.borrow().entries.token(&key).unwrap();
                drop(descriptor);
                token
            });
            {
                let _borrow = heap.objects.borrow();
                assert!(heap.struct_storage_description(semantic).is_err());
            }
            if let Some(original) = original {
                // Removing by the original identity proves failed replacement did
                // not overwrite its cleanup association with an abandoned token.
                for available in [0, WEAK_CACHE_REMOVE_WORK - 1] {
                    let mut budget = ScanBudget::new(available);
                    let (status, removed) = heap
                        .struct_layouts
                        .borrow_mut()
                        .release(&original, &mut budget);
                    assert_eq!(status, ScanStatus::Pending);
                    assert!(removed.is_none());
                    assert_eq!(budget.spent(), 0);
                    assert!(heap.struct_layouts.borrow().entries.token(&key).is_some());
                }
                let mut budget = ScanBudget::new(WEAK_CACHE_REMOVE_WORK);
                let (status, removed) = heap
                    .struct_layouts
                    .borrow_mut()
                    .release(&original, &mut budget);
                assert_eq!(status, ScanStatus::Complete);
                assert_eq!(budget.spent(), WEAK_CACHE_REMOVE_WORK);
                assert!(removed.is_some());
                drop(removed);
            }
            assert!(heap.struct_layouts.borrow().entries.token(&key).is_none());
            runtime.collect_garbage().unwrap();
            assert!(!runtime.is_quarantined());
        }
    }

    #[test]
    fn descriptor_release_preserves_live_tokens_and_newer_cache_replacements() {
        let mut runtime = Runtime::default();
        let semantic = layout(
            &mut runtime,
            "Record",
            &[("value", Ty::Builtin(BuiltinType::I32), true)],
        );
        let key = semantic.storage_key();
        let heap = runtime.gc();
        let first = heap.struct_storage_description(semantic.clone()).unwrap();
        let original = heap.struct_layouts.borrow().entries.token(&key).unwrap();
        runtime.collect_garbage().unwrap();
        assert!(Arc::ptr_eq(
            &heap.struct_layouts.borrow().entries.get(&key).unwrap(),
            &first,
        ));
        drop(first);
        let next = heap.struct_storage_description(semantic).unwrap();
        assert!(!original.is_for(&next));
        runtime.collect_garbage().unwrap();
        assert!(Arc::ptr_eq(
            &heap.struct_layouts.borrow().entries.get(&key).unwrap(),
            &next,
        ));
        let mut budget = ScanBudget::new(WEAK_CACHE_REMOVE_WORK);
        let (status, removed) = heap
            .struct_layouts
            .borrow_mut()
            .release(&original, &mut budget);
        assert_eq!(status, ScanStatus::Complete);
        assert_eq!(budget.spent(), WEAK_CACHE_REMOVE_WORK);
        assert!(removed.is_none());
        assert!(Arc::ptr_eq(
            &heap.struct_layouts.borrow().entries.get(&key).unwrap(),
            &next,
        ));
        drop(next);
        runtime.collect_garbage().unwrap();
        assert!(heap.struct_layouts.borrow().entries.token(&key).is_none());
        assert!(!runtime.is_quarantined());
    }
}
