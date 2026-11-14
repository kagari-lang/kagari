//! Read-only edge enumeration reuses provider dispatch without marking or roots.
use crate::{
    access::Scope,
    descriptor::ScanWorkspace,
    error::GcError,
    object::{AnyObject, Gc},
    scan::{EdgeVisitor, ScanBudget, ScanKind, ScanStatus, VisitTarget},
};
use std::ops::ControlFlow;

/// Caller-owned scanner scratch, reusable across objects and closed inspections.
/// It retains capacity and pointer-free Copy state, never an object or root. The
/// capacity is external traversal scratch, separate from Heap's marking workspace.
#[derive(Debug, Default)]
pub struct EdgeInspector {
    workspace: ScanWorkspace,
}

impl EdgeInspector {
    /// Current owned scratch bytes, excluding Heap's collection workspace.
    pub fn capacity_bytes(&self) -> usize {
        self.workspace.bytes()
    }
}

impl<'gc> Scope<'gc> {
    /// Enumerate an admitted object's direct physical edges without marking them.
    /// Reuses its ordinary provider scanner, including dynamic/erased storage.
    /// Duplicate fields are reported separately; graph policy and cycle detection
    /// belong to the caller. No implicit traversal or Host root is introduced.
    ///
    /// Only shared Scope access is available to `visit` and `checkpoint`; they
    /// cannot mutate or collect this heap. Returned handles keep this scope's
    /// brand. They may be queued for later reads inside the same closed interval.
    /// Checkpoints precede each 1,024-unit scanner turn, including turns inspecting
    /// holes without edges. Indivisible providers may exceed that allowance,
    /// as during collection.
    /// Either callback can stop with an owned result. Provider/storage failures
    /// return GcError; callback panics propagate as for Scope::read.
    ///
    /// ```
    /// use kagari_gc::{access::inspection::EdgeInspector, error::GcError, heap::Heap};
    /// use std::ops::ControlFlow;
    /// let mut heap = Heap::default();
    /// heap.scope(|cx| {
    ///     let leaf = cx.alloc_leaf(42u64)?;
    ///     let mut scratch = EdgeInspector::default();
    ///     let status = cx.inspect_edges(leaf, &mut scratch,
    ///         |_| unreachable!("edge-free leaf"), || ControlFlow::<()>::Continue(()))?;
    ///     assert_eq!(status, ControlFlow::Continue(()));
    ///     Ok::<_, GcError>(())
    /// }).unwrap();
    /// ```
    /// A visit cannot mutate the same heap through its enclosing scope:
    /// ```compile_fail
    /// use kagari_gc::{access::inspection::EdgeInspector, heap::Heap};
    /// use std::ops::ControlFlow;
    /// let mut heap = Heap::default();
    /// heap.scope(|cx| {
    ///     let value = cx.alloc_leaf(42u64)?;
    ///     cx.inspect_edges(value, &mut EdgeInspector::default(), |_| {
    ///         cx.alloc_leaf(1u64).unwrap();
    ///         ControlFlow::<()>::Continue(())
    ///     }, || ControlFlow::Continue(()))?;
    ///     Ok(())
    /// }).unwrap();
    /// ```
    pub fn inspect_edges<T: 'static, B>(
        &self,
        object: Gc<'gc, T>,
        scratch: &mut EdgeInspector,
        mut visit: impl FnMut(Gc<'gc, AnyObject>) -> ControlFlow<B>,
        mut checkpoint: impl FnMut() -> ControlFlow<B>,
    ) -> Result<ControlFlow<B>, GcError> {
        let reference = self.check(object)?;
        let object = self.access.heap.physical.resolve(reference)?;
        if let ControlFlow::Break(value) = checkpoint() {
            return Ok(ControlFlow::Break(value));
        }
        if object.descriptor.kind == ScanKind::EdgeFree {
            return Ok(ControlFlow::Continue(()));
        }
        scratch.workspace.reserve(object.descriptor.state_layout)?;
        // SAFETY: admitted initialized object, exact descriptor/aligned workspace.
        // The private state is Copy and this shared scope excludes mutation/GC.
        unsafe {
            object
                .descriptor
                .begin(object.pointer, scratch.workspace.pointer())
        };
        loop {
            let mut stopped = None;
            let mut consume = |reference| {
                if stopped.is_some() {
                    return false;
                }
                match visit(self.handle(reference)) {
                    ControlFlow::Continue(()) => true,
                    ControlFlow::Break(value) => {
                        stopped = Some(value);
                        false
                    }
                }
            };
            let mut budget = ScanBudget::new(1024);
            let mut visitor = EdgeVisitor {
                physical: &self.access.heap.physical,
                target: VisitTarget::Inspect {
                    visit: &mut consume,
                    indivisible: object.descriptor.kind == ScanKind::Indivisible,
                },
            };
            // SAFETY: the original object remains live and immutable. Visitor
            // admission validates each initialized edge before branding its target.
            let status = unsafe {
                object.descriptor.scan(
                    object.pointer,
                    scratch.workspace.pointer(),
                    &mut budget,
                    &mut visitor,
                )?
            };
            if let Some(value) = stopped {
                return Ok(ControlFlow::Break(value));
            }
            if object.descriptor.kind != ScanKind::Indivisible && budget.spent() > 1024 {
                return Err(GcError::Provider("inspection scan exceeded allowance"));
            }
            if matches!(object.descriptor.kind, ScanKind::Bounded { max_work }
                if status != ScanStatus::Complete || budget.spent() > max_work * 5)
            {
                return Err(GcError::Provider(
                    "inspection bounded scan violated contract",
                ));
            }
            if status == ScanStatus::Complete {
                return Ok(ControlFlow::Continue(()));
            }
            if budget.spent() == 0 {
                return Err(GcError::Provider("inspection scan made no progress"));
            }
            if let ControlFlow::Break(value) = checkpoint() {
                return Ok(ControlFlow::Break(value));
            }
        }
    }
}
