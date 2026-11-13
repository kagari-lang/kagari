//! Immutable enum payloads retain type facts, field edges and detached origins.
use crate::{
    error_trace::ErrorTrace,
    module::EnumVariantRef,
    value::{
        carriers::{ManagedRepresentation, ScopedValue, StoredValue},
        kind::ObjectKind,
        properties::ValueProperties,
    },
};
use kagari_contract::representation::semantic_representation;
use kagari_gc::{
    access::Scope,
    error::GcError,
    object::{Gc, GcObject, ObjectCharge},
    scan::{EdgeVisitor, ScanBudget, ScanKind, ScanStatus},
};
use std::sync::Arc;

/// Plain nominal data never supplies executable retention. Layout/type backing
/// and error origins contain no mutable script state; only fields are traced.
/// The semantic owner checks nominal payload types before this physical admission.
#[derive(Debug)]
pub(crate) struct EnumObject {
    layout: EnumVariantRef,
    fields: Vec<StoredValue>,
    origin: Option<Arc<ErrorTrace>>,
}

impl EnumObject {
    pub(crate) fn alloc<'gc>(
        cx: &mut Scope<'gc>,
        layout: EnumVariantRef,
        values: &[ScopedValue<'gc>],
        origin: Option<Arc<ErrorTrace>>,
    ) -> Result<Gc<'gc, Self>, GcError> {
        let types = &layout.variant().payload;
        if values.len() != types.len() {
            return Err(GcError::Representation);
        }
        for (value, ty) in values.iter().zip(types) {
            if !value.has_representation(semantic_representation(ty))
                || !ValueProperties::of(cx, *value)?.storable
            {
                return Err(GcError::Representation);
            }
        }
        // SAFETY: complete protected input, imported through Init into the sole
        // immutable field buffer. Partial failure drops only inert stored edges.
        unsafe {
            cx.alloc_with(|mut init| {
                let mut fields = Vec::new();
                fields.try_reserve_exact(values.len())?;
                for value in values {
                    fields.push(StoredValue::initialize(&mut init, *value)?);
                }
                Ok(Self {
                    layout,
                    fields,
                    origin,
                })
            })
        }
    }

    pub(crate) fn layout(&self) -> &EnumVariantRef {
        &self.layout
    }

    pub(crate) fn len(&self) -> usize {
        self.fields.len()
    }

    pub(crate) fn origin(&self) -> Option<&Arc<ErrorTrace>> {
        self.origin.as_ref()
    }

    pub(crate) fn get<'gc>(
        cx: &Scope<'gc>,
        owner: Gc<'gc, Self>,
        index: usize,
    ) -> Result<ScopedValue<'gc>, GcError> {
        StoredValue::get_field(cx, owner, |value| value.fields.get(index))
    }
}

impl ManagedRepresentation for EnumObject {
    const KIND: ObjectKind = ObjectKind::Enum;
}

// SAFETY: all initialized values belong to the immutable field buffer and are
// scanned at finite numeric positions. A pending visit retries the same field.
// Shared immutable metadata/origin backing is excluded from owned Vec charge.
// Drop releases inert edges and Rust owners without dereferencing managed peers.
unsafe impl GcObject for EnumObject {
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
        while let Some(field) = self.fields.get(*next) {
            if !budget.take(1) {
                return Ok(ScanStatus::Pending);
            }
            // SAFETY: the admitted object owns this initialized field throughout
            // tracing; no carrier is extracted into untraced temporary storage.
            if unsafe { field.scan(budget, visitor)? } == ScanStatus::Pending {
                return Ok(ScanStatus::Pending);
            }
            *next += 1;
        }
        Ok(ScanStatus::Complete)
    }

    fn charge(&self) -> ObjectCharge {
        ObjectCharge {
            bytes: self.fields.capacity() * size_of::<StoredValue>(),
            units: self.fields.len(),
        }
    }
}
