#![allow(dead_code)]

use kagari_gc::{
    access::{Access, EdgeField, ScalarField, Scope},
    error::GcError,
    object::{Edge, Gc, GcObject, ObjectCharge},
    root::Root,
    scan::{EdgeVisitor, ScanBudget, ScanKind, ScanStatus},
};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

pub struct Node {
    value: i64,
    next: Option<Edge<Node>>,
    drops: Arc<AtomicUsize>,
}

impl Node {
    pub fn alloc_scoped<'gc>(
        cx: &mut Scope<'gc>,
        value: i64,
        drops: &Arc<AtomicUsize>,
    ) -> Result<Gc<'gc, Self>, GcError> {
        // SAFETY: initially edge-free; all later edits use checked fields/providers.
        unsafe {
            cx.alloc_with(|_| {
                Ok(Self {
                    value,
                    next: None,
                    drops: drops.clone(),
                })
            })
        }
    }

    // SAFETY: a plain scalar with no tracing/charge dependency.
    pub const VALUE: ScalarField<Self, i64> =
        unsafe { ScalarField::new(|node| &node.value, |node| &mut node.value) };

    // SAFETY: one fixed traced position with no coupled invariants or capacity.
    pub const NEXT: EdgeField<Self, Self> =
        unsafe { EdgeField::new(|node| &node.next, |node| &mut node.next) };

    pub fn alloc(
        access: &mut Access<'_>,
        value: i64,
        drops: &Arc<AtomicUsize>,
    ) -> Result<Root<Self>, GcError> {
        let descriptor = access.register::<Self>()?;
        // SAFETY: no edges initially; all later writes are encapsulated below.
        unsafe {
            access.alloc_with(&descriptor, |_| {
                Ok(Self {
                    value,
                    next: None,
                    drops: drops.clone(),
                })
            })
        }
    }

    pub fn value(access: &mut Access<'_>, owner: &Root<Self>) -> Result<i64, GcError> {
        access.with_object(owner, |view| view.object().value)
    }

    pub fn next_edge(&self) -> Option<&Edge<Self>> {
        self.next.as_ref()
    }

    pub fn value_field_mut(&mut self) -> &mut i64 {
        &mut self.value
    }

    pub fn set_next(
        access: &mut Access<'_>,
        owner: &Root<Self>,
        next: Option<&Root<Self>>,
    ) -> Result<(), GcError> {
        // SAFETY: import before replacing, fixed scan position, no callbacks/growth.
        unsafe {
            access.with_object_mut(owner, |node, mutation| {
                node.next = next.map(|root| mutation.edge(root)).transpose()?;
                mutation.continue_scan();
                Ok(())
            })
        }
    }

    pub fn next(
        access: &mut Access<'_>,
        owner: &Root<Self>,
    ) -> Result<Option<Root<Self>>, GcError> {
        access.with_object(owner, |mut view| {
            let Some(edge) = view.object().next.as_ref() else {
                return Ok(None);
            };
            view.retain(edge).map(Some)
        })?
    }
}

// SAFETY: the only edge is always visited, writes publish through admitted sources,
// cursor is owned, charge is fixed, and Drop touches only a host counter.
unsafe impl GcObject for Node {
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
            if let Some(edge) = &self.next {
                // SAFETY: this admitted node retains its stored edge through the visit.
                if unsafe { visitor.try_visit_stored(edge, budget)? } == ScanStatus::Pending {
                    return Ok(ScanStatus::Pending);
                }
            }
            *state = true;
        }
        Ok(ScanStatus::Complete)
    }
}

impl Drop for Node {
    fn drop(&mut self) {
        self.drops.fetch_add(1, Ordering::Relaxed);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VecScan {
    pub next: usize,
    pub end: usize,
}

pub struct ObjectVec {
    items: Vec<Edge<Node>>,
    label: i64,
}

impl ObjectVec {
    // SAFETY: user label is independent of references, scan coordinates and charge.
    pub const LABEL: ScalarField<Self, i64> =
        unsafe { ScalarField::new(|object| &object.label, |object| &mut object.label) };

    pub fn item_edge(&self, index: usize) -> Option<&Edge<Node>> {
        self.items.get(index)
    }

    pub fn clear(&mut self) {
        self.items.clear();
    }

    pub fn truncate(&mut self, len: usize) {
        self.items.truncate(len);
    }

    pub fn alloc(access: &mut Access<'_>, items: &[Root<Node>]) -> Result<Root<Self>, GcError> {
        let descriptor = access.register::<Self>()?;
        // SAFETY: every initialized edge is imported from a live Root under exclusion.
        unsafe {
            access.alloc_with(&descriptor, |mut init| {
                let mut stored = Vec::new();
                stored.try_reserve_exact(items.len())?;
                for root in items {
                    stored.push(init.edge(root)?);
                }
                Ok(Self {
                    items: stored,
                    label: 0,
                })
            })
        }
    }

    pub fn remove(
        access: &mut Access<'_>,
        owner: &Root<Self>,
        index: usize,
    ) -> Result<Root<Node>, GcError> {
        // SAFETY: retain first, remove/repair in one closed commit, no new edges or growth.
        unsafe {
            access.with_object_mut(owner, |vec, mutation| {
                let edge = vec.items.get(index).ok_or(GcError::Index)?;
                let result = mutation.retain_existing(edge)?;
                mutation.with_scan_state(|state| {
                    vec.items.remove(index);
                    if let Some(state) = state {
                        if index < state.next {
                            state.next -= 1;
                        }
                        if index < state.end {
                            state.end -= 1;
                        }
                    }
                });
                mutation.continue_scan();
                Ok(result)
            })
        }
    }

    pub fn push(
        access: &mut Access<'_>,
        owner: &Root<Self>,
        value: &Root<Node>,
    ) -> Result<(), GcError> {
        // SAFETY: publish first. Staged growth is checked at actual allocated capacity
        // before moving stored edges; the old numeric extent stays bounds-valid.
        unsafe {
            access.with_object_mut(owner, |vec, mutation| {
                let edge = mutation.edge(value)?;
                if vec.items.len() == vec.items.capacity() {
                    let wanted = vec
                        .items
                        .len()
                        .checked_add(1)
                        .ok_or(GcError::Limit("vector length"))?;
                    let mut grown = Vec::new();
                    grown.try_reserve_exact(wanted)?;
                    mutation.prepare_charge(ObjectCharge {
                        bytes: grown.capacity() * size_of::<Edge<Node>>(),
                        units: wanted,
                    })?;
                    grown.append(&mut vec.items);
                    vec.items = grown;
                }
                vec.items.push(edge);
                mutation.continue_scan();
                Ok(())
            })
        }
    }

    pub fn values(access: &mut Access<'_>, owner: &Root<Self>) -> Result<Vec<Root<Node>>, GcError> {
        access.with_object(owner, |mut view| {
            let mut roots = Vec::new();
            for index in 0..view.object().items.len() {
                let edge = &view.object().items[index];
                roots.push(view.retain(edge)?);
            }
            Ok(roots)
        })?
    }

    pub fn cursor(access: &mut Access<'_>, owner: &Root<Self>) -> Result<Option<VecScan>, GcError> {
        // SAFETY: diagnostic copies only pointer-free state, without editing the graph.
        unsafe {
            access.with_object_mut(owner, |_, mutation| {
                let state = mutation.with_scan_state(|state| state.copied());
                mutation.continue_scan();
                Ok(state)
            })
        }
    }
}

// SAFETY: every occupied slot is traced, cursors keep no buffer pointers, and all
// structural edits either repair the exact cursor or preserve it with publication.
unsafe impl GcObject for ObjectVec {
    type ScanState = VecScan;

    const SCAN_KIND: ScanKind = ScanKind::Resumable;

    fn begin_scan(&self) -> VecScan {
        VecScan {
            next: 0,
            end: self.items.len(),
        }
    }

    fn scan(
        &self,
        state: &mut VecScan,
        budget: &mut ScanBudget,
        visitor: &mut EdgeVisitor<'_>,
    ) -> Result<ScanStatus, GcError> {
        while state.next < state.end {
            if !budget.take(1) {
                return Ok(ScanStatus::Pending);
            }
            let edge = self
                .items
                .get(state.next)
                .ok_or(GcError::Provider("invalid repaired vector cursor"))?;
            // SAFETY: the initialized owning vector protects this source slot.
            if unsafe { visitor.try_visit_stored(edge, budget)? } == ScanStatus::Pending {
                return Ok(ScanStatus::Pending);
            }
            state.next += 1;
        }
        Ok(ScanStatus::Complete)
    }

    fn charge(&self) -> ObjectCharge {
        ObjectCharge {
            bytes: self.items.capacity() * size_of::<Edge<Node>>(),
            units: self.items.len(),
        }
    }
}
