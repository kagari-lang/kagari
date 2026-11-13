//! Packed scalar bytes and typed managed fields implement ordinary shared structs.
use crate::{
    module::{
        StructLayoutRef,
        layout_storage::{StoredField, StructStorageLayout},
    },
    resource::candidate::CandidateOrigin,
    value::{
        carriers::{ManagedRepresentation, ScopedValue, StoredValue},
        kind::ObjectKind,
        properties::ValueProperties,
        scalar::{self, domain, fields},
    },
};
use kagari_contract::representation::{builtin_representation, semantic_representation};
use kagari_gc::{
    access::Scope,
    error::GcError,
    object::{Gc, GcObject, ObjectCharge},
    scan::{EdgeVisitor, ScanBudget, ScanKind, ScanStatus},
};
use kagari_types::{scalar::BuiltinType, ty::Ty};
use std::sync::Arc;

/// The shared description retains immutable type backing only. A struct's
/// managed fields are real edges; neither nominal facts nor the weak description
/// cache keeps mutable module instances or executable generations alive.
#[derive(Debug)]
pub(crate) struct StructObject {
    origin: CandidateOrigin,
    description: Arc<StructStorageLayout>,
    scalars: Vec<u8>,
    managed: Vec<StoredValue>,
}

impl StructObject {
    /// The semantic owner checks nominal field types before physical admission.
    pub(crate) fn alloc<'gc>(
        cx: &mut Scope<'gc>,
        description: Arc<StructStorageLayout>,
        values: &[ScopedValue<'gc>],
        origin: CandidateOrigin,
    ) -> Result<Gc<'gc, Self>, GcError> {
        if values.len() != description.fields.len() {
            return Err(GcError::Representation);
        }
        for (field, value) in description.fields.iter().zip(values) {
            if !ValueProperties::of(cx, *value)?.storable {
                return Err(GcError::Representation);
            }
            match *field {
                StoredField::Scalar { encoding, .. } => {
                    let (representation, bits) =
                        value.scalar_payload().ok_or(GcError::Representation)?;
                    if representation != encoding.representation
                        || !domain::admits_payload(representation, Some(encoding.ty), bits)
                    {
                        return Err(GcError::Representation);
                    }
                }
                StoredField::Managed { .. } if value.object().is_none() => {
                    return Err(GcError::Representation);
                }
                StoredField::Managed { .. } => {}
            }
        }
        // SAFETY: prechecked protected fields, complete scalar initialization,
        // and typed managed cells imported only through this initializer. Every
        // managed cell is scanned below; partial failure drops inert backing.
        unsafe {
            cx.alloc_with(|mut init| {
                let mut scalars = Vec::new();
                scalars.try_reserve_exact(description.scalar_bytes)?;
                scalars.resize(description.scalar_bytes, 0);
                let mut managed = Vec::new();
                managed.try_reserve_exact(description.managed_count)?;
                for (field, value) in description.fields.iter().zip(values) {
                    match *field {
                        StoredField::Scalar { offset, encoding } => {
                            fields::write_scalar(
                                &mut scalars,
                                offset,
                                encoding,
                                scalar::encode(value).ok_or(GcError::Representation)?,
                            )
                            .ok_or(GcError::Representation)?;
                        }
                        StoredField::Managed { index } => {
                            if index != managed.len() {
                                return Err(GcError::Representation);
                            }
                            managed.push(StoredValue::initialize(&mut init, *value)?);
                        }
                    }
                }
                Ok(Self {
                    origin,
                    description,
                    scalars,
                    managed,
                })
            })
        }
    }

    pub(crate) fn origin(&self) -> &CandidateOrigin {
        &self.origin
    }

    pub(crate) fn layout(&self) -> &StructLayoutRef {
        &self.description.semantic
    }

    pub(crate) fn len(&self) -> usize {
        self.description.fields.len()
    }

    pub(crate) fn get<'gc>(
        cx: &Scope<'gc>,
        owner: Gc<'gc, Self>,
        slot: usize,
    ) -> Result<ScopedValue<'gc>, GcError> {
        let field = cx
            .read(owner, |owner| owner.description.fields.get(slot).copied())
            .ok_or(GcError::Index)?;
        match field {
            StoredField::Scalar { offset, encoding } => {
                let bits = cx
                    .read(owner, |owner| {
                        fields::read_scalar(&owner.scalars, offset, encoding)
                    })
                    .ok_or(GcError::Representation)?;
                scalar::decode(encoding.representation, bits).ok_or(GcError::Representation)
            }
            StoredField::Managed { index } => {
                StoredValue::get_field(cx, owner, |owner| owner.managed.get(index))
            }
        }
    }

    /// Preserve input-before-target checking. The semantic operation checks the
    /// incoming nominal/access contract before entering this physical writer.
    pub(crate) fn set<'gc>(
        cx: &mut Scope<'gc>,
        owner: Gc<'gc, Self>,
        expected: &StructLayoutRef,
        slot: usize,
        value: ScopedValue<'gc>,
    ) -> Result<(), GcError> {
        if !ValueProperties::of(cx, value)?.storable {
            return Err(GcError::Representation);
        }
        let contract = expected.layout().fields.get(slot).ok_or(GcError::Index)?;
        if !contract.mutable || !value.has_representation(semantic_representation(&contract.ty)) {
            return Err(GcError::Representation);
        }
        if let Ty::Builtin(ty) = contract.ty
            && ty != BuiltinType::String
        {
            return Self::set_scalar(
                cx,
                owner,
                expected,
                slot,
                ty,
                scalar::encode(&value).ok_or(GcError::Representation)?,
            );
        }
        let StoredField::Managed { index } = Self::field(cx, owner, expected, slot)? else {
            return Err(GcError::Representation);
        };
        if value.object().is_none() {
            return Err(GcError::Representation);
        }
        // SAFETY: fixed initialized coordinates and unchanged capacity/charge.
        // Publish before the no-fail replacement. A previously scanned field's
        // new target is shaded, so its existing continuation remains valid.
        unsafe {
            cx.with_object_mut(owner, |owner, mutation| {
                let value = StoredValue::publish_field(mutation, value)?;
                owner.managed[index] = value;
                mutation.continue_scan();
                Ok(())
            })
        }
    }

    pub(crate) fn scalar<'gc>(
        cx: &Scope<'gc>,
        owner: Gc<'gc, Self>,
        expected: &StructLayoutRef,
        slot: usize,
        ty: BuiltinType,
    ) -> Result<u64, GcError> {
        let StoredField::Scalar { offset, encoding } = Self::field(cx, owner, expected, slot)?
        else {
            return Err(GcError::Representation);
        };
        if encoding.ty != ty {
            return Err(GcError::Representation);
        }
        cx.read(owner, |owner| {
            fields::read_scalar(&owner.scalars, offset, encoding)
        })
        .ok_or(GcError::Representation)
    }

    pub(crate) fn set_scalar<'gc>(
        cx: &mut Scope<'gc>,
        owner: Gc<'gc, Self>,
        expected: &StructLayoutRef,
        slot: usize,
        ty: BuiltinType,
        bits: u64,
    ) -> Result<(), GcError> {
        if !domain::admits_payload(builtin_representation(ty), Some(ty), bits) {
            return Err(GcError::Representation);
        }
        let contract = expected.layout().fields.get(slot).ok_or(GcError::Index)?;
        if !contract.mutable || contract.ty != Ty::Builtin(ty) {
            return Err(GcError::Representation);
        }
        let StoredField::Scalar { offset, encoding } = Self::field(cx, owner, expected, slot)?
        else {
            return Err(GcError::Representation);
        };
        if encoding.ty != ty {
            return Err(GcError::Representation);
        }
        // SAFETY: only initialized scalar bytes change. No edge, length, capacity
        // or charge changes, and the fixed scanner continuation remains valid.
        unsafe {
            cx.with_object_mut(owner, |owner, mutation| {
                fields::write_scalar(&mut owner.scalars, offset, encoding, bits)
                    .ok_or(GcError::Representation)?;
                mutation.continue_scan();
                Ok(())
            })
        }
    }

    fn field<'gc>(
        cx: &Scope<'gc>,
        owner: Gc<'gc, Self>,
        expected: &StructLayoutRef,
        slot: usize,
    ) -> Result<StoredField, GcError> {
        cx.read(owner, |owner| {
            if !owner.layout().matches(expected) {
                return Err(GcError::Representation);
            }
            owner
                .description
                .fields
                .get(slot)
                .copied()
                .ok_or(GcError::Index)
        })
    }
}

impl ManagedRepresentation for StructObject {
    const KIND: ObjectKind = ObjectKind::Struct;
}

// SAFETY: only the fully initialized managed buffer carries edges; scalar bytes
// and immutable descriptions contain none. Scan positions are fixed numeric
// indexes; writes publish incoming targets and preserve valid continuation.
// No provider mutation resizes either buffer, and Drop reads no managed peer.
unsafe impl GcObject for StructObject {
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
        while let Some(field) = self.managed.get(*next) {
            if !budget.take(1) {
                return Ok(ScanStatus::Pending);
            }
            // SAFETY: retained initialized cell in this admitted object.
            if unsafe { field.scan(budget, visitor)? } == ScanStatus::Pending {
                return Ok(ScanStatus::Pending);
            }
            *next += 1;
        }
        Ok(ScanStatus::Complete)
    }

    fn charge(&self) -> ObjectCharge {
        ObjectCharge {
            bytes: self.scalars.capacity() + self.managed.capacity() * size_of::<StoredValue>(),
            units: self.description.fields.len(),
        }
    }
}
