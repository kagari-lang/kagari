//! Short payload loans release the directory before validation/root services run.
use super::NativeBorrow;
use crate::{
    Runtime,
    error::RuntimeError,
    gc::{GcHeap, HeapObjectId, storage::HeapObject},
    native::{
        binding::NativeResult,
        storage::{
            NativeStorage,
            access::{EditPlan, StorageAccess},
        },
        views::{NativeOperationGuard, NativeOperationKind, Recovery},
    },
    resource::HeapChange,
};
use std::{
    any::Any,
    panic::{AssertUnwindSafe, catch_unwind},
    ptr::{self, NonNull},
};

struct PayloadLoan<'heap> {
    heap: &'heap GcHeap,
    id: HeapObjectId,
    payload: NonNull<dyn Any + Send>,
    storage: NativeStorage,
    revision: u64,
    max_units: usize,
    cleanup_bytes: Option<usize>,
    change: Option<HeapChange<'heap>>,
    _borrow: NativeBorrow<'heap>,
}

impl Drop for PayloadLoan<'_> {
    fn drop(&mut self) {
        // SAFETY: the nonmoving allocation is pinned by _borrow, and the header's
        // loan flag rejects every competing payload view until restoration below.
        let payload = unsafe { self.payload.as_ref() };
        let charge = self
            .change
            .as_ref()
            .map(|_| catch_unwind(AssertUnwindSafe(|| self.storage.installed_charge(payload))));
        // Entry acquired exclusive directory access before lending the payload.
        // While _borrow is alive, public directory-backed views are rejected;
        // internal validation views are scoped and cannot escape into the callback.
        let mut objects = self.heap.objects.borrow_mut();
        let object = self
            .heap
            .native_mut(&mut objects, self.id)
            .expect("loaned identity is pinned");
        object.restore_payload();
        object.revision = self.revision;
        if let Some(charge) = charge {
            match charge {
                Ok(charge)
                    if charge.logical_units <= self.max_units
                        && self
                            .cleanup_bytes
                            .is_none_or(|maximum| charge.retained_bytes <= maximum) =>
                {
                    object.charge = charge;
                    self.change.take().expect("edit accounting").commit(charge);
                }
                _ => {
                    self.heap
                        .resources
                        .quarantine("native provider violated installed charge contract");
                }
            }
        }
    }
}

impl GcHeap {
    pub(crate) fn check_native_operation(
        &self,
        id: HeapObjectId,
        operation: Option<&NativeOperationGuard<'_>>,
        write: bool,
    ) -> NativeResult<()> {
        if let Some(guard) = operation {
            if !ptr::eq(guard.heap, self) || guard.id != id || !self.native_operations.is_active(id)
            {
                return Err(RuntimeError::module_validation(
                    "foreign native operation guard",
                ));
            }
            if write
                && self.native_operations.operation(id)
                    == Some(NativeOperationKind::ReadOnlyCallbacks)
            {
                return Err(RuntimeError::module_validation(
                    "read-only operation cannot authorize mutation",
                ));
            }
            return Ok(());
        }
        if let Some(kind) = self.native_operations.operation(id)
            && (write || kind == NativeOperationKind::ExclusiveSlots)
        {
            return Err(RuntimeError::module_validation(
                "native operation protects receiver slots",
            ));
        }
        Ok(())
    }

    fn loan_native(
        &self,
        id: HeapObjectId,
        plan: Option<EditPlan>,
        cleanup: bool,
    ) -> NativeResult<PayloadLoan<'_>> {
        self.ensure_no_native_borrow()?;
        if !cleanup {
            self.ensure_execution_allowed()?;
        }
        let mut objects = self.objects_mut()?;
        let object = self
            .native_mut(&mut objects, id)
            .ok_or_else(|| RuntimeError::module_validation("invalid native loan receiver"))?;
        let max_units = plan.map_or(object.charge.logical_units, |plan| plan.max_logical_units);
        if max_units < object.charge.logical_units {
            return Err(RuntimeError::module_validation(
                "native edit maximum excludes its initial state",
            ));
        }
        let change = if plan.is_some() {
            Some(if cleanup {
                self.resources
                    .prepare_cleanup_heap_change(object.charge, max_units)?
            } else {
                self.resources
                    .prepare_heap_change(object.charge, max_units)?
            })
        } else {
            None
        };
        let cleanup_bytes = cleanup.then_some(object.charge.retained_bytes);
        let storage = object.storage.clone();
        let revision = object.revision;
        let payload = object.loan_payload()?;
        self.native_borrows.set(self.native_borrows.get() + 1);
        Ok(PayloadLoan {
            heap: self,
            id,
            payload,
            storage,
            revision,
            max_units,
            cleanup_bytes,
            change,
            _borrow: NativeBorrow(&self.native_borrows),
        })
    }

    pub(crate) fn read_native<P: Send + 'static, R>(
        &self,
        runtime: &Runtime,
        id: HeapObjectId,
        operation: Option<&NativeOperationGuard<'_>>,
        read: impl for<'a> FnOnce(&'a P, &mut StorageAccess<'a>) -> NativeResult<R>,
    ) -> NativeResult<R> {
        self.check_native_operation(id, operation, false)?;
        self.check_native_payload::<P>(id, false)?;
        let mut loan = self.loan_native(id, None, false)?;
        // SAFETY: the active loan owns access to initialized, nonmoving T. The
        // callback cannot retain this reference or collect/reenter through an alias.
        let payload = unsafe { loan.payload.as_ref() }
            .downcast_ref::<P>()
            .expect("checked payload type");
        let mut access = StorageAccess::new(runtime, &mut loan.revision, None, false);
        read(payload, &mut access)
    }

    pub(crate) fn edit_native<P: Send + 'static, R>(
        &self,
        runtime: &Runtime,
        id: HeapObjectId,
        operation: Option<&NativeOperationGuard<'_>>,
        plan: EditPlan,
        edit: impl for<'a> FnOnce(&'a mut P, &mut StorageAccess<'a>) -> NativeResult<R>,
    ) -> NativeResult<R> {
        self.check_native_operation(id, operation, true)?;
        self.ensure_key_mutable(id)?;
        self.check_native_payload::<P>(id, false)?;
        let next = if plan.structural {
            let objects = self.objects.borrow();
            let object = self
                .native_ref(&objects, id)
                .expect("checked native identity");
            // An exclusive operation owns one invalidate-on-exit transition;
            // its short edits do not consume additional revisions.
            Some(if operation.is_some() {
                object.revision
            } else {
                object
                    .revision
                    .checked_add(1)
                    .ok_or_else(|| RuntimeError::module_validation("native revision exhausted"))?
            })
        } else {
            None
        };
        let mut loan = self.loan_native(id, Some(plan), false)?;
        // SAFETY: exclusive loan flag and NativeBorrow exclude all competing T access.
        let payload = unsafe { loan.payload.as_mut() }
            .downcast_mut::<P>()
            .expect("checked payload type");
        let mut access = StorageAccess::new(runtime, &mut loan.revision, next, false);
        edit(payload, &mut access)
    }

    pub(crate) fn prepare_native_recovery(&self, id: HeapObjectId) -> NativeResult<()> {
        let objects = self.objects.borrow();
        let Some(HeapObject::Native(object)) = self.readable_object(&objects, id) else {
            return Err(RuntimeError::module_validation(
                "invalid native recovery receiver",
            ));
        };
        object
            .revision
            .checked_add(1)
            .ok_or_else(|| RuntimeError::module_validation("native revision exhausted"))?;
        // Preflight arithmetic now; do not retain the resource counter borrow across callbacks.
        self.resources
            .prepare_cleanup_heap_change(object.charge, object.charge.logical_units)?;
        Ok(())
    }

    pub(crate) fn finish_native_operation(&self, id: HeapObjectId) {
        let Ok(mut objects) = self.objects.try_borrow_mut() else {
            self.resources
                .quarantine("native operation released during a storage loan");
            return;
        };
        if let Some(object) = self.native_mut(&mut objects, id)
            && let Some(next) = object.revision.checked_add(1)
        {
            object.revision = next;
        } else {
            self.resources
                .quarantine("native operation lost its reserved revision");
        }
    }

    pub(crate) fn recover_native(&self, runtime: &Runtime, id: HeapObjectId, recovery: &Recovery) {
        let result = catch_unwind(AssertUnwindSafe(|| -> NativeResult<()> {
            let (units, revision) = {
                let objects = self.objects.borrow();
                let object = self
                    .native_ref(&objects, id)
                    .expect("operation retains receiver");
                (object.charge.logical_units, object.revision)
            };
            let next = revision.checked_add(1).ok_or_else(|| {
                RuntimeError::module_validation("native recovery revision exhausted")
            })?;
            let mut loan = self.loan_native(
                id,
                Some(EditPlan {
                    max_logical_units: units,
                    structural: true,
                }),
                true,
            )?;
            loan.revision = next;
            // SAFETY: recovery uses the same exclusive loan, including on unwind.
            let payload = unsafe { loan.payload.as_mut() };
            let mut access = StorageAccess::new(runtime, &mut loan.revision, Some(next), true);
            recovery(payload, &mut access);
            Ok(())
        }));
        if !matches!(result, Ok(Ok(()))) {
            self.resources
                .quarantine("native operation recovery failed");
        }
    }
}
