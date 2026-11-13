//! Cold owned snapshots account every source buffer, including parked executions.
use crate::{
    diagnostics::{self, Event, storage::BufferUsage},
    execution::roots::ExecutionRoots,
};
use std::mem::size_of;

pub(super) fn reservation<T>(before: usize, values: &Vec<T>) {
    if values.capacity() != before {
        diagnostics::record(Event::BankReservation);
        diagnostics::record_amount(
            Event::BankBytesReallocated,
            (before * size_of::<T>()) as u64,
        );
    }
}

pub(super) fn initialized<T>(count: usize) {
    diagnostics::record_amount(Event::BankBytesInitialized, (count * size_of::<T>()) as u64);
}

pub(super) fn examined() {
    diagnostics::record(Event::ExecutionRootSlotExamined);
}

impl ExecutionRoots {
    pub(crate) fn visit_usage(&self, visit: &mut impl FnMut(BufferUsage)) {
        visit(BufferUsage::new(
            "execution source directory",
            &self.entries,
        ));
        for storage in self
            .entries
            .iter()
            .filter_map(|entry| entry.storage.as_ref())
        {
            for (name, bank) in [
                "managed values",
                "local temporaries",
                "queued factory values",
                "pending values",
            ]
            .into_iter()
            .zip(&storage.banks)
            {
                visit(BufferUsage::new(name, bank));
            }
            for buffer in [
                BufferUsage::new("scalar values", &storage.scalars),
                BufferUsage::new("scalar initialization", &storage.initialized),
                BufferUsage::new("Native source records", &storage.native_calls),
                BufferUsage::new("typed dependencies", &storage.dependencies),
                BufferUsage::new("frame return records", &storage.returns),
            ] {
                visit(buffer);
            }
        }
    }
}
