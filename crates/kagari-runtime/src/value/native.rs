//! One script identity retains its concrete Rust payload and executable edges.
pub(crate) mod data;
use crate::{
    execution_metadata::managed::applications::ApplicationObject,
    frame::types::{arguments::TypeArgument, compatibility::TypeView},
    module::ExecutableModule,
    native::storage::PayloadDescriptor,
    resource::candidate::CandidateOrigin,
    value::{carriers::ManagedRepresentation, kind::ObjectKind},
};
use kagari_gc::{
    access::Scope,
    error::GcError,
    object::{AnyObject, Edge, Gc, GcObject, ObjectCharge},
    scan::{EdgeVisitor, ScanBudget, ScanKind, ScanStatus},
};
use std::sync::Arc;

#[derive(Debug)]
pub(crate) struct NativeObject {
    allocation: NativeAllocation,
    payload: Edge<AnyObject>,
    selections: Option<Edge<ApplicationObject>>,
    dependencies: Vec<Edge<AnyObject>>,
    revision: u64,
    operation_active: bool,
}

/// Immutable physical/nominal facts captured by the producing allocation scope.
#[derive(Debug)]
pub(crate) struct NativeAllocation {
    pub(crate) origin: CandidateOrigin,
    pub(crate) storage: Arc<PayloadDescriptor>,
    pub(crate) owner: ExecutableModule,
    pub(crate) applied: TypeArgument,
}

impl NativeObject {
    pub(crate) fn alloc<'gc>(
        cx: &mut Scope<'gc>,
        allocation: NativeAllocation,
        payload: Gc<'gc, AnyObject>,
        selections: Option<Gc<'gc, ApplicationObject>>,
        dependencies: &[Gc<'gc, AnyObject>],
    ) -> Result<Gc<'gc, Self>, GcError> {
        // SAFETY: all inputs are scoped; every initialized edge remains in this
        // returned provider and is enumerated at a fixed scan coordinate.
        unsafe {
            cx.alloc_with(|mut init| {
                let payload = init.edge_scoped(payload)?;
                let selections = selections
                    .map(|value| init.edge_scoped(value))
                    .transpose()?;
                let mut edges = Vec::new();
                edges.try_reserve_exact(dependencies.len())?;
                for dependency in dependencies {
                    edges.push(init.edge_scoped(*dependency)?);
                }
                Ok(Self {
                    allocation,
                    payload,
                    selections,
                    dependencies: edges,
                    revision: 0,
                    operation_active: false,
                })
            })
        }
    }

    pub(crate) fn origin(&self) -> &CandidateOrigin {
        &self.allocation.origin
    }

    pub(crate) fn payload<'gc>(cx: &Scope<'gc>, owner: Gc<'gc, Self>) -> Gc<'gc, AnyObject> {
        cx.follow(owner, |owner| Some(&owner.payload))
            .expect("initialized native payload edge")
    }

    pub(crate) fn storage(&self) -> &Arc<PayloadDescriptor> {
        &self.allocation.storage
    }

    pub(crate) fn selections<'gc>(
        cx: &Scope<'gc>,
        owner: Gc<'gc, Self>,
    ) -> Option<Gc<'gc, ApplicationObject>> {
        cx.follow(owner, |owner| owner.selections.as_ref())
    }

    pub(crate) fn owner(&self) -> &ExecutableModule {
        &self.allocation.owner
    }

    pub(crate) fn applied(&self) -> &TypeArgument {
        &self.allocation.applied
    }

    pub(crate) fn matches(&self, expected: TypeView<'_>) -> bool {
        self.allocation
            .applied
            .view(&self.allocation.owner)
            .compatible(expected)
    }

    pub(crate) fn revision(&self) -> u64 {
        self.revision
    }

    pub(crate) fn advance_revision<'gc>(
        cx: &mut Scope<'gc>,
        owner: Gc<'gc, Self>,
    ) -> Result<(), GcError> {
        let next = cx
            .read(owner, |owner| owner.revision.checked_add(1))
            .ok_or(GcError::Limit("native revision"))?;
        // SAFETY: only a scalar counter changes, with fixed edges and charge.
        unsafe {
            cx.with_object_mut(owner, |owner, mutation| {
                owner.revision = next;
                mutation.continue_scan();
                Ok(())
            })
        }
    }

    pub(crate) fn replace_payload<'gc>(
        cx: &mut Scope<'gc>,
        owner: Gc<'gc, Self>,
        payload: Gc<'gc, AnyObject>,
    ) -> Result<(), GcError> {
        let next = cx
            .read(owner, |owner| owner.revision.checked_add(1))
            .ok_or(GcError::Limit("native revision"))?;
        // SAFETY: same-brand source, incoming publication before the coupled
        // payload/revision commit; fixed positions/charge permit continuation.
        unsafe {
            cx.with_object_mut(owner, |owner, mutation| {
                let payload = mutation.edge_scoped(payload)?;
                owner.payload = payload;
                owner.revision = next;
                mutation.continue_scan();
                Ok(())
            })
        }
    }

    pub(crate) fn enter_operation<'gc>(
        cx: &mut Scope<'gc>,
        owner: Gc<'gc, Self>,
    ) -> Result<bool, GcError> {
        if cx.read(owner, |owner| owner.operation_active) {
            return Ok(false);
        }
        Self::set_operation(cx, owner, true)?;
        Ok(true)
    }

    pub(crate) fn leave_operation<'gc>(
        cx: &mut Scope<'gc>,
        owner: Gc<'gc, Self>,
    ) -> Result<(), GcError> {
        Self::set_operation(cx, owner, false)
    }

    fn set_operation<'gc>(
        cx: &mut Scope<'gc>,
        owner: Gc<'gc, Self>,
        active: bool,
    ) -> Result<(), GcError> {
        // SAFETY: scalar policy state changes no edge, position or owned capacity.
        unsafe {
            cx.with_object_mut(owner, |owner, mutation| {
                owner.operation_active = active;
                mutation.continue_scan();
                Ok(())
            })
        }
    }
}

impl ManagedRepresentation for NativeObject {
    const KIND: ObjectKind = ObjectKind::Native;
}

// SAFETY: the payload and every executable dependency are ordinary initialized
// edges. Metadata contains immutable facts, not hidden mutable script storage.
// Scalar edits preserve positions; no destructor dereferences either target.
unsafe impl GcObject for NativeObject {
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
        loop {
            if *next == 0 {
                if !budget.take(1) {
                    return Ok(ScanStatus::Pending);
                }
                if let Some(selections) = &self.selections {
                    // SAFETY: this admitted object owns the initialized edge.
                    if unsafe { visitor.try_visit_stored(selections, budget)? }
                        == ScanStatus::Pending
                    {
                        return Ok(ScanStatus::Pending);
                    }
                }
                *next += 1;
            }
            let edge = if *next == 1 {
                &self.payload
            } else if let Some(edge) = self.dependencies.get(*next - 2) {
                edge
            } else {
                return Ok(ScanStatus::Complete);
            };
            if !budget.take(1) {
                return Ok(ScanStatus::Pending);
            }
            // SAFETY: admitted retained provider owns this initialized edge.
            if unsafe { visitor.try_visit_stored(edge, budget)? } == ScanStatus::Pending {
                return Ok(ScanStatus::Pending);
            }
            *next += 1;
        }
    }

    fn charge(&self) -> ObjectCharge {
        ObjectCharge {
            bytes: self.dependencies.capacity() * size_of::<Edge<AnyObject>>(),
            units: self.dependencies.len() + usize::from(self.selections.is_some()),
        }
    }
}

#[cfg(test)]
mod tests;
