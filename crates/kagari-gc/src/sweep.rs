//! Bounded detached owners share dispatch/backing pins until disposal completes.

use crate::{
    allocation::{PhysicalHeap, Reservation, page::Backing},
    descriptor::Descriptor,
    error::GcError,
    heap::accounting::MemoryLedger,
    object::{ObjectCharge, ObjectRef},
    root::RootEntry,
};
use std::{
    array,
    panic::{AssertUnwindSafe, catch_unwind},
    ptr::NonNull,
    sync::Arc,
};

pub(crate) const RETIREMENT_CAP: usize = 32;

struct Pin {
    descriptor: Arc<Descriptor>,
    _backing: Arc<Backing>,
    page: u32,
}

struct Retired {
    reservation: Reservation,
    descriptor: NonNull<Descriptor>,
    pointer: NonNull<u8>,
    initialized: bool,
}

enum Owner {
    Object(Retired),
    Roots(RootEntry),
}

/// Owners never escape this batch. Field order ensures all remaining objects are
/// destroyed before pins are released, including early errors and unwinding.
pub(crate) struct RetirementBatch {
    owners: [Option<Owner>; RETIREMENT_CAP],
    pins: [Option<Pin>; RETIREMENT_CAP],
    len: usize,
    pin_count: usize,
}

impl RetirementBatch {
    pub(crate) fn new() -> Self {
        Self {
            owners: array::from_fn(|_| None),
            pins: array::from_fn(|_| None),
            len: 0,
            pin_count: 0,
        }
    }

    pub(crate) fn len(&self) -> usize {
        self.len
    }

    pub(crate) fn push_root(&mut self, root: RootEntry) {
        assert!(self.len < RETIREMENT_CAP);
        self.owners[self.len] = Some(Owner::Roots(root));
        self.len += 1;
    }

    /// Detach from a known physical cursor, returning the admitted provider charge.
    /// Adjacent objects of the same page and descriptor share one pair of pins.
    pub(crate) fn detach(
        &mut self,
        physical: &mut PhysicalHeap,
        page: u32,
        slot: usize,
        ledger: Option<&MemoryLedger>,
    ) -> Result<ObjectCharge, GcError> {
        assert!(self.len < RETIREMENT_CAP);
        let object = physical.object_at(page, slot)?;
        let charge = if ledger.is_some() {
            // SAFETY: initialized occupied object and its exact registered hook.
            unsafe { object.descriptor.charge(object.pointer) }
        } else {
            ObjectCharge::default()
        };
        if ledger.is_some_and(|ledger| {
            charge.bytes > ledger.indirect || charge.units as u128 > ledger.units
        }) {
            return Err(GcError::Storage("retirement charge exceeds ledger"));
        }
        let previous = self
            .pin_count
            .checked_sub(1)
            .and_then(|i| self.pins[i].as_ref());
        if !previous
            .is_some_and(|pin| pin.page == page && Arc::ptr_eq(&pin.descriptor, object.descriptor))
        {
            self.pins[self.pin_count] = Some(Pin {
                descriptor: object.descriptor.clone(),
                _backing: physical.page(page)?.backing().clone(),
                page,
            });
            self.pin_count += 1;
        }
        let owner = Retired {
            reservation: Reservation {
                reference: ObjectRef(physical.page(page)?.pointer(slot)),
                page,
                slot,
            },
            descriptor: NonNull::from(object.descriptor.as_ref()),
            pointer: object.pointer,
            initialized: true,
        };
        // All fallible preparation precedes consuming initialized ownership.
        physical.page_mut(page)?.detach(slot);
        physical.objects -= 1;
        self.owners[self.len] = Some(Owner::Object(owner));
        self.len += 1;
        Ok(charge)
    }

    /// Run destructors outside storage loans, then release descriptor/slot credits.
    pub(crate) fn finish(
        mut self,
        physical: &mut PhysicalHeap,
        mut on_panic: impl FnMut(),
    ) -> Result<usize, GcError> {
        let mut panics = 0;
        for owner in &mut self.owners[..self.len] {
            match owner.take().expect("detached batch owner") {
                Owner::Object(mut retired) => {
                    if retired.dispose() {
                        panics += 1;
                        on_panic();
                    }
                    // SAFETY: batch pins outlive every retired owner and this loop.
                    let descriptor = unsafe { retired.descriptor.as_ref() };
                    physical.descriptors.release(descriptor.id);
                    physical.release_slot(retired.reservation.page, retired.reservation.slot)?;
                }
                Owner::Roots(root) => drop(root),
            }
        }
        Ok(panics)
    }
}

impl Retired {
    fn dispose(&mut self) -> bool {
        if !self.initialized {
            return false;
        }
        self.initialized = false;
        // SAFETY: consume initialized ownership before Drop; the containing batch
        // pins dispatch and backing through this call, including destructor unwind.
        catch_unwind(AssertUnwindSafe(|| unsafe {
            self.descriptor.as_ref().destroy(self.pointer)
        }))
        .is_err()
    }
}

impl Drop for Retired {
    fn drop(&mut self) {
        self.dispose();
    }
}
