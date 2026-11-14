//! Persistent cell storage retains initializer code only until its terminal state.
use crate::{
    module::{
        ExecutableModule,
        managed::{InstallationState, revision::RevisionState},
        state::{StaticFailure, initialization::InitializationAttempt},
    },
    value::{
        carriers::{ScopedValue, StoredValue},
        properties::ValueProperties,
    },
};
use kagari_bytecode::{instruction::ModuleSlot, program::ModuleRef};
use kagari_gc::{
    access::Scope,
    error::GcError,
    object::{Edge, Gc, GcObject, ObjectCharge},
    scan::{EdgeVisitor, ScanBudget, ScanKind, ScanStatus},
};
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StaticPhase {
    Uninitialized,
    Initializing,
    Ready,
    Failed,
}

/// Runtime's initializer frame keeps admission/attempt authority outside this
/// object. Its semantic owner validates type, access and attempt before commits.
#[derive(Debug)]
pub(crate) struct StaticCellObject {
    installation: Edge<InstallationState>,
    owner: ExecutableModule,
    slot: ModuleSlot,
    phase: StaticPhase,
    recipe: Option<Edge<RevisionState>>,
    value: StoredValue,
    failure: Option<StaticFailure>,
    attempt: Option<Arc<InitializationAttempt>>,
}

impl StaticCellObject {
    pub(crate) fn alloc<'gc>(
        cx: &mut Scope<'gc>,
        revision: Gc<'gc, RevisionState>,
        module: ModuleRef,
        slot: ModuleSlot,
    ) -> Result<Gc<'gc, Self>, GcError> {
        let installation = RevisionState::installation(cx, revision);
        let owner = cx
            .read(revision, |revision| revision.code().member(module))
            .ok_or(GcError::Index)?;
        let descriptor = owner
            .bytecode
            .module_slots
            .get(slot.index())
            .ok_or(GcError::Index)?;
        if descriptor.contract.supplier != owner.bytecode.identity {
            return Err(GcError::Representation);
        }
        // SAFETY: the protected introducing revision is imported by Init; value
        // is initialized Unit and all subsequent edits use checked operations.
        unsafe {
            cx.alloc_with(|mut init| {
                Ok(Self {
                    installation: init.edge_scoped(installation)?,
                    owner,
                    slot,
                    phase: StaticPhase::Uninitialized,
                    recipe: Some(init.edge_scoped(revision)?),
                    value: StoredValue::unit(),
                    failure: None,
                    attempt: None,
                })
            })
        }
    }

    pub(crate) fn installation<'gc>(
        cx: &Scope<'gc>,
        cell: Gc<'gc, Self>,
    ) -> Gc<'gc, InstallationState> {
        cx.follow(cell, |cell| Some(&cell.installation))
            .expect("static cell installation edge")
    }

    pub(crate) fn owner(&self) -> &ExecutableModule {
        &self.owner
    }

    pub(crate) fn slot(&self) -> ModuleSlot {
        self.slot
    }

    pub(crate) fn phase(&self) -> StaticPhase {
        self.phase
    }

    pub(crate) fn failure(&self) -> Option<&StaticFailure> {
        self.failure.as_ref()
    }

    pub(crate) fn recipe<'gc>(
        cx: &Scope<'gc>,
        cell: Gc<'gc, Self>,
    ) -> Option<Gc<'gc, RevisionState>> {
        cx.follow(cell, |cell| cell.recipe.as_ref())
    }

    pub(crate) fn attempt(&self) -> Option<&Arc<InitializationAttempt>> {
        self.attempt.as_ref()
    }

    pub(crate) fn begin<'gc>(
        cx: &mut Scope<'gc>,
        cell: Gc<'gc, Self>,
        attempt: Arc<InitializationAttempt>,
    ) -> Result<(), GcError> {
        if cx.read(cell, |cell| {
            cell.phase != StaticPhase::Uninitialized || cell.attempt.is_some()
        }) {
            return Err(GcError::Storage("static cell is not uninitialized"));
        }
        // SAFETY: only phase and outside-heap control metadata change. Physical
        // edges, positions and owned charge are unchanged; the scanner ignores phase.
        unsafe {
            cx.with_object_mut(cell, |cell, mutation| {
                cell.phase = StaticPhase::Initializing;
                cell.attempt = Some(attempt);
                mutation.continue_scan();
                Ok(())
            })
        }
    }

    pub(crate) fn read<'gc>(
        cx: &Scope<'gc>,
        cell: Gc<'gc, Self>,
    ) -> Result<ScopedValue<'gc>, GcError> {
        StoredValue::get_field(cx, cell, |cell| {
            (cell.phase == StaticPhase::Ready).then_some(&cell.value)
        })
    }

    pub(crate) fn complete<'gc>(
        cx: &mut Scope<'gc>,
        cell: Gc<'gc, Self>,
        value: ScopedValue<'gc>,
        attempt: &Arc<InitializationAttempt>,
    ) -> Result<(), GcError> {
        if !cx.read(cell, |cell| {
            cell.attempt
                .as_ref()
                .is_some_and(|actual| Arc::ptr_eq(actual, attempt))
        }) {
            return Err(GcError::Storage("static completion lost attempt authority"));
        }
        Self::write(cx, cell, value, StaticPhase::Initializing)
    }

    pub(crate) fn replace<'gc>(
        cx: &mut Scope<'gc>,
        cell: Gc<'gc, Self>,
        value: ScopedValue<'gc>,
    ) -> Result<(), GcError> {
        Self::write(cx, cell, value, StaticPhase::Ready)
    }

    fn write<'gc>(
        cx: &mut Scope<'gc>,
        cell: Gc<'gc, Self>,
        value: ScopedValue<'gc>,
        required: StaticPhase,
    ) -> Result<(), GcError> {
        if cx.read(cell, Self::phase) != required {
            return Err(GcError::Storage("static write has the wrong phase"));
        }
        if !ValueProperties::of(cx, value)?.storable {
            return Err(GcError::Representation);
        }
        // SAFETY: same-scope input, publish before the coupled no-fail replacement,
        // and default scan restart. Type/mutability/attempt checks precede this call.
        unsafe {
            cx.with_object_mut(cell, |cell, mutation| {
                let value = StoredValue::publish_field(mutation, value)?;
                cell.value = value;
                cell.recipe = None;
                cell.phase = StaticPhase::Ready;
                cell.attempt = None;
                Ok(())
            })
        }
    }

    /// Failure cannot overwrite a committed value. Runtime must resolve detached
    /// attempt failure before admitting another transition or collection.
    pub(crate) fn fail<'gc>(
        cx: &mut Scope<'gc>,
        cell: Gc<'gc, Self>,
        failure: StaticFailure,
    ) -> Result<(), GcError> {
        match cx.read(cell, Self::phase) {
            StaticPhase::Ready => return Err(GcError::Storage("cannot fail a ready static cell")),
            StaticPhase::Failed => return Ok(()),
            StaticPhase::Uninitialized | StaticPhase::Initializing => {}
        }
        // SAFETY: reserve the final owned diagnostic charge before editing. No
        // new edge is introduced, and normal scan restart covers removed edges.
        unsafe {
            cx.with_object_mut(cell, |cell, mutation| {
                mutation.prepare_charge(ObjectCharge {
                    bytes: failure.retained_bytes(),
                    units: 0,
                })?;
                cell.recipe = None;
                cell.value = StoredValue::unit();
                cell.phase = StaticPhase::Failed;
                cell.failure = Some(failure);
                cell.attempt = None;
                Ok(())
            })
        }
    }
}

// SAFETY: installation, recipe and value are initialized and always scanned. The
// cursor is a finite field coordinate; mutation never stores peer payload borrows.
// Owned failure text has constant-time capacity accounting and an inert Drop.
unsafe impl GcObject for StaticCellObject {
    type ScanState = u8;

    const SCAN_KIND: ScanKind = ScanKind::Resumable;

    fn begin_scan(&self) -> u8 {
        0
    }

    fn scan(
        &self,
        next: &mut u8,
        budget: &mut ScanBudget,
        visitor: &mut EdgeVisitor<'_>,
    ) -> Result<ScanStatus, GcError> {
        while *next < 3 {
            if !budget.take(1) {
                return Ok(ScanStatus::Pending);
            }
            let status = match *next {
                // SAFETY: the admitted cell retains its installation edge.
                0 => unsafe { visitor.try_visit_stored(&self.installation, budget)? },
                1 => match &self.recipe {
                    // SAFETY: the admitted cell retains its initialized recipe.
                    Some(edge) => unsafe { visitor.try_visit_stored(edge, budget)? },
                    None => ScanStatus::Complete,
                },
                // SAFETY: the admitted cell retains its initialized tagged value.
                _ => unsafe { self.value.scan(budget, visitor)? },
            };
            if status == ScanStatus::Pending {
                return Ok(status);
            }
            *next += 1;
        }
        Ok(ScanStatus::Complete)
    }

    fn charge(&self) -> ObjectCharge {
        ObjectCharge {
            bytes: self
                .failure
                .as_ref()
                .map_or(0, StaticFailure::retained_bytes),
            units: 0,
        }
    }
}
