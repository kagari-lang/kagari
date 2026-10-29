//! Owned, pointer-free provider continuations in a reusable aligned runtime workspace.
#[cfg(test)]
mod tests;
use crate::{
    error::RuntimeError,
    gc::{
        GcHeap,
        metrics::{IndivisibleKind, IndivisibleSample},
    },
    native::{
        binding::NativeResult,
        storage::{NativePayload, value::StoredValue},
    },
};
use std::{
    alloc::{Layout, alloc, dealloc},
    any::{Any, TypeId},
    fmt,
    mem::size_of_val,
    panic::{AssertUnwindSafe, catch_unwind, resume_unwind},
    ptr::{self, NonNull},
};

pub(crate) const NORMAL_WORK: usize = 1024;
// One inspected provider unit can emit one edge, whose owner may reset four words.
pub(crate) const EDGE_SCAN_WORK: usize = 5;
pub(crate) const MAX_BOUNDED_WORK: usize = NORMAL_WORK / EDGE_SCAN_WORK;

/// The work guarantee of a payload's single scanner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeScanKind {
    EdgeFree,
    /// A whole scan of at most 204 provider units. Each unit also reserves up to
    /// four collector bitmap-word resets, fitting the 1024-unit service allowance.
    /// Larger providers must use `Resumable` or explicitly declare `Indivisible`.
    Bounded {
        max_work: usize,
    },
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

    /// Return unused work from a reservation made by the same owner before entry.
    pub(crate) fn refund(&mut self, work: usize) {
        assert!(work <= self.spent);
        self.spent -= work;
        self.remaining += work;
    }

    pub fn spent(&self) -> usize {
        self.spent
    }

    pub(crate) fn remaining(&self) -> usize {
        self.remaining
    }
}

type Inspect = for<'a> fn(&'a dyn Any, &mut dyn FnMut(&'a StoredValue)) -> NativeResult<()>;
type Begin = unsafe fn(&dyn Any, *mut u8);
type Step = for<'a> unsafe fn(
    &'a dyn Any,
    *mut u8,
    &mut ScanBudget,
    &mut dyn FnMut(&'a StoredValue),
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
            visit: &mut dyn FnMut(&'a StoredValue),
        ) -> ScanStatus {
            let payload = payload.downcast_ref::<P>().expect("installed scanner type");
            // SAFETY: begin initialized this Copy state, and the workspace is exclusive.
            payload.scan(unsafe { &mut *state.cast::<P::ScanState>() }, budget, visit)
        }

        fn inspect<'a, P: NativePayload>(
            payload: &'a dyn Any,
            visit: &mut dyn FnMut(&'a StoredValue),
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
        visit: &mut dyn FnMut(&'a StoredValue),
    ) -> NativeResult<()> {
        self.validate_kind()?;
        (self.inspect)(payload, visit)
    }

    fn validate_kind(&self) -> NativeResult<()> {
        if matches!(self.kind, NativeScanKind::Bounded { max_work } if max_work > MAX_BOUNDED_WORK)
        {
            return Err(RuntimeError::module_validation(
                "native bounded scanner exceeds 204 provider units plus edge work; use Resumable or Indivisible",
            ));
        }
        Ok(())
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
        self.validate_kind()?;
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
        visit: &mut dyn FnMut(&'a StoredValue),
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

    /// Reserve both provider work and all possible edge resets before advancing
    /// its opaque continuation. An emitted edge cannot be paused independently.
    pub(crate) fn resume_traced<'a>(
        &self,
        heap: &GcHeap,
        payload: &'a dyn Any,
        workspace: &mut ScanWorkspace,
        budget: &mut ScanBudget,
        visit: &mut dyn FnMut(&'a StoredValue, &mut ScanBudget) -> NativeResult<ScanStatus>,
    ) -> NativeResult<ScanStatus> {
        let indivisible = self.kind == NativeScanKind::Indivisible;
        let available = if indivisible {
            budget.remaining
        } else {
            (budget.remaining / EDGE_SCAN_WORK).min(drain_work(self.kind))
        };
        if available == 0
            || matches!(self.kind, NativeScanKind::Bounded { max_work } if available < max_work)
        {
            return Ok(ScanStatus::Pending);
        }
        let reserved = if indivisible {
            0
        } else {
            available * EDGE_SCAN_WORK
        };
        assert!(budget.take(reserved));
        let mut provider = ScanBudget::new(available);
        let mut edges = ScanBudget::new(if indivisible {
            usize::MAX
        } else {
            available * 4
        });
        let mut error = None;
        let started = if indivisible {
            heap.observation_clock().start()
        } else {
            None
        };
        let mut emitted = 0usize;
        let mut scan = || {
            self.resume(payload, workspace, &mut provider, &mut |value| {
                emitted = emitted.saturating_add(1);
                if error.is_none() {
                    error = match visit(value, &mut edges) {
                        Ok(ScanStatus::Complete) => None,
                        Ok(ScanStatus::Pending) => Some(RuntimeError::module_validation(
                            "native scanner exhausted its reserved edge work",
                        )),
                        Err(error) => Some(error),
                    };
                }
            })
        };
        let result = if indivisible {
            catch_unwind(AssertUnwindSafe(scan))
        } else {
            Ok(scan())
        };
        let pause = started.map(|started| started.elapsed());
        budget.refund(reserved);
        let work = provider.spent().saturating_add(edges.spent());
        budget.charge(work);
        if let Some(time) = pause {
            heap.observe_indivisible(
                IndivisibleKind::ProviderScan,
                IndivisibleSample {
                    owner: "NativePayload",
                    rust_type: Some(self.payload_type),
                    items: emitted,
                    work,
                    known_bytes: Some(size_of_val(payload)),
                    time,
                },
            );
        }
        let status = match result {
            Ok(result) => result?,
            Err(panic) => resume_unwind(panic),
        };
        if let Some(error) = error {
            workspace.state = WorkspaceState::Faulted;
            return Err(error);
        }
        Ok(status)
    }

    #[cfg(test)]
    pub(crate) fn trace<'a>(
        &self,
        payload: &'a dyn Any,
        workspace: &mut ScanWorkspace,
        visit: &mut dyn FnMut(&'a StoredValue),
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

/// Transfer reported provider work even when its callback unwinds. This is a
/// stack-scoped budget owner, never a continuation or observation buffer.
struct ProviderSlice<'a> {
    parent: &'a mut ScanBudget,
    budget: ScanBudget,
}

impl Drop for ProviderSlice<'_> {
    fn drop(&mut self) {
        self.parent.charge(self.budget.spent());
    }
}

fn checked_step<'a>(
    kind: NativeScanKind,
    budget: &mut ScanBudget,
    visit: &mut dyn FnMut(&'a StoredValue),
    step: impl FnOnce(&mut ScanBudget, &mut dyn FnMut(&'a StoredValue)) -> ScanStatus,
) -> NativeResult<ScanStatus> {
    // Service may drain indefinitely, but a resumable provider still receives
    // finite slices. Keep its progress contract independent of driver scheduling.
    let available = budget.remaining.min(drain_work(kind));
    if available == 0
        || matches!(kind, NativeScanKind::Bounded { max_work } if available < max_work)
    {
        return Ok(ScanStatus::Pending);
    }
    let mut slice = ProviderSlice {
        parent: budget,
        budget: ScanBudget::new(available),
    };
    let mut edges = 0usize;
    let status = step(&mut slice.budget, &mut |value| {
        edges = edges.saturating_add(1);
        visit(value);
    });
    let spent = slice.budget.spent();
    drop(slice);
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
