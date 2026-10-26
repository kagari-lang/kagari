//! Owned, pointer-free provider continuations in a reusable aligned runtime workspace.
#[cfg(test)]
mod tests;
use crate::{
    error::RuntimeError,
    native::{binding::NativeResult, storage::NativePayload},
    value::Value,
};
use std::{
    alloc::{Layout, alloc, dealloc},
    any::Any,
    fmt,
    ptr::NonNull,
};

/// The work guarantee of a payload's single scanner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeScanKind {
    EdgeFree,
    Bounded { max_work: usize },
    Resumable,
    Indivisible,
}

/// Whether the captured finite extent has been completely inspected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScanStatus {
    Pending,
    Complete,
}

/// Cooperative scan work, counting inspected slots even when they contain no edge.
#[derive(Debug)]
pub struct ScanBudget {
    remaining: usize,
    spent: usize,
}

impl ScanBudget {
    pub fn new(work: usize) -> Self {
        Self {
            remaining: work,
            spent: 0,
        }
    }

    /// Spend bounded work before inspecting a slot. False leaves the budget unchanged.
    pub fn take(&mut self, work: usize) -> bool {
        if work > self.remaining {
            return false;
        }
        self.charge(work);
        true
    }

    /// Report indivisible work honestly, including work exceeding this step's budget.
    pub fn charge(&mut self, work: usize) {
        self.remaining = self.remaining.saturating_sub(work);
        self.spent = self.spent.saturating_add(work);
    }

    pub fn spent(&self) -> usize {
        self.spent
    }
}

type Begin = unsafe fn(&dyn Any, *mut u8);
type Step = for<'a> unsafe fn(
    &'a dyn Any,
    *mut u8,
    &mut ScanBudget,
    &mut dyn FnMut(&'a Value),
) -> ScanStatus;

pub(crate) struct ScanEntries {
    pub(crate) layout: Layout,
    kind: NativeScanKind,
    begin: Begin,
    step: Step,
}

impl ScanEntries {
    pub(crate) fn new<P: NativePayload>() -> Self {
        unsafe fn begin<P: NativePayload>(payload: &dyn Any, state: *mut u8) {
            let payload = payload.downcast_ref::<P>().expect("installed scanner type");
            // SAFETY: The descriptor's workspace was reserved with this state's layout.
            unsafe { state.cast::<P::ScanState>().write(payload.begin_scan()) };
        }

        unsafe fn step<'a, P: NativePayload>(
            payload: &'a dyn Any,
            state: *mut u8,
            budget: &mut ScanBudget,
            visit: &mut dyn FnMut(&'a Value),
        ) -> ScanStatus {
            let payload = payload.downcast_ref::<P>().expect("installed scanner type");
            // SAFETY: begin initialized this Copy state, and the workspace is exclusive.
            payload.scan(unsafe { &mut *state.cast::<P::ScanState>() }, budget, visit)
        }

        Self {
            layout: Layout::new::<P::ScanState>(),
            kind: P::SCAN_KIND,
            begin: begin::<P>,
            step: step::<P>,
        }
    }

    pub(crate) fn trace<'a>(
        &self,
        payload: &'a dyn Any,
        workspace: &mut ScanWorkspace,
        visit: &mut dyn FnMut(&'a Value),
    ) -> NativeResult<()> {
        let state = workspace.pointer(self.layout)?;
        // SAFETY: The descriptor matches the payload; the aligned workspace is exclusive.
        unsafe { (self.begin)(payload, state) };
        let work = match self.kind {
            NativeScanKind::Bounded { max_work } => max_work.max(1),
            NativeScanKind::Indivisible => usize::MAX,
            NativeScanKind::EdgeFree | NativeScanKind::Resumable => 256,
        };
        loop {
            let mut budget = ScanBudget::new(work);
            let mut edges = 0usize;
            let mut visitor = |value: &'a Value| {
                edges = edges.saturating_add(1);
                visit(value);
            };
            // SAFETY: The same descriptor initialized state; no references survive a step.
            let status = unsafe { (self.step)(payload, state, &mut budget, &mut visitor) };
            if self.kind == NativeScanKind::EdgeFree && edges != 0 {
                return Err(RuntimeError::module_validation(
                    "edge-free native scanner reported an edge",
                ));
            }
            if matches!(self.kind, NativeScanKind::Bounded { max_work } if budget.spent() > max_work)
                || edges > budget.spent()
                || (self.kind == NativeScanKind::Resumable && budget.spent() > work)
                || (status == ScanStatus::Pending
                    && (budget.spent() == 0 || self.kind != NativeScanKind::Resumable))
            {
                return Err(RuntimeError::module_validation(
                    "native scanner violated its progress contract",
                ));
            }
            if status == ScanStatus::Complete {
                return Ok(());
            }
        }
    }
}

/// One heap-local allocation, reserved at payload admission and reused during tracing.
/// The current synchronous driver finishes one continuation before starting another.
#[derive(Default)]
pub(crate) struct ScanWorkspace {
    allocation: Option<(NonNull<u8>, Layout)>,
}

impl fmt::Debug for ScanWorkspace {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ScanWorkspace")
            .field("layout", &self.allocation.map(|(_, layout)| layout))
            .finish()
    }
}

// SAFETY: The allocation is exclusively owned; it contains only the active provider's
// Copy + Send cursor. No reference into it survives a workspace borrow.
unsafe impl Send for ScanWorkspace {}

impl ScanWorkspace {
    pub(crate) fn reserve(&mut self, required: Layout) -> NativeResult<()> {
        let size = required.size().max(1);
        if self
            .allocation
            .is_some_and(|(_, layout)| layout.size() >= size && layout.align() >= required.align())
        {
            return Ok(());
        }
        let (old_size, old_align) = self
            .allocation
            .map_or((0, 1), |(_, layout)| (layout.size(), layout.align()));
        let layout = Layout::from_size_align(size.max(old_size), required.align().max(old_align))
            .map_err(|_| RuntimeError::resource_limit("native scan workspace layout"))?;
        // SAFETY: Layout is valid and nonzero. Failure leaves the old workspace intact.
        let pointer = NonNull::new(unsafe { alloc(layout) })
            .ok_or_else(|| RuntimeError::resource_limit("native scan workspace"))?;
        if let Some((old, old_layout)) = self.allocation.replace((pointer, layout)) {
            // SAFETY: Admission only reserves an idle workspace. The next begin
            // initializes its state; no previous cursor or reference survives here.
            unsafe { dealloc(old.as_ptr(), old_layout) };
        }
        Ok(())
    }

    pub(crate) fn pointer(&mut self, required: Layout) -> NativeResult<*mut u8> {
        self.allocation
            .filter(|(_, layout)| {
                layout.size() >= required.size() && layout.align() >= required.align()
            })
            .map(|(pointer, _)| pointer.as_ptr())
            .ok_or_else(|| {
                RuntimeError::module_validation("native scan workspace was not admitted")
            })
    }
}

impl Drop for ScanWorkspace {
    fn drop(&mut self) {
        if let Some((pointer, layout)) = self.allocation.take() {
            // SAFETY: This workspace owns the allocation and the cursor needs no drop.
            unsafe { dealloc(pointer.as_ptr(), layout) };
        }
    }
}
