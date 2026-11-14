//! One mutable loan/finalization kernel for Root callbacks and scoped handles.

use crate::{
    access::{Access, ChargeState, Mutation, ObjectMut, Retention},
    error::GcError,
    heap::{Heap, Phase},
    object::{GcObject, ObjectRef},
};
use std::{
    marker::PhantomData,
    panic::{AssertUnwindSafe, catch_unwind},
    sync::Arc,
    thread,
};

impl Access<'_> {
    // Callers hold a live Root or scoped handle and have checked heap state in
    // this operation: Access::with_object_mut, Scope::with_object_mut, or
    // Scope::borrow_object_mut. Their admission cannot yield or change state
    // before entering this kernel. Provider charge/callbacks run only afterward.
    pub(super) fn borrow_object_mut<T: GcObject>(
        &mut self,
        reference: ObjectRef,
    ) -> Result<ObjectMut<'_, T>, GcError> {
        debug_assert!(
            self.heap.marking.check().is_ok(),
            "mutable loan requires lifecycle admission"
        );
        let object = self.heap.physical.resolve(reference)?;
        if !object.descriptor.matches::<T>() {
            return Err(GcError::Representation);
        }
        if let Some(active) = &self.heap.active
            && active.owner == reference
            && !Arc::ptr_eq(&active.descriptor, object.descriptor)
        {
            return Err(GcError::Representation);
        }
        let pointer = object.pointer;
        let old_charge = match catch_unwind(AssertUnwindSafe(|| unsafe {
            object.descriptor.charge(pointer)
        })) {
            Ok(charge) => charge,
            Err(_) => {
                self.heap.marking.phase = Phase::Faulted;
                return Err(GcError::Provider("charge panicked"));
            }
        };
        let Heap {
            physical,
            roots,
            marking,
            active,
            workspace,
            ledger,
            config,
            ..
        } = self.heap;
        // An owned guard may be safely forgotten. Install the conservative scan
        // fallback before lending payload bytes, not only in its destructor.
        let scan_reusable = active
            .as_ref()
            .is_some_and(|active| active.owner == reference && !active.restart);
        if let Some(active) = active.as_mut()
            && active.owner == reference
        {
            active.restart = true;
        }
        let mutation = Mutation {
            retention: Retention {
                physical,
                roots,
                marking,
                max_roots: config.max_roots,
            },
            owner: reference,
            active,
            workspace,
            ledger,
            old_charge,
            admitted_bytes: old_charge.bytes,
            max_indirect: config.max_indirect_bytes,
            continuation: false,
            scan_reusable,
            force_restart: false,
            marker: PhantomData,
        };
        // SAFETY: live source, exact type and exclusive Access. Services expose no
        // second payload loan; the returned guard retains exclusion until finalized.
        let object = unsafe { &mut *pointer.cast::<T>().as_ptr() };
        Ok(ObjectMut {
            object,
            mutation,
            finished: false,
            charge_state: ChargeState::Dirty,
            panicking_on_entry: thread::panicking(),
        })
    }
}

impl<'op, T: GcObject> ObjectMut<'op, T> {
    // Reborrow services for the callback's shorter lifetime instead of extending
    // an exclusive payload borrow to the guard's entire lifetime. Copy admission/
    // continuation results back before returning either success or a caught panic.
    pub(super) fn apply<R>(
        &mut self,
        operation: impl for<'scope> FnOnce(&'scope mut T, &mut Mutation<'scope, T>) -> R,
    ) -> thread::Result<R> {
        self.charge_state = ChargeState::Dirty;
        let (result, admitted, continuation, force_restart) = {
            let parent = &mut self.mutation;
            let mut mutation = Mutation {
                retention: Retention {
                    physical: parent.retention.physical,
                    roots: parent.retention.roots,
                    marking: parent.retention.marking,
                    max_roots: parent.retention.max_roots,
                },
                owner: parent.owner,
                active: parent.active,
                workspace: parent.workspace,
                ledger: parent.ledger,
                old_charge: parent.old_charge,
                admitted_bytes: parent.admitted_bytes,
                max_indirect: parent.max_indirect,
                continuation: false,
                scan_reusable: parent.scan_reusable,
                force_restart: parent.force_restart,
                marker: PhantomData,
            };
            let result = catch_unwind(AssertUnwindSafe(|| operation(self.object, &mut mutation)));
            (
                result,
                mutation.admitted_bytes,
                mutation.continuation,
                mutation.force_restart,
            )
        };
        self.mutation.admitted_bytes = admitted;
        self.mutation.continuation = continuation;
        self.mutation.force_restart = force_restart;
        result
    }

    /// Read this exclusively borrowed object; the reference cannot outlive the guard.
    pub fn object(&self) -> &T {
        self.object
    }

    /// Split the object's unique loan from its publication/accounting services.
    ///
    /// # Safety
    /// Honor with_object_mut's complete provider contract. Do not expose general
    /// mutable T to safe callers, bypass barriers/charge admission, or let owned
    /// edges escape. A safe wrapper may project a scalar field whose mutation
    /// cannot change traced edges, scan coordinates or accounted charge.
    /// Safe wrappers must tolerate mem::forget: never rely on guard destruction
    /// alone to commit charge changes or establish valid traced storage. Call
    /// commit_accounting or finish after changing charge, before exposing a safe
    /// result that may be forgotten. Reference edits still require publication.
    pub unsafe fn parts(&mut self) -> (&mut T, &mut Mutation<'op, T>) {
        self.charge_state = ChargeState::Dirty;
        (self.object, &mut self.mutation)
    }

    /// Commit this provider's admitted bytes and logical units while retaining
    /// the exclusive loan. The scanner fallback remains installed even if this
    /// guard is subsequently forgotten. A later parts/apply edit needs a new
    /// checkpoint; finish/Drop performs it automatically when needed.
    /// Errors fault the heap, as with finalization, and are not retried by Drop.
    /// This is cleanup of an admitted loan, not new access or growth admission.
    pub fn commit_accounting(&mut self) -> Result<(), GcError> {
        match self.charge_state {
            ChargeState::Committed => return Ok(()),
            ChargeState::Failed => return Err(GcError::Faulted),
            ChargeState::Dirty => {}
        }
        let result = catch_unwind(AssertUnwindSafe(|| {
            let charge = self.object.charge();
            if charge.bytes > self.mutation.admitted_bytes {
                return Err(GcError::Provider("capacity growth was not admitted"));
            }
            let indirect = self.mutation.ledger.check(
                self.mutation.old_charge.bytes,
                charge.bytes,
                self.mutation.max_indirect,
            )?;
            let units = self
                .mutation
                .ledger
                .check_units(self.mutation.old_charge.units, charge.units)?;
            self.mutation.ledger.indirect = indirect;
            self.mutation.ledger.units = units;
            self.mutation
                .ledger
                .grew(charge.bytes.saturating_sub(self.mutation.old_charge.bytes));
            self.mutation
                .ledger
                .observe(self.mutation.retention.physical);
            self.mutation.old_charge = charge;
            self.charge_state = ChargeState::Committed;
            Ok(())
        }))
        .unwrap_or(Err(GcError::Provider("mutation charge panicked")));
        if result.is_err() {
            self.charge_state = ChargeState::Failed;
            self.mutation.retention.marking.phase = Phase::Faulted;
        }
        result
    }

    /// Finish explicitly to observe accounting/contract failures. Drop performs
    /// the same cleanup when omitted, faulting the heap on a finalization failure.
    pub fn finish(mut self) -> Result<(), GcError> {
        self.finalize()
    }

    fn finalize(&mut self) -> Result<(), GcError> {
        if self.finished {
            return Ok(());
        }
        self.finished = true;
        if !self.mutation.continuation || (thread::panicking() && !self.panicking_on_entry) {
            self.mutation.invalidate();
        }
        if let Some(active) = self.mutation.active.as_mut()
            && active.owner == self.mutation.owner
        {
            // Clear only this guard's fallback, never an earlier restart or an
            // explicit failure/unwind invalidation.
            active.restart = !self.mutation.scan_reusable || self.mutation.force_restart;
        }
        self.commit_accounting()
    }
}

impl<T: GcObject> Drop for ObjectMut<'_, T> {
    fn drop(&mut self) {
        let _ = self.finalize();
    }
}
