//! Fixed arrays own compact primitive data or ordinary managed field edges.
pub(crate) mod storage;
use crate::{
    native::storage_type::StorageType,
    resource::candidate::CandidateOrigin,
    value::{
        arrays::storage::{ArrayValues, PackedStorage},
        carriers::{ManagedRepresentation, ScopedValue},
        kind::ObjectKind,
        properties::ValueProperties,
    },
};
use kagari_gc::{
    access::Scope,
    error::GcError,
    object::{Gc, GcObject, ObjectCharge},
    scan::{EdgeVisitor, ScanBudget, ScanKind, ScanStatus},
};
use std::{ops::Range, sync::Arc};

/// Element contracts are immutable type facts, not executable-state roots. The
/// semantic operation checks nominal values; this provider enforces storability,
/// fixed length/kind and complete publication/scanning of actual managed fields.
#[derive(Debug)]
pub(crate) struct ArrayObject {
    origin: CandidateOrigin,
    contract: Arc<StorageType>,
    values: PackedStorage,
}

impl ArrayObject {
    pub(crate) fn alloc<'gc>(
        cx: &mut Scope<'gc>,
        contract: Arc<StorageType>,
        values: ArrayValues<'gc>,
        origin: CandidateOrigin,
    ) -> Result<Gc<'gc, Self>, GcError> {
        values
            .len()
            .checked_add(1)
            .ok_or(GcError::Limit("array length"))?;
        if !values.has_type(&contract.ty) {
            return Err(GcError::Representation);
        }
        // SAFETY: protected scoped input; all initialized cells move into this
        // returned provider and are scanned. Partial construction drops inert edges.
        unsafe {
            cx.alloc_with(|mut init| {
                Ok(Self {
                    origin,
                    contract,
                    values: values.initialize(&mut init)?,
                })
            })
        }
    }

    pub(crate) fn origin(&self) -> &CandidateOrigin {
        &self.origin
    }

    pub(crate) fn contract(&self) -> &Arc<StorageType> {
        &self.contract
    }

    pub(crate) fn len(&self) -> usize {
        self.values.len()
    }

    pub(crate) fn get<'gc>(
        cx: &Scope<'gc>,
        owner: Gc<'gc, Self>,
        index: usize,
    ) -> Result<ScopedValue<'gc>, GcError> {
        PackedStorage::get(cx, owner, |owner| &owner.values, index)
    }

    pub(crate) fn set<'gc>(
        cx: &mut Scope<'gc>,
        owner: Gc<'gc, Self>,
        index: usize,
        value: ScopedValue<'gc>,
    ) -> Result<(), GcError> {
        if !ValueProperties::of(cx, value)?.storable {
            return Err(GcError::Representation);
        }
        let value = cx.read(owner, |owner| owner.values.prepare(value))?;
        if index >= cx.read(owner, Self::len) {
            return Err(GcError::Index);
        }
        // SAFETY: fixed admitted coordinates, protected input and publication
        // before the no-fail cell write. Existing scan positions remain valid.
        unsafe {
            cx.with_object_mut(owner, |owner, mutation| {
                owner.values.write(mutation, index, value)?;
                mutation.continue_scan();
                Ok(())
            })
        }
    }

    /// Append an admitted chunk to one scoped preparation buffer. Runtime polls
    /// cancellation between chunks, outside every payload loan.
    pub(crate) fn copy_into<'gc>(
        cx: &Scope<'gc>,
        owner: Gc<'gc, Self>,
        range: Range<usize>,
        output: &mut ArrayValues<'gc>,
    ) -> Result<(), GcError> {
        // SAFETY: this provider admits storable initialized fields, remains
        // retained, and cannot mutate during the closed shared source read.
        unsafe { PackedStorage::copy_into(cx, owner, |owner| &owner.values, range, output) }
    }

    /// Swap a completely prepared fixed buffer. All fallible publication/charge
    /// admission precedes replacement; cancellation policy belongs to its caller.
    pub(crate) fn replace<'gc>(
        cx: &mut Scope<'gc>,
        owner: Gc<'gc, Self>,
        values: ArrayValues<'gc>,
    ) -> Result<(), GcError> {
        if !cx.read(owner, |owner| {
            values.len() == owner.len() && values.matches(&owner.values)
        }) {
            return Err(GcError::Representation);
        }
        // SAFETY: no field changes until every incoming carrier is published and
        // the actual staged capacity is admitted. Fixed length/kind keep numeric
        // scanner positions valid; all overwritten targets have been shaded.
        unsafe {
            cx.with_object_mut(owner, |owner, mutation| {
                let values = values.publish(mutation)?;
                mutation.prepare_charge(ObjectCharge {
                    bytes: values.retained_bytes(),
                    units: values.len(),
                })?;
                owner.values = values;
                mutation.continue_scan();
                Ok(())
            })
        }
    }

    /// Commit a prepared memmove-style range without changing target capacity.
    pub(crate) fn overwrite<'gc>(
        cx: &mut Scope<'gc>,
        owner: Gc<'gc, Self>,
        start: usize,
        values: ArrayValues<'gc>,
    ) -> Result<(), GcError> {
        cx.read(owner, |owner| {
            if start > owner.len() || values.len() > owner.len() - start {
                return Err(GcError::Index);
            }
            if !values.matches(&owner.values) {
                return Err(GcError::Representation);
            }
            Ok(())
        })?;
        // SAFETY: admitted whole range/kind, publication before any field changes,
        // unchanged target capacity/charge and stable scanner coordinates.
        unsafe {
            cx.with_object_mut(owner, |owner, mutation| {
                let values = values.publish(mutation)?;
                owner.values.overwrite(start, values);
                mutation.continue_scan();
                Ok(())
            })
        }
    }
}

impl ManagedRepresentation for ArrayObject {
    const KIND: ObjectKind = ObjectKind::Array;
}

// SAFETY: values owns every initialized edge and delegates its resumable scan.
// Mutation preserves fixed positions and publishes before commits. Charge reads
// owned capacity only; immutable contract backing is shared and Drop reads no peer.
unsafe impl GcObject for ArrayObject {
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
        // SAFETY: this admitted object owns the complete initialized buffer.
        unsafe { self.values.scan(next, budget, visitor) }
    }

    fn charge(&self) -> ObjectCharge {
        ObjectCharge {
            bytes: self.values.retained_bytes(),
            units: self.values.len(),
        }
    }
}
