//! Cold owned snapshots account every source buffer, including parked executions.
use crate::{
    diagnostics::{self, Event, storage::BufferUsage},
    execution::roots::{ExecutionRoots, ExecutionStorage},
    execution::space::ExecutionSpace,
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
            storage.visit_usage(false, visit);
        }
        self.reusable.visit_usage(true, visit);
    }
}

impl ExecutionStorage {
    fn visit_usage(&self, reusable: bool, visit: &mut impl FnMut(BufferUsage)) {
        if self.space.is_some() {
            // Current Rust Arc control has strong/weak usize counters. This
            // ledger is counted once here, not again through SessionState.
            visit(BufferUsage {
                name: if reusable {
                    "reusable execution space ledger"
                } else {
                    "execution space ledger"
                },
                entries: 1,
                capacity: 1,
                entry_bytes: size_of::<ExecutionSpace>() + 2 * size_of::<usize>(),
            });
        }
        let names = if reusable {
            [
                "reusable managed values",
                "reusable local temporaries",
                "reusable queued factory values",
                "reusable pending values",
                "reusable scalar values",
                "reusable scalar initialization",
                "reusable Native source records",
                "reusable typed dependencies",
                "reusable frame return records",
            ]
        } else {
            [
                "managed values",
                "local temporaries",
                "queued factory values",
                "pending values",
                "scalar values",
                "scalar initialization",
                "Native source records",
                "typed dependencies",
                "frame return records",
            ]
        };
        for (name, bank) in names[..4].iter().zip(&self.banks) {
            visit(BufferUsage::new(name, bank));
        }
        for buffer in [
            BufferUsage::new(names[4], &self.scalars),
            BufferUsage::new(names[5], &self.initialized),
            BufferUsage::new(names[6], &self.native_calls),
            BufferUsage::new(names[7], &self.dependencies),
            BufferUsage::new(names[8], &self.returns),
        ] {
            visit(buffer);
        }
    }
}
