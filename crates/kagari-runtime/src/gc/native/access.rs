//! Short payload loans release the directory before validation/root services run.
use super::NativeBorrow;
#[cfg(feature = "execution-diagnostics")]
use crate::diagnostics::{self, Event};
use crate::{
    Runtime,
    error::RuntimeError,
    gc::{GcHeap, HeapObjectId, storage::HeapObject},
    native::{
        binding::NativeResult,
        storage::{
            NativeObject, NativeStorage,
            access::{EditPlan, StorageAccess},
        },
        views::{NativeOperationGuard, NativeOperationKind, Recovery},
    },
    resource::HeapChange,
};
use std::{
    any::{Any, TypeId},
    panic::{AssertUnwindSafe, catch_unwind},
    ptr::{self, NonNull},
};

struct PayloadLoan<'heap> {
    heap: &'heap GcHeap,
    header: NonNull<NativeObject>,
    payload: NonNull<dyn Any + Send>,
    storage: Option<NativeStorage>,
    next_revision: Option<u64>,
    revision: u64,
    max_units: usize,
    cleanup_bytes: Option<usize>,
    change: Option<HeapChange<'heap>>,
    _borrow: NativeBorrow<'heap>,
}

impl PayloadLoan<'_> {
    fn prepare_edit(
        &mut self,
        plan: EditPlan,
        cleanup: bool,
        protected_revision: bool,
    ) -> NativeResult<()> {
        // SAFETY: the active loan pins the nonmoving header. Preparation only
        // reads it; all scoped validation views have ended before this call.
        let object = unsafe { self.header.as_ref() };
        let next_revision = if plan.structural {
            Some(if protected_revision {
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
        let max_units = plan.max_logical_units;
        if max_units < object.charge.logical_units {
            return Err(RuntimeError::module_validation(
                "native edit maximum excludes its initial state",
            ));
        }
        let change = if cleanup {
            self.heap
                .resources
                .prepare_cleanup_heap_change(object.charge, max_units)?
        } else {
            self.heap
                .resources
                .prepare_heap_change(object.charge, max_units)?
        };
        self.next_revision = next_revision;
        self.max_units = max_units;
        self.cleanup_bytes = cleanup.then_some(object.charge.retained_bytes);
        self.storage = Some(object.storage.clone());
        self.change = Some(change);
        Ok(())
    }
}

impl Drop for PayloadLoan<'_> {
    fn drop(&mut self) {
        // SAFETY: the nonmoving allocation is pinned by _borrow, and the header's
        // loan flag rejects every competing payload view until restoration below.
        let payload = unsafe { self.payload.as_ref() };
        let charge = self.change.as_ref().map(|_| {
            catch_unwind(AssertUnwindSafe(|| {
                self.storage
                    .as_ref()
                    .expect("edit descriptor")
                    .installed_charge(payload)
            }))
        });
        // The same nonmoving allocation holds header and payload. The active
        // NativeBorrow excludes detach/collection/reentry, and all internal header
        // validation views have ended before this loan's Drop. Restore through the
        // pinned owner instead of resolving its identity in the directory again.
        let object = unsafe { self.header.as_mut() };
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
        expected: Option<TypeId>,
        protected_revision: bool,
    ) -> NativeResult<PayloadLoan<'_>> {
        self.ensure_no_native_borrow()?;
        if !cleanup {
            self.ensure_execution_allowed()?;
        }
        let mut objects = self.objects_mut()?;
        let object = self
            .native_mut(&mut objects, id)
            .ok_or_else(|| RuntimeError::module_validation("invalid native loan receiver"))?;
        if expected.is_some_and(|expected| expected != object.payload_type()) {
            return Err(RuntimeError::module_validation("native loan payload type"));
        }
        let revision = object.revision;
        let payload = object.loan_payload()?;
        self.native_borrows.set(self.native_borrows.get() + 1);
        let max_units = object.charge.logical_units;
        let header = NonNull::from(object);
        let mut loan = PayloadLoan {
            heap: self,
            header,
            payload,
            storage: None,
            next_revision: None,
            revision,
            max_units,
            cleanup_bytes: None,
            change: None,
            _borrow: NativeBorrow(&self.native_borrows),
        };
        if let Some(plan) = plan {
            loan.prepare_edit(plan, cleanup, protected_revision)?;
        }
        Ok(loan)
    }

    pub(crate) fn read_native<P: Send + 'static, R>(
        &self,
        runtime: &Runtime,
        id: HeapObjectId,
        operation: Option<&NativeOperationGuard<'_>>,
        read: impl for<'a> FnOnce(&'a P, &mut StorageAccess<'a>) -> NativeResult<R>,
    ) -> NativeResult<R> {
        #[cfg(feature = "execution-diagnostics")]
        diagnostics::record(Event::NativeRead);
        self.check_native_operation(id, operation, false)?;
        let mut loan = self.loan_native(id, None, false, Some(TypeId::of::<P>()), false)?;
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
        #[cfg(feature = "execution-diagnostics")]
        diagnostics::record(Event::NativeEdit);
        self.check_native_operation(id, operation, true)?;
        let mut loan = self.loan_native(
            id,
            Some(plan),
            false,
            Some(TypeId::of::<P>()),
            operation.is_some(),
        )?;
        let next = loan.next_revision;
        // SAFETY: exclusive loan flag and NativeBorrow exclude all competing T access.
        let payload = unsafe { loan.payload.as_mut() }
            .downcast_mut::<P>()
            .expect("checked payload type");
        let mut access = StorageAccess::new(runtime, &mut loan.revision, next, false);
        edit(payload, &mut access)
    }

    pub(crate) fn edit_native_prepared<P: Send + 'static, S, R>(
        &self,
        runtime: &Runtime,
        id: HeapObjectId,
        prepare: impl for<'a> FnOnce(
            &'a P,
            &mut StorageAccess<'a>,
        ) -> NativeResult<Option<(EditPlan, S)>>,
        edit: impl for<'a> FnOnce(&'a mut P, &mut StorageAccess<'a>, S) -> NativeResult<R>,
    ) -> NativeResult<Option<R>> {
        #[cfg(feature = "execution-diagnostics")]
        diagnostics::record(Event::NativeRead);
        self.check_native_operation(id, None, true)?;
        let mut loan = self.loan_native(id, None, false, Some(TypeId::of::<P>()), false)?;
        // SAFETY: Preparation has a shared, nonescaping view. Its checked facts
        // remain valid until commit because this same loan excludes every writer.
        let payload = unsafe { loan.payload.as_ref() }
            .downcast_ref::<P>()
            .expect("checked payload type");
        let mut access = StorageAccess::new(runtime, &mut loan.revision, None, false);
        let Some((plan, prepared)) = prepare(payload, &mut access)? else {
            return Ok(None);
        };
        loan.prepare_edit(plan, false, false)?;
        #[cfg(feature = "execution-diagnostics")]
        diagnostics::record(Event::NativeEdit);
        // SAFETY: Preparation's shared view ended before this exclusive view.
        let payload = unsafe { loan.payload.as_mut() }
            .downcast_mut::<P>()
            .expect("checked payload type");
        let mut access = StorageAccess::new(runtime, &mut loan.revision, loan.next_revision, false);
        edit(payload, &mut access, prepared).map(Some)
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
                None,
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
