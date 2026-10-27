//! Persistent runtime-local work credits and colors, independent of storage borrows.
//!
//! Collection drains this marker synchronously; PG05 owns incremental scheduling.
pub(crate) mod driver;
pub(crate) mod marks;
pub(crate) mod objects;
#[cfg(test)]
mod owner_tests;
mod owners;
#[cfg(test)]
mod poison_tests;
#[cfg(test)]
mod publication_tests;
pub(crate) mod roots;
pub(in crate::gc) mod sweep;
#[cfg(test)]
mod tests;
#[cfg(feature = "execution-diagnostics")]
use crate::diagnostics::{
    allocations,
    storage::{BufferUsage, RetirementUsage},
};
use crate::{
    error::RuntimeError,
    execution_metadata::{
        applications::ApplicationId,
        environments::EnvironmentId,
        groups::OperationGroupId,
        interfaces::InterfaceSnapshotId,
        scan_records::{InterfaceCallScanId, InterfaceDescriptionScanId, NativeApplicationScanId},
    },
    gc::{
        HeapObjectId,
        graph::marks::{Color, MarkRef, WorkGroup},
    },
    module::scanning::{ProgramScanId, StaticScanId},
    native::storage::scan::ScanBudget,
};
use std::{
    fmt,
    ops::{Deref, DerefMut},
    sync::{
        Arc, Mutex, MutexGuard, TryLockError,
        atomic::{AtomicU8, Ordering},
    },
    thread,
};

/// Shared control state only; storage and active payload cursors stay with their
/// runtime owners. Never hold this lock while invoking a provider or an edge visitor.
#[derive(Clone, Default)]
pub(crate) struct GraphState(Arc<GraphControl>);

impl fmt::Debug for GraphState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Mutex's Debug implementation takes a raw guard. Formatting must use
        // our guard too: a user-supplied writer can panic while printing work.
        let mut output = f.debug_struct("GraphState");
        match self.try_lock() {
            Ok(marker) => output
                .field("phase", &marker.phase())
                .field("work", &*marker)
                .finish(),
            Err(error) => output
                .field("phase", &self.phase())
                .field("unavailable", &error)
                .finish_non_exhaustive(),
        }
    }
}

/// Phase is authoritative here, not mirrored inside the locked work state. The
/// exclusive Runtime driver serializes transitions with mutation; the atomic
/// preserves Send/Sync for shared publication owners without a lock on Idle reads.
/// Poison is a terminal phase, not a cached copy of Mutex's advisory flag. All
/// acquisitions, including Debug, must immediately wrap the raw guard; there is
/// no callback, allocation or fallible work between acquisition and wrapping.
#[derive(Default)]
struct GraphControl {
    phase: AtomicU8,
    work: Mutex<GraphWork>,
}

/// Like MutexGuard, but records Poisoned before unwinding unlocks. Even a guard
/// acquired during unwinding poisons this owner; Mutex itself may not flag that.
#[derive(Debug)]
pub(crate) struct GraphMarker<'a> {
    phase: &'a AtomicU8,
    work: MutexGuard<'a, GraphWork>,
}

impl Deref for GraphMarker<'_> {
    type Target = GraphWork;

    fn deref(&self) -> &GraphWork {
        &self.work
    }
}

impl DerefMut for GraphMarker<'_> {
    fn deref_mut(&mut self) -> &mut GraphWork {
        &mut self.work
    }
}

impl Drop for GraphMarker<'_> {
    fn drop(&mut self) {
        if thread::panicking() {
            self.set_phase(MarkPhase::Poisoned);
        }
    }
}

impl GraphState {
    pub(crate) fn try_lock(&self) -> Result<GraphMarker<'_>, RuntimeError> {
        match self.0.work.try_lock() {
            Ok(work) => {
                let marker = GraphMarker {
                    phase: &self.0.phase,
                    work,
                };
                // A guard acquired during unwinding can poison our owner without
                // poisoning Mutex. Never expose work again in that terminal state.
                if marker.phase() == MarkPhase::Poisoned {
                    return Err(Self::poisoned());
                }
                Ok(marker)
            }
            Err(TryLockError::Poisoned(poisoned)) => {
                self.0
                    .phase
                    .store(MarkPhase::Poisoned as u8, Ordering::Release);
                drop(poisoned);
                Err(Self::poisoned())
            }
            Err(TryLockError::WouldBlock) => Err(RuntimeError::module_validation(
                "graph control state is borrowed",
            )),
        }
    }

    pub(crate) fn lock(&self) -> GraphMarker<'_> {
        self.try_lock().expect("exclusive runtime graph access")
    }

    #[inline]
    fn phase(&self) -> MarkPhase {
        MarkPhase::load(&self.0.phase)
    }

    #[cold]
    fn poisoned() -> RuntimeError {
        RuntimeError::module_validation("poisoned graph control state")
    }

    /// Call only after the original owner checks identity and occupancy, even in
    /// Idle. Phase-first filtering avoids queue synchronization, not validation.
    #[inline]
    pub(crate) fn shade(&self, id: GraphIdentity, mark: MarkRef<'_>) -> Result<(), RuntimeError> {
        self.with_publication(|| (id, mark))
    }

    /// The caller already resolved its original owner under a live store borrow.
    /// Construct graph-only arguments only when the authoritative phase needs them;
    /// the closure must not perform callbacks or replace identity validation.
    #[inline]
    pub(crate) fn with_publication<'a>(
        &self,
        target: impl FnOnce() -> (GraphIdentity, MarkRef<'a>),
    ) -> Result<(), RuntimeError> {
        match self.phase() {
            MarkPhase::Idle => Ok(()),
            MarkPhase::Faulted => Err(Self::faulted_publication()),
            MarkPhase::Poisoned => Err(Self::poisoned()),
            MarkPhase::Roots | MarkPhase::Mark | MarkPhase::Sweep => self.publish_active(target),
        }
    }

    #[cold]
    fn faulted_publication() -> RuntimeError {
        RuntimeError::module_validation("faulted graph publication")
    }

    // Keep marker service and its guard/error storage outside ordinary mutator
    // admission. The caller's original-owner borrow still covers target creation.
    #[inline(never)]
    fn publish_active<'a>(
        &self,
        target: impl FnOnce() -> (GraphIdentity, MarkRef<'a>),
    ) -> Result<(), RuntimeError> {
        let (id, mark) = target();
        self.try_lock()?.shade(id, mark)
    }

    /// Read access never creates retention. Poison is an error; an explicit trace
    /// fault still permits diagnostic reads, as before, but never publication.
    #[inline]
    pub(crate) fn accessible(&self, mark: MarkRef<'_>) -> Result<bool, RuntimeError> {
        self.with_access(|| mark)
    }

    /// Like publication, ordinary availability must validate the owner first but
    /// need not materialize a mark when there is no condemnation boundary.
    #[inline]
    pub(crate) fn with_access<'a>(
        &self,
        mark: impl FnOnce() -> MarkRef<'a>,
    ) -> Result<bool, RuntimeError> {
        match self.phase() {
            MarkPhase::Poisoned => Err(Self::poisoned()),
            MarkPhase::Sweep => self.access_during_sweep(mark),
            MarkPhase::Idle | MarkPhase::Roots | MarkPhase::Mark | MarkPhase::Faulted => Ok(true),
        }
    }

    #[inline(never)]
    fn access_during_sweep<'a>(
        &self,
        mark: impl FnOnce() -> MarkRef<'a>,
    ) -> Result<bool, RuntimeError> {
        Ok(self.try_lock()?.accessible(mark()))
    }
}

/// Domains are deliberately distinct: an inert taken identity never inherits the
/// mark of a new heap generation occupying the same physical directory index.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum GraphIdentity {
    Object(HeapObjectId),
    Moved(HeapObjectId),
    Environment(EnvironmentId),
    Group(OperationGroupId),
    Application(ApplicationId),
    Interface(InterfaceSnapshotId),
    NativeApplication(NativeApplicationScanId),
    InterfaceCall(InterfaceCallScanId),
    InterfaceDescription(InterfaceDescriptionScanId),
    Program(ProgramScanId),
    Static(StaticScanId),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub(crate) enum MarkPhase {
    Idle = 0,
    Roots,
    Mark,
    Sweep,
    /// An explicit trace fault: no publication or sweep, but diagnostic reads remain valid.
    Faulted,
    /// Work may have been interrupted by unwinding; even diagnostic access is rejected.
    Poisoned,
}

impl MarkPhase {
    fn load(phase: &AtomicU8) -> Self {
        match phase.load(Ordering::Acquire) {
            0 => Self::Idle,
            1 => Self::Roots,
            2 => Self::Mark,
            3 => Self::Sweep,
            4 => Self::Faulted,
            5 => Self::Poisoned,
            _ => unreachable!("private graph phase encoding"),
        }
    }
}

/// Block jobs batch owner-local gray bits (Go 1.25 Green Tea's seen/scanned
/// mechanism). Kagari retains one resumable owner, not a whole-page scan.
/// https://go.dev/blog/greenteagc
#[derive(Debug, Clone, Copy)]
enum WorkItem {
    Group(WorkGroup),
    Singleton(GraphIdentity),
}

/// Identity admission and pending-storage capacity have distinct bounds. Every
/// admitted identity has an original-owner mark; every registered slot block and
/// live singleton reserves one queue position before publication. A block's queued
/// bit deduplicates jobs even while one member is active. Thus pending jobs never
/// exceed work credits, while gray identities (including the active owner) never
/// exceed identity credits. Empty jobs left by native take retain their block
/// credit until resolved; they do not require a live payload or a second registry.
#[derive(Debug, Default)]
pub(crate) struct GraphWork {
    epoch: u64,
    credits: usize,
    bitmap_work: usize,
    pending: Vec<WorkItem>,
    work_credits: usize,
    active: Option<GraphIdentity>,
    roots_finished: bool,
    prepared: bool,
    #[cfg(feature = "execution-diagnostics")]
    retirement: RetirementUsage,
    #[cfg(test)]
    fail_reservation: bool,
}

impl GraphMarker<'_> {
    #[cfg(feature = "execution-diagnostics")]
    pub(crate) fn storage_usage(&self) -> (BufferUsage, usize, usize, RetirementUsage) {
        (
            BufferUsage::new("graph_pending", &self.pending),
            self.credits,
            self.work_credits,
            self.retirement,
        )
    }

    /// Caller keeps this exclusive borrow through physical admission and commit.
    /// A failed physical admission may leave spare capacity, never an extra credit.
    pub(crate) fn prepare_identity(&mut self) -> Result<(), RuntimeError> {
        if matches!(self.phase(), MarkPhase::Faulted | MarkPhase::Poisoned) {
            return Err(RuntimeError::module_validation("faulted graph admission"));
        }
        self.prepared = false;
        self.credits
            .checked_add(1)
            .ok_or_else(|| RuntimeError::resource_limit("graph identity credits"))?;
        let count = self
            .work_credits
            .checked_add(1)
            .ok_or_else(|| RuntimeError::resource_limit("graph block credits"))?;
        #[cfg(test)]
        if self.fail_reservation {
            self.fail_reservation = false;
            return Err(RuntimeError::resource_limit(
                "injected graph reservation failure",
            ));
        }

        let additional = count - self.pending.len();
        self.pending
            .try_reserve(additional)
            .map_err(|_| RuntimeError::resource_limit("graph pending credits"))?;
        self.prepared = true;
        Ok(())
    }

    /// Commit before exposing a new identity. Outgoing targets require separate
    /// validation, including the survivor rule in Sweep; this is not an edge check.
    pub(crate) fn admit_prepared(&mut self, id: GraphIdentity, mark: MarkRef<'_>) {
        assert!(self.prepared, "prepared fresh graph identity");
        assert!(
            self.pending.capacity() > self.work_credits,
            "reserved graph work credit"
        );
        self.prepared = false;
        if mark.register() {
            self.work_credits += 1;
        }
        let color = match self.phase() {
            MarkPhase::Idle => Color::White,
            MarkPhase::Roots | MarkPhase::Mark => Color::Gray,
            MarkPhase::Sweep => Color::Black,
            MarkPhase::Faulted | MarkPhase::Poisoned => panic!("admission after graph fault"),
        };
        self.bitmap_work += mark.set(self.epoch, color);
        if color == Color::Gray {
            self.enqueue(id, mark);
        }
        self.credits += 1;
        self.check_credits();
    }

    /// Transfer color to the inert identity's original moved-ledger slot block.
    /// Clearing the directory bit can leave an empty block job, whose registered
    /// block credit remains reserved. Active work retargets through the moved ledger.
    /// Neither transfer nor resolution searches the queue or allocates.
    pub(crate) fn admit_moved_prepared(
        &mut self,
        id: GraphIdentity,
        source: MarkRef<'_>,
        target: MarkRef<'_>,
    ) {
        assert!(self.prepared && self.pending.capacity() > self.work_credits);
        self.prepared = false;
        if target.register() {
            self.work_credits += 1;
        }
        let color = source.color(self.epoch);
        self.bitmap_work += target.set(self.epoch, color);
        self.bitmap_work += source.set(self.epoch, Color::White);
        if color == Color::Gray {
            self.enqueue(id, target);
        }
        self.credits += 1;
        self.check_credits();
    }

    fn enqueue(&mut self, id: GraphIdentity, mark: MarkRef<'_>) {
        let work = match mark.queue_group() {
            Some((_, false)) => return,
            Some((group, true)) => WorkItem::Group(group),
            None => WorkItem::Singleton(id),
        };
        assert!(
            self.pending.len() < self.pending.capacity(),
            "reserved block work"
        );
        self.pending.push(work);
    }

    pub(crate) fn release_singleton(&mut self, mark: MarkRef<'_>) {
        if mark.unregister_singleton() {
            self.work_credits -= 1;
        }
    }

    pub(crate) fn retarget_taken_owner(
        &mut self,
        old: GraphIdentity,
        moved: GraphIdentity,
    ) -> Result<(), RuntimeError> {
        if self.active != Some(old)
            || !matches!((old, moved), (GraphIdentity::Object(a), GraphIdentity::Moved(b)) if a == b)
        {
            return Err(Self::invalid("invalid taken work transfer"));
        }
        self.active = Some(moved);
        Ok(())
    }

    /// A failed mark has no condemnation boundary; safe diagnostic reads remain
    /// available. Only successful closure makes old white owners inaccessible.
    pub(crate) fn accessible(&self, mark: MarkRef<'_>) -> bool {
        match self.phase() {
            MarkPhase::Poisoned => false,
            MarkPhase::Sweep => mark.color(self.epoch) == Color::Black,
            MarkPhase::Idle | MarkPhase::Roots | MarkPhase::Mark | MarkPhase::Faulted => true,
        }
    }

    /// Remove a credit only when its work is impossible. Gray identities must
    /// finish first, even if their semantic owners have already released them.
    pub(crate) fn can_retire(&self, mark: MarkRef<'_>) -> Result<(), RuntimeError> {
        if matches!(self.phase(), MarkPhase::Faulted | MarkPhase::Poisoned)
            || (matches!(self.phase(), MarkPhase::Roots | MarkPhase::Mark)
                && mark.color(self.epoch) == Color::Gray)
        {
            return Err(Self::invalid("retiring unfinished graph work"));
        }
        Ok(())
    }

    /// The owner validates occupancy/generation and preflights can_retire before
    /// detachment. Only that physical retirement can release this credit.
    pub(crate) fn retire_prepared(&mut self) {
        self.credits = self
            .credits
            .checked_sub(1)
            .expect("admitted retirement credit");
        self.check_credits();
    }

    pub(crate) fn phase(&self) -> MarkPhase {
        MarkPhase::load(self.phase)
    }

    fn set_phase(&self, phase: MarkPhase) {
        self.phase.store(phase as u8, Ordering::Release);
    }

    /// No provider invocation, graph traversal or allocation occurs here.
    pub(crate) fn shade(
        &mut self,
        id: GraphIdentity,
        mark: MarkRef<'_>,
    ) -> Result<(), RuntimeError> {
        match self.phase() {
            MarkPhase::Idle => return Ok(()),
            MarkPhase::Faulted => return Err(Self::invalid("faulted graph publication")),
            MarkPhase::Poisoned => return Err(GraphState::poisoned()),
            MarkPhase::Sweep => {
                return if mark.color(self.epoch) == Color::Black {
                    Ok(())
                } else {
                    Err(Self::invalid("condemned graph edge"))
                };
            }
            MarkPhase::Roots | MarkPhase::Mark => {}
        }
        if mark.color(self.epoch) == Color::White {
            self.bitmap_work += mark.set(self.epoch, Color::Gray);
            self.enqueue(id, mark);
        }
        self.check_credits();
        Ok(())
    }

    pub(crate) fn begin_cycle(&mut self) -> Result<(), RuntimeError> {
        if self.phase() != MarkPhase::Idle || !self.pending.is_empty() || self.active.is_some() {
            return Err(Self::invalid("graph cycle already active"));
        }
        let Some(epoch) = self.epoch.checked_add(1) else {
            self.fault();
            return Err(Self::invalid("graph mark epoch exhausted"));
        };
        self.epoch = epoch;
        self.bitmap_work = 0;
        self.set_phase(MarkPhase::Roots);
        self.roots_finished = false;
        Ok(())
    }

    /// The active identity stays gray and keeps its credit while a scanner is
    /// suspended. Repeated calls return it, never start a second continuation.
    pub(crate) fn needs_owner(&self) -> bool {
        self.active.is_none() && !self.pending.is_empty()
    }

    /// Resolve at most one block, including an empty block left by native take.
    pub(crate) fn next_owner(
        &mut self,
        mut resolve: impl FnMut(WorkGroup, u64) -> Result<(Option<GraphIdentity>, bool), RuntimeError>,
    ) -> Result<Option<GraphIdentity>, RuntimeError> {
        if !matches!(self.phase(), MarkPhase::Roots | MarkPhase::Mark) {
            return Err(Self::invalid("graph work outside marking"));
        }
        if self.active.is_none()
            && let Some(work) = self.pending.last().copied()
        {
            match work {
                WorkItem::Singleton(id) => {
                    self.pending.pop();
                    self.active = Some(id);
                }
                WorkItem::Group(group) => {
                    let (id, more) = resolve(group, self.epoch)?;
                    if !more {
                        self.pending.pop();
                    }
                    self.active = id;
                }
            }
        }
        self.check_credits();
        Ok(self.active)
    }

    pub(crate) fn finish_owner(
        &mut self,
        id: GraphIdentity,
        mark: MarkRef<'_>,
    ) -> Result<(), RuntimeError> {
        if !matches!(self.phase(), MarkPhase::Roots | MarkPhase::Mark) || self.active != Some(id) {
            return Err(Self::invalid("graph completion identity mismatch"));
        }
        if mark.color(self.epoch) != Color::Gray {
            return Err(Self::invalid("invalid active graph color"));
        }
        self.bitmap_work += mark.set(self.epoch, Color::Black);
        self.active = None;
        Ok(())
    }

    pub(crate) fn finish_roots(&mut self) -> Result<(), RuntimeError> {
        if self.phase() != MarkPhase::Roots {
            return Err(Self::invalid("invalid root completion"));
        }
        self.roots_finished = true;
        self.set_phase(MarkPhase::Mark);
        Ok(())
    }

    /// Call under the execution/loan guard after all initial root cursors finish.
    /// This checks closure, never rescans roots or converts partial marks to success.
    pub(crate) fn close_mark(&mut self) -> Result<(), RuntimeError> {
        if self.phase() != MarkPhase::Mark
            || !self.roots_finished
            || self.active.is_some()
            || !self.pending.is_empty()
            || self.bitmap_work != 0
        {
            return Err(Self::invalid("graph marking is not closed"));
        }
        self.set_phase(MarkPhase::Sweep);
        Ok(())
    }

    pub(crate) fn is_survivor(&self, mark: MarkRef<'_>) -> Result<bool, RuntimeError> {
        if self.phase() != MarkPhase::Sweep {
            return Err(Self::invalid("marks are not authoritative"));
        }
        Ok(mark.color(self.epoch) == Color::Black)
    }

    pub(crate) fn begin_retirement(&mut self) {
        assert_eq!(self.phase(), MarkPhase::Sweep);
        #[cfg(feature = "execution-diagnostics")]
        {
            let counts = allocations::current();
            self.retirement = RetirementUsage {
                pre_detach_net_bytes: counts.map(|counts| counts.net_bytes),
                pre_detach_interval_peak_bytes: counts.map(|counts| counts.interval_peak_net_bytes),
                max_detached: 0,
                module_slots: None,
            };
        }
    }

    #[cfg(feature = "execution-diagnostics")]
    pub(crate) fn record_module_storage(&mut self, slots: BufferUsage) {
        self.retirement.module_slots = Some(slots);
    }

    pub(crate) fn observe_retirement(&mut self, count: usize) {
        assert_eq!(self.phase(), MarkPhase::Sweep);
        assert!(count <= 32, "bounded total detached owners");
        #[cfg(feature = "execution-diagnostics")]
        {
            self.retirement.max_detached = self.retirement.max_detached.max(count);
        }
    }

    pub(crate) fn finish_sweep(&mut self) -> Result<(), RuntimeError> {
        if self.phase() != MarkPhase::Sweep {
            return Err(Self::invalid("invalid graph sweep completion"));
        }
        self.set_phase(MarkPhase::Idle);
        self.trim_idle_capacity();
        Ok(())
    }

    /// Once per successful sweep, never per retirement. Hysteresis avoids repeated
    /// shrinking/growing; capacity covers registered blocks and admitted singletons.
    /// Stable slot blocks remain with their generation-owning stores, not payload pages.
    pub(crate) fn trim_idle_capacity(&mut self) {
        assert_eq!(self.phase(), MarkPhase::Idle);
        assert!(self.pending.is_empty() && self.active.is_none());
        if self.pending.capacity() > self.work_credits.saturating_mul(4).max(64) {
            let target = self.work_credits.saturating_mul(2);
            self.pending.shrink_to(target);
        }
        self.check_credits();
    }

    pub(crate) fn charge_bitmap_work(&mut self, budget: &mut ScanBudget) -> bool {
        while self.bitmap_work != 0 {
            if !budget.take(1) {
                return false;
            }
            self.bitmap_work -= 1;
        }
        true
    }

    pub(crate) fn fault(&mut self) {
        if self.phase() != MarkPhase::Poisoned {
            self.set_phase(MarkPhase::Faulted);
        }
    }

    fn check_credits(&self) {
        assert!(
            self.pending.capacity() >= self.work_credits,
            "graph credit capacity"
        );
        assert!(
            self.pending.len() <= self.work_credits,
            "one queued job per registered block or singleton"
        );
    }

    fn invalid(message: &'static str) -> RuntimeError {
        RuntimeError::module_validation(message)
    }
}
