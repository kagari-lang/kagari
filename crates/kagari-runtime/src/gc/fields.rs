//! Ordinary structs borrow initialized inline bytes under the heap storage guard.
mod codec;

use crate::{
    error::{RuntimeError, RuntimeErrorKind},
    gc::{GcHeap, HeapObjectId, directory::descriptors::Descriptor},
    module::{
        StructLayoutRef,
        layout_storage::{StoredField, StructStorageKey, StructStorageLayout},
    },
    value::{StructValueField, Value, scalar},
    value_check::matches_type_in,
};
use kagari_contract::representation::builtin_representation;
use kagari_types::{scalar::BuiltinType, ty::Ty};
use std::{
    collections::HashMap,
    sync::{Arc, Weak},
};

#[derive(Debug, Default)]
pub(super) struct StructStorageLayouts {
    entries: HashMap<StructStorageKey, Weak<Descriptor>>,
}

impl StructStorageLayouts {
    pub(super) fn prune(&mut self) {
        self.entries.retain(|_, layout| layout.strong_count() != 0);
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
        if let Some(layout) = layouts.entries.get(&key).and_then(Weak::upgrade) {
            return Ok(layout);
        }
        let layout = Descriptor::structure(
            StructStorageLayout::prepare(layout)
                .ok_or_else(|| RuntimeError::module_validation("closed struct storage layout"))?,
        )?;
        layouts
            .entries
            .try_reserve(1)
            .map_err(|_| self.resource_limit("struct description registry"))?;
        layouts.entries.insert(key, Arc::downgrade(&layout));
        Ok(layout)
    }
}

impl GcHeap {
    pub fn struct_layout(&self, id: HeapObjectId) -> Option<StructLayoutRef> {
        self.with_struct(id, |storage| storage.description.semantic.clone())
    }

    pub fn struct_name(&self, id: HeapObjectId) -> Option<String> {
        self.with_struct(id, |storage| {
            let layout = &storage.description.semantic;
            layout
                .module()
                .definition_name(layout.layout().declaration)
                .expect("verified struct definition")
                .to_owned()
        })
    }

    pub fn struct_snapshot(&self, id: HeapObjectId) -> Option<(String, Vec<StructValueField>)> {
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

    pub fn struct_get_slot(
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

    pub fn struct_set_slot(
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
