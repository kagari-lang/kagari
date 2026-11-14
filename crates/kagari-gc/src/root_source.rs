//! Driver-owned execution roots sharing the ordinary heap marking engine.

use crate::{
    access::{Access, Scope},
    error::GcError,
    heap::{
        CollectionReport, Heap, HeapConfig, HeapStats, ShutdownReport, StepReport,
        accounting::AccountingReport, metadata::MetadataStats, pacing::AutomaticReport,
    },
    object::{Edge, Gc},
    scan::{EdgeVisitor, ScanBudget, ScanStatus, WorkBudget},
};
use std::{marker::PhantomData, time::Duration};

/// Complete, bounded enumeration of roots in execution-owned storage.
///
/// This source supplies references to heap objects, not foreign graph nodes.
/// Implementations encapsulate their slots behind reviewed mutation operations.
///
/// # Safety
/// `empty` must construct storage with no managed references. Every installed
/// reference must belong to the paired driver and remain traced until removed.
/// Publish new targets before committing writes, including new/reused frames
/// beyond the captured frontier. Preserve cursor coverage across relocation.
/// Include suspended execution and all managed dependencies, not just operands.
/// Scanning cannot allocate managed objects, call user code or mutate storage.
/// Charge every inspected slot/hole; Pending must progress when at least five
/// units remain. Begin is constant-time. Errors/unwinds leave owned storage valid.
/// Cursor state is finite, pointer/reference/root-free; destruction of either
/// source or cursor cannot dereference managed targets or reenter the heap.
pub unsafe trait RootSource: Send + 'static {
    type Cursor: Send + 'static;

    fn empty() -> Self;

    fn begin_scan(&self) -> Result<Self::Cursor, GcError>;

    fn scan(
        &self,
        cursor: &mut Self::Cursor,
        budget: &mut ScanBudget,
        visitor: &mut EdgeVisitor<'_>,
    ) -> Result<ScanStatus, GcError>;
}

/// Owns a heap and its complete execution root source for their usable lifetime.
///
/// No raw heap/source extraction is available. Every step and full collection
/// includes both this source and host roots. Scopes exclude collection/reentry;
/// end the scope before a host callback, then resume through this same driver.
pub struct RootDriver<R: RootSource> {
    // Drop order closes the heap before source/cursor storage is released.
    heap: Heap,
    source: SourceScan<R>,
}

impl<R: RootSource> RootDriver<R> {
    /// Create an empty source and fresh heap; cannot attach stale external slots.
    pub fn new(config: HeapConfig) -> Result<Self, GcError> {
        Ok(Self {
            heap: Heap::new(config)?,
            source: SourceScan {
                roots: R::empty(),
                cursor: None,
                complete: false,
            },
        })
    }

    /// Lend rooted/provider services, including dynamic layout registration.
    /// This capability cannot collect or expose the underlying Heap; existing
    /// execution roots stay installed while its exclusive loan is active.
    pub fn access(&mut self) -> Result<Access<'_>, GcError> {
        self.heap.access()
    }

    /// Lend disjoint object and root-source capabilities in one closed region.
    /// Return owned data or host roots. Neither Gc nor source loans may escape.
    /// The adapter's unsafe operations enforce root publication and coverage;
    /// ordinary clients should use its safe frame/slot methods.
    ///
    /// ```compile_fail
    /// use kagari_gc::{heap::HeapConfig, root_source::{RootDriver, RootSource}};
    /// fn escape<R: RootSource>(driver: &mut RootDriver<R>) {
    ///     let _borrow = driver.scope(|cx, roots| {
    ///         // Even the unsafe adapter cannot return a borrowed source here.
    ///         unsafe { roots.source_mut(cx) }
    ///     });
    /// }
    /// ```
    ///
    /// Collection cannot overlap a source/heap operation, even through the driver.
    ///
    /// ```compile_fail
    /// use kagari_gc::root_source::{RootDriver, RootSource};
    /// fn overlap<R: RootSource>(driver: &mut RootDriver<R>) {
    ///     driver.scope(|_, _| driver.collect().map(|_| ())).unwrap();
    /// }
    /// ```
    pub fn scope<T>(
        &mut self,
        operation: impl for<'gc> FnOnce(&mut Scope<'gc>, &mut RootAccess<'_, R>) -> Result<T, GcError>,
    ) -> Result<T, GcError> {
        let source = &mut self.source.roots;
        let heap = self.heap.id;
        self.heap
            .scope(|cx| operation(cx, &mut RootAccess { source, heap }))
    }

    /// Perform one bounded step; root-source failures fault the heap before sweep.
    pub fn step(&mut self, budget: WorkBudget) -> Result<StepReport, GcError> {
        self.heap.step_with(budget, &mut self.source)
    }

    /// Service allocation pressure outside access scopes, including this complete
    /// execution source in every automatic step/assist. None means no work due.
    pub fn safepoint(&mut self) -> Result<Option<AutomaticReport>, GcError> {
        self.heap.safepoint_with(&mut self.source)
    }

    /// Finish a carried cycle and a fresh cycle, scanning this source in both.
    pub fn collect(&mut self) -> Result<CollectionReport, GcError> {
        self.heap.collect_with(&mut self.source)
    }

    /// Observe the last closed access/service interval without changing it.
    /// Session attribution belongs to the embedding driver after all loans end.
    pub fn accounting(&self) -> AccountingReport {
        self.heap.accounting()
    }

    /// Observe heap memory; source-owned frame capacity is not managed backing.
    pub fn stats(&self) -> HeapStats {
        self.heap.stats()
    }

    /// Copy source metadata without lending a heap capability or source borrow.
    /// Mutation still requires paired scope admission/publication. The higher-
    /// ranked callback cannot return a reference into the installed source.
    ///
    /// ```compile_fail
    /// use kagari_gc::root_source::{RootDriver, RootSource};
    /// fn escape<R: RootSource>(driver: &RootDriver<R>) {
    ///     let _source = driver.inspect_source(|source| source);
    /// }
    /// ```
    pub fn inspect_source<T>(&self, inspect: impl for<'source> FnOnce(&'source R) -> T) -> T {
        inspect(&self.source.roots)
    }

    /// Inspect collector metadata; excludes this source's frame allocations.
    pub fn metadata_stats(&self) -> MetadataStats {
        self.heap.metadata_stats()
    }

    /// Schedule idle metadata maintenance separately when driving bounded steps.
    /// Does not scan or modify execution roots; see Heap::trim_metadata.
    pub fn trim_metadata(&mut self) -> Result<Option<Duration>, GcError> {
        self.heap.trim_metadata()
    }

    /// Close access permanently; remaining source carriers and host roots are inert.
    pub fn shutdown(&mut self) -> ShutdownReport {
        self.heap.shutdown()
    }
}

/// A short provider capability, not an independently movable root store.
pub struct RootAccess<'a, R: RootSource> {
    source: &'a mut R,
    heap: u64,
}

impl<R: RootSource> RootAccess<'_, R> {
    /// Borrow the authoritative source for a reviewed adapter operation.
    ///
    /// # Safety
    /// Do not replace, extract, clone or expose the source/managed carriers to
    /// ordinary callers. Use only the Scope paired with this RootAccess. Preserve
    /// RootSource's complete tracing, publication, cursor and unwind contracts.
    /// Publish before a no-fail slot commit; plain Rust reference reassignment is
    /// not sufficient. Expose safe operations on your adapter rather than this loan.
    pub unsafe fn source_mut(&mut self, cx: &Scope<'_>) -> Result<&mut R, GcError> {
        if self.heap != cx.access.heap.id {
            return Err(GcError::ForeignHeap);
        }
        cx.access.heap.marking.check()?;
        Ok(self.source)
    }
}

impl<'gc> Scope<'gc> {
    /// Publish an object for installation into the driver's execution roots.
    ///
    /// # Safety
    /// Install the carrier only in the paired RootAccess source, under its
    /// tracing protocol, before this scope ends. Do not expose it as a live handle
    /// or use it in another heap. Failure leaves the destination unchanged.
    pub unsafe fn source_edge<T: 'static>(
        &mut self,
        value: Gc<'gc, T>,
    ) -> Result<Edge<T>, GcError> {
        self.access.heap.marking.check()?;
        let reference = self.publish_admitted(value)?;
        Ok(Edge {
            reference,
            marker: PhantomData,
        })
    }

    /// Import a currently retained source slot into this collection-free scope.
    ///
    /// # Safety
    /// The carrier is an initialized, still-retained slot of the RootAccess paired
    /// with this Scope and has its declared T representation. A detached or foreign
    /// edge is insufficient: resolving an address cannot establish its liveness.
    pub unsafe fn source_get<T: 'static>(&mut self, edge: &Edge<T>) -> Result<Gc<'gc, T>, GcError> {
        self.access.heap.marking.check()?;
        Ok(self.handle(edge.reference))
    }
}

pub(crate) trait RootScan {
    fn begin(&mut self) -> Result<(), GcError>;

    fn scan(
        &mut self,
        budget: &mut ScanBudget,
        visitor: &mut EdgeVisitor<'_>,
    ) -> Result<ScanStatus, GcError>;
}

impl RootScan for () {
    fn begin(&mut self) -> Result<(), GcError> {
        Ok(())
    }

    fn scan(&mut self, _: &mut ScanBudget, _: &mut EdgeVisitor<'_>) -> Result<ScanStatus, GcError> {
        Ok(ScanStatus::Complete)
    }
}

struct SourceScan<R: RootSource> {
    roots: R,
    cursor: Option<R::Cursor>,
    complete: bool,
}

impl<R: RootSource> RootScan for SourceScan<R> {
    fn begin(&mut self) -> Result<(), GcError> {
        self.complete = false;
        self.cursor = Some(self.roots.begin_scan()?);
        Ok(())
    }

    fn scan(
        &mut self,
        budget: &mut ScanBudget,
        visitor: &mut EdgeVisitor<'_>,
    ) -> Result<ScanStatus, GcError> {
        if self.complete {
            return Ok(ScanStatus::Complete);
        }
        let available = budget.remaining();
        let before = budget.spent();
        let status = self.roots.scan(
            self.cursor.as_mut().expect("source begun before scanning"),
            budget,
            visitor,
        )?;
        let spent = budget.spent() - before;
        if spent > available {
            return Err(GcError::Provider("root source exceeded allowance"));
        }
        if status == ScanStatus::Pending && spent == 0 && available >= 5 {
            return Err(GcError::Provider("root source made no progress"));
        }
        self.complete = status == ScanStatus::Complete;
        Ok(status)
    }
}
