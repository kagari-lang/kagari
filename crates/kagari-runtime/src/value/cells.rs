//! Shared capture cells keep a fixed physical contract and a coupled tagged field.
use crate::{
    resource::candidate::CandidateOrigin,
    value::{
        carriers::{ManagedRepresentation, ScopedValue, StoredValue},
        kind::ObjectKind,
        properties::ValueProperties,
    },
};
use kagari_abi::representation::ValueType;
use kagari_gc::{
    access::Scope,
    error::GcError,
    object::{Gc, GcObject, ObjectCharge},
    scan::{EdgeVisitor, ScanBudget, ScanKind, ScanStatus},
};

#[derive(Debug)]
pub(crate) struct CellObject {
    origin: CandidateOrigin,
    representation: ValueType,
    value: StoredValue,
}

impl CellObject {
    pub(crate) fn alloc<'gc>(
        cx: &mut Scope<'gc>,
        representation: ValueType,
        value: ScopedValue<'gc>,
        origin: CandidateOrigin,
    ) -> Result<Gc<'gc, Self>, GcError> {
        Self::check_value(cx, representation, value)?;
        // SAFETY: one initialized field imported through Init. Its immutable
        // contract is checked before all construction and replacement operations.
        unsafe {
            cx.alloc_with(|mut init| {
                Ok(Self {
                    origin,
                    representation,
                    value: StoredValue::initialize(&mut init, value)?,
                })
            })
        }
    }

    pub(crate) fn origin(&self) -> &CandidateOrigin {
        &self.origin
    }

    pub(crate) fn get<'gc>(
        cx: &Scope<'gc>,
        cell: Gc<'gc, Self>,
        representation: ValueType,
    ) -> Result<ScopedValue<'gc>, GcError> {
        if cx.read(cell, |cell| cell.representation) != representation {
            return Err(GcError::Representation);
        }
        StoredValue::get_field(cx, cell, |cell| Some(&cell.value))
    }

    pub(crate) fn set<'gc>(
        cx: &mut Scope<'gc>,
        cell: Gc<'gc, Self>,
        representation: ValueType,
        value: ScopedValue<'gc>,
    ) -> Result<(), GcError> {
        // Preserve input-before-target validation; the semantic owner maps these
        // checks to its distinct invalid-value and invalid-capture diagnostics.
        Self::check_value(cx, representation, value)?;
        if cx.read(cell, |cell| cell.representation) != representation {
            return Err(GcError::Representation);
        }
        // SAFETY: same-scope protection, publication before the coupled commit,
        // fixed scan position and unchanged charge/capacity. The new target is
        // shaded even if this field was already visited, so continuation is valid.
        unsafe {
            cx.with_object_mut(cell, |cell, mutation| {
                let value = StoredValue::publish_field(mutation, value)?;
                cell.value = value;
                mutation.continue_scan();
                Ok(())
            })
        }
    }

    fn check_value<'gc>(
        cx: &Scope<'gc>,
        representation: ValueType,
        value: ScopedValue<'gc>,
    ) -> Result<(), GcError> {
        if !value.has_representation(representation) || !ValueProperties::of(cx, value)?.storable {
            return Err(GcError::Representation);
        }
        Ok(())
    }
}

impl ManagedRepresentation for CellObject {
    const KIND: ObjectKind = ObjectKind::Cell;
}

// SAFETY: the only initialized field is always scanned at one fixed coordinate.
// Edits preserve the contract, publish first and repair/retain valid continuation.
// There is no indirect backing or peer-aware destructor.
unsafe impl GcObject for CellObject {
    type ScanState = bool;

    const SCAN_KIND: ScanKind = ScanKind::Resumable;

    fn charge(&self) -> ObjectCharge {
        ObjectCharge { units: 1, bytes: 0 }
    }

    fn begin_scan(&self) -> bool {
        false
    }

    fn scan(
        &self,
        visited: &mut bool,
        budget: &mut ScanBudget,
        visitor: &mut EdgeVisitor<'_>,
    ) -> Result<ScanStatus, GcError> {
        if !*visited {
            if !budget.take(1) {
                return Ok(ScanStatus::Pending);
            }
            // SAFETY: this admitted cell retains its initialized value field.
            if unsafe { self.value.scan(budget, visitor)? } == ScanStatus::Pending {
                return Ok(ScanStatus::Pending);
            }
            *visited = true;
        }
        Ok(ScanStatus::Complete)
    }
}
