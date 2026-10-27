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
    any::{Any, TypeId},
    fmt,
    ptr::{self, NonNull},
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

type Inspect = for<'a> fn(&'a dyn Any, &mut dyn FnMut(&'a Value)) -> NativeResult<()>;
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
    payload_type: TypeId,
    inspect: Inspect,
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

        fn inspect<'a, P: NativePayload>(
            payload: &'a dyn Any,
            visit: &mut dyn FnMut(&'a Value),
        ) -> NativeResult<()> {
            let payload = payload.downcast_ref::<P>().ok_or_else(|| {
                RuntimeError::module_validation("native scanner payload type mismatch")
            })?;
            let mut state = payload.begin_scan();
            loop {
                let mut budget = ScanBudget::new(drain_work(P::SCAN_KIND));
                if checked_step(P::SCAN_KIND, &mut budget, visit, |budget, visit| {
                    payload.scan(&mut state, budget, visit)
                })? == ScanStatus::Complete
                {
                    return Ok(());
                }
            }
        }

        Self {
            layout: Layout::new::<P::ScanState>(),
            kind: P::SCAN_KIND,
            payload_type: TypeId::of::<P>(),
            inspect: inspect::<P>,
            begin: begin::<P>,
            step: step::<P>,
        }
    }

    /// Admission and inspection use a scoped typed state, never the collector's
    /// suspended continuation. Both paths execute the same provider scanner and
    /// enforce the same progress contract.
    pub(crate) fn inspect<'a>(
        &self,
        payload: &'a dyn Any,
        visit: &mut dyn FnMut(&'a Value),
    ) -> NativeResult<()> {
        (self.inspect)(payload, visit)
    }

    /// Start only after the graph owner has resolved its full checked identity.
    /// The workspace checks state type, not graph identity; the graph continuation
    /// must retain that identity and resolve it again before every resume.
    pub(crate) fn begin(
        &self,
        payload: &dyn Any,
        workspace: &mut ScanWorkspace,
    ) -> NativeResult<()> {
        if workspace.state != WorkspaceState::Idle {
            return Err(RuntimeError::module_validation(
                "native scan workspace is active or faulted",
            ));
        }
        if payload.type_id() != self.payload_type {
            return Err(RuntimeError::module_validation(
                "native scanner payload type mismatch",
            ));
        }
        let state = workspace.pointer(self.layout)?;
        // An unwind cannot leave a partially initialized cursor resumable.
        workspace.state = WorkspaceState::Faulted;
        // SAFETY: The checked descriptor matches the payload and the admitted layout.
        unsafe { (self.begin)(payload, state) };
        workspace.state = WorkspaceState::Active(self.payload_type, self.layout);
        Ok(())
    }

    pub(crate) fn resume<'a>(
        &self,
        payload: &'a dyn Any,
        workspace: &mut ScanWorkspace,
        budget: &mut ScanBudget,
        visit: &mut dyn FnMut(&'a Value),
    ) -> NativeResult<ScanStatus> {
        if workspace.state != WorkspaceState::Active(self.payload_type, self.layout)
            || payload.type_id() != self.payload_type
        {
            workspace.state = WorkspaceState::Faulted;
            return Err(RuntimeError::module_validation(
                "native scan continuation mismatch",
            ));
        }
        let state = workspace.pointer(self.layout)?;
        workspace.state = WorkspaceState::Faulted;
        let status = checked_step(self.kind, budget, visit, |budget, visit| {
            // SAFETY: begin initialized this exact descriptor's Copy state. Neither
            // payload nor workspace references survive the call or workspace growth.
            unsafe { (self.step)(payload, state, budget, visit) }
        })?;
        workspace.state = match status {
            ScanStatus::Pending => WorkspaceState::Active(self.payload_type, self.layout),
            ScanStatus::Complete => WorkspaceState::Idle,
        };
        Ok(status)
    }

    #[cfg(test)]
    pub(crate) fn trace<'a>(
        &self,
        payload: &'a dyn Any,
        workspace: &mut ScanWorkspace,
        visit: &mut dyn FnMut(&'a Value),
    ) -> NativeResult<()> {
        self.begin(payload, workspace)?;
        loop {
            let mut budget = ScanBudget::new(drain_work(self.kind));
            if self.resume(payload, workspace, &mut budget, visit)? == ScanStatus::Complete {
                return Ok(());
            }
        }
    }
}

fn drain_work(kind: NativeScanKind) -> usize {
    match kind {
        NativeScanKind::Bounded { max_work } => max_work.max(1),
        NativeScanKind::Indivisible => usize::MAX,
        NativeScanKind::EdgeFree | NativeScanKind::Resumable => 256,
    }
}

fn checked_step<'a>(
    kind: NativeScanKind,
    budget: &mut ScanBudget,
    visit: &mut dyn FnMut(&'a Value),
    step: impl FnOnce(&mut ScanBudget, &mut dyn FnMut(&'a Value)) -> ScanStatus,
) -> NativeResult<ScanStatus> {
    // Service may drain indefinitely, but a resumable provider still receives
    // finite slices. Keep its progress contract independent of driver scheduling.
    let available = budget.remaining.min(drain_work(kind));
    if available == 0
        || matches!(kind, NativeScanKind::Bounded { max_work } if available < max_work)
    {
        return Ok(ScanStatus::Pending);
    }
    let mut slice = ScanBudget::new(available);
    let mut edges = 0usize;
    let status = step(&mut slice, &mut |value| {
        edges = edges.saturating_add(1);
        visit(value);
    });
    let spent = slice.spent();
    budget.charge(spent);
    if kind == NativeScanKind::EdgeFree && edges != 0 {
        return Err(RuntimeError::module_validation(
            "edge-free native scanner reported an edge",
        ));
    }
    if matches!(kind, NativeScanKind::Bounded { max_work } if spent > max_work)
        || edges > spent
        || (kind == NativeScanKind::Resumable && spent > available)
        || (status == ScanStatus::Pending && (spent == 0 || kind != NativeScanKind::Resumable))
    {
        return Err(RuntimeError::module_validation(
            "native scanner violated its progress contract",
        ));
    }
    Ok(status)
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum WorkspaceState {
    #[default]
    Idle,
    Active(TypeId, Layout),
    Faulted,
}

/// One heap-local allocation holding at most one Copy continuation. Descriptor
/// admission can grow it between steps without replacing or restarting that state.
#[derive(Default)]
pub(crate) struct ScanWorkspace {
    allocation: Option<(NonNull<u8>, Layout)>,
    state: WorkspaceState,
    #[cfg(test)]
    fail_next_growth: bool,
}

impl fmt::Debug for ScanWorkspace {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ScanWorkspace")
            .field("layout", &self.allocation.map(|(_, layout)| layout))
            .field("state", &self.state)
            .finish()
    }
}

// SAFETY: The allocation is exclusively owned; it contains only the active provider's
// Copy + Send cursor. No reference into it survives a workspace borrow.
unsafe impl Send for ScanWorkspace {}

impl ScanWorkspace {
    /// Taken edge-free payloads have no remaining outgoing obligations. Their
    /// pointer-free Copy continuation can be abandoned without provider code.
    pub(crate) fn abandon_taken(&mut self) -> NativeResult<()> {
        if self.state == WorkspaceState::Faulted {
            return Err(RuntimeError::module_validation(
                "faulted native scan abandonment",
            ));
        }
        self.state = WorkspaceState::Idle;
        Ok(())
    }

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
        #[cfg(test)]
        if self.fail_next_growth {
            self.fail_next_growth = false;
            return Err(RuntimeError::resource_limit(
                "injected native scan workspace failure",
            ));
        }
        // SAFETY: Layout is valid and nonzero. Failure leaves the old workspace intact.
        let pointer = NonNull::new(unsafe { alloc(layout) })
            .ok_or_else(|| RuntimeError::resource_limit("native scan workspace"))?;
        if let Some((old, old_layout)) = self.allocation {
            if let WorkspaceState::Active(_, active_layout) = self.state {
                // SAFETY: begin initialized a Copy state of active_layout. Copying
                // bytes preserves its padding without reading it as initialized u8.
                // Allocations are disjoint and the replacement satisfies the old
                // size/alignment as well as the newly admitted descriptor's layout.
                unsafe {
                    ptr::copy_nonoverlapping(old.as_ptr(), pointer.as_ptr(), active_layout.size());
                }
            }
            self.allocation = Some((pointer, layout));
            // SAFETY: No cursor pointer escapes a step; the replacement now owns
            // the active Copy state. The old allocation has no drop obligations.
            unsafe { dealloc(old.as_ptr(), old_layout) };
        } else {
            self.allocation = Some((pointer, layout));
        }
        Ok(())
    }

    fn pointer(&mut self, required: Layout) -> NativeResult<*mut u8> {
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
