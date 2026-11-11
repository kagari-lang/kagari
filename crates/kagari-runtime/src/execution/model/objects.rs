//! Fixed fields model object -> closure -> environment/revision -> static edges.
use crate::program::Program;
use kagari_gc::{
    access::{EdgeField, Scope},
    error::GcError,
    object::{Edge, Gc, GcObject},
    scan::{EdgeVisitor, ScanBudget, ScanKind, ScanStatus},
};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Kind {
    Payload,
    Closure,
    Revision,
    Static,
}

pub(super) struct Object {
    pub kind: Kind,
    pub value: i64,
    pub code: Option<Program>,
    next: Option<Edge<Self>>,
    drops: Arc<AtomicUsize>,
}

impl Object {
    pub fn counter(&self) -> Arc<AtomicUsize> {
        self.drops.clone()
    }

    // SAFETY: the scanner covers this fixed edge; replacement changes neither
    // coordinates, capacity nor any coupled tag/initialization invariant.
    const NEXT: EdgeField<Self, Self> =
        unsafe { EdgeField::new(|node| &node.next, |node| &mut node.next) };

    pub fn alloc<'gc>(
        cx: &mut Scope<'gc>,
        kind: Kind,
        value: i64,
        code: Option<Program>,
        drops: &Arc<AtomicUsize>,
    ) -> Result<Gc<'gc, Self>, GcError> {
        // SAFETY: initially edge-free. Subsequent reference writes use NEXT;
        // immutable Program contains no script heap references.
        unsafe {
            cx.alloc_with(|_| {
                Ok(Self {
                    kind,
                    value,
                    code,
                    next: None,
                    drops: drops.clone(),
                })
            })
        }
    }

    pub fn set_next<'gc>(
        cx: &mut Scope<'gc>,
        owner: Gc<'gc, Self>,
        expected: Kind,
        next: Option<Gc<'gc, Self>>,
    ) -> Result<(), GcError> {
        if cx.read(owner, |owner| owner.kind) != expected {
            return Err(GcError::Representation);
        }
        Self::NEXT.set(cx, owner, next);
        Ok(())
    }

    pub fn next<'gc>(cx: &Scope<'gc>, owner: Gc<'gc, Self>) -> Option<Gc<'gc, Self>> {
        Self::NEXT.get(cx, owner)
    }
}

// SAFETY: only NEXT holds an edge, always traced at the same coordinate. Kind,
// scalar and immutable code have no managed edges. Scanning borrows no peer;
// Drop only releases edge-free code backing and updates a host counter.
unsafe impl GcObject for Object {
    type ScanState = bool;

    const SCAN_KIND: ScanKind = ScanKind::Bounded { max_work: 1 };

    fn begin_scan(&self) -> bool {
        false
    }

    fn scan(
        &self,
        state: &mut bool,
        budget: &mut ScanBudget,
        visitor: &mut EdgeVisitor<'_>,
    ) -> Result<ScanStatus, GcError> {
        if !*state {
            if !budget.take(1) {
                return Ok(ScanStatus::Pending);
            }
            if let Some(edge) = &self.next
                // SAFETY: this admitted object retains the field while visited.
                && unsafe { visitor.try_visit_stored(edge, budget)? } == ScanStatus::Pending
            {
                return Ok(ScanStatus::Pending);
            }
            *state = true;
        }
        Ok(ScanStatus::Complete)
    }
}

impl Drop for Object {
    fn drop(&mut self) {
        self.drops.fetch_add(1, Ordering::Relaxed);
    }
}
