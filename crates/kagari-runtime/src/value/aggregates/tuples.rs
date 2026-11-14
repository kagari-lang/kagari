//! Immutable tuple membership owns ordinary collector edges and stable properties.
use crate::value::{
    carriers::{ManagedRepresentation, ScopedValue, StoredValue},
    kind::ObjectKind,
    properties::ValueProperties,
};
use kagari_gc::{
    access::Scope,
    error::GcError,
    object::{Gc, GcObject, ObjectCharge},
    scan::{EdgeVisitor, ScanBudget, ScanKind, ScanStatus},
};

/// Immutable tuple fields. Construction admits the complete protected input;
/// later access cannot mutate fields, capacity or scanner coordinates.
#[derive(Debug)]
pub(crate) struct TupleObject {
    fields: Vec<StoredValue>,
    properties: ValueProperties,
}

impl TupleObject {
    pub(crate) fn alloc<'gc>(
        cx: &mut Scope<'gc>,
        values: &[ScopedValue<'gc>],
    ) -> Result<Gc<'gc, Self>, GcError> {
        let properties = ValueProperties::tuple(cx, values)?;
        // SAFETY: every field comes from a protected same-scope input, imported
        // through this initializer. The scanner covers every initialized field;
        // partial construction drops only inert edges and owned Vec backing.
        unsafe {
            cx.alloc_with(|mut init| {
                let mut fields = Vec::new();
                fields.try_reserve_exact(values.len())?;
                for value in values {
                    fields.push(StoredValue::initialize(&mut init, *value)?);
                }
                Ok(Self { fields, properties })
            })
        }
    }

    pub(crate) fn len(&self) -> usize {
        self.fields.len()
    }

    pub(crate) fn properties(&self) -> ValueProperties {
        self.properties
    }

    pub(crate) fn get<'gc>(
        cx: &Scope<'gc>,
        owner: Gc<'gc, Self>,
        index: usize,
    ) -> Result<ScopedValue<'gc>, GcError> {
        StoredValue::get_field(cx, owner, |tuple| tuple.fields.get(index))
    }
}

impl ManagedRepresentation for TupleObject {
    const KIND: ObjectKind = ObjectKind::Tuple;
}

// SAFETY: fields are initialized through Init and then immutable. ScanState is a
// finite numeric coordinate; each inspected cell is charged, including scalars.
// A pending target visit does not advance its coordinate. Drop releases Vec/Edge
// backing without accessing managed peers, and charge reads owned capacity only.
unsafe impl GcObject for TupleObject {
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
        while let Some(value) = self.fields.get(*next) {
            if !budget.take(1) {
                return Ok(ScanStatus::Pending);
            }
            // SAFETY: this admitted object owns the initialized field throughout
            // scanning. The carrier is borrowed, never extracted or cloned.
            if unsafe { value.scan(budget, visitor)? } == ScanStatus::Pending {
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
