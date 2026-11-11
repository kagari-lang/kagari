//! A callback holds exclusive services, never a GC scope or a bank/payload loan.
use super::{
    ScopedValue,
    source::{Bank, ExecutionId, ExecutionRoots, Location, Slots},
};
use crate::error::{RuntimeError, reason::CancellationReason};
use kagari_gc::{error::GcError, root_source::RootDriver};
use std::mem;

pub(super) type NativeStatus = Result<(), Box<RuntimeError>>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PendingReturn {
    Unset,
    Unit,
    Scalar(i64),
    ManagedLocal(usize),
}

#[derive(Clone, Copy)]
pub(super) struct NativeMark {
    pub local_top: usize,
    pub loan_top: usize,
    pub logical_call_top: usize,
}

#[derive(Default)]
pub(super) struct Control {
    pub loans: usize,
    pub effects: usize,
    pub cancelled: bool,
}

pub(super) struct NativeServices<'a> {
    pub memory: &'a mut RootDriver<ExecutionRoots>,
    pub control: &'a mut Control,
    pub script: &'a mut dyn FnMut(i64) -> Result<i64, RuntimeError>,
}

pub(super) struct NativeCall<'a, 'services> {
    pub services: &'a mut NativeServices<'services>,
    pub execution: ExecutionId,
    pub arguments: &'a [Location],
    pub destination: Location,
    pub pending: PendingReturn,
    pub mark: NativeMark,
}

impl<'a, 'services> NativeCall<'a, 'services> {
    pub fn enter(
        services: &'a mut NativeServices<'services>,
        execution: ExecutionId,
        arguments: &'a [Location],
        destination: Location,
    ) -> Result<Self, GcError> {
        let loan_top = services.control.loans;
        let mark = services.memory.scope(|cx, roots| {
            ExecutionRoots::with(roots, cx, |slots| {
                let local_top = slots.len(execution, Bank::Local)?;
                let storage = slots.storage(execution)?;
                let logical_call_top = storage.logical_calls.len();
                storage.logical_calls.try_reserve(1)?;
                storage.logical_calls.push(destination.slot);
                Ok(NativeMark {
                    local_top,
                    loan_top,
                    logical_call_top,
                })
            })
        })?;
        Ok(Self {
            services,
            execution,
            arguments,
            destination,
            pending: PendingReturn::Unset,
            mark,
        })
    }

    pub fn access<T>(
        &mut self,
        operation: impl for<'gc> FnOnce(&mut Slots<'_, 'gc>, &[Location]) -> Result<T, GcError>,
    ) -> Result<T, GcError> {
        self.services.memory.scope(|cx, roots| {
            ExecutionRoots::with(roots, cx, |slots| operation(slots, self.arguments))
        })
    }

    pub fn stage_argument(&mut self, argument: usize) -> Result<(), GcError> {
        self.stage_from(*self.arguments.get(argument).ok_or(GcError::Index)?)
    }

    pub fn stage_from(&mut self, source: Location) -> Result<(), GcError> {
        self.stage(|slots| slots.get(source))
    }

    pub fn stage(
        &mut self,
        value: impl for<'gc> FnOnce(&mut Slots<'_, 'gc>) -> Result<ScopedValue<'gc>, GcError>,
    ) -> Result<(), GcError> {
        let local = self.services.memory.scope(|cx, roots| {
            ExecutionRoots::with(roots, cx, |slots| {
                let value = value(slots)?;
                match value {
                    ScopedValue::Unit => Ok(PendingReturn::Unit),
                    ScopedValue::I64(value) => Ok(PendingReturn::Scalar(value)),
                    ScopedValue::Managed(_) => {
                        let index = slots.len(self.execution, Bank::Local)?;
                        slots.grow(self.execution, Bank::Local, index + 1)?;
                        slots.set(
                            Location {
                                execution: self.execution,
                                bank: Bank::Local,
                                slot: index,
                            },
                            value,
                        )?;
                        Ok(PendingReturn::ManagedLocal(index))
                    }
                }
            })
        })?;
        self.pending = local;
        Ok(())
    }

    /// Poll after retaining callback outcome, before publishing the destination.
    pub fn finish(&mut self, outcome: NativeStatus) -> NativeStatus {
        let pending = mem::replace(&mut self.pending, PendingReturn::Unset);
        if self.services.control.cancelled {
            return Err(Box::new(RuntimeError::from(
                CancellationReason::ExecutionCancelled,
            )));
        }
        outcome?;
        let destination = self.destination;
        self.services
            .memory
            .scope(|cx, roots| {
                ExecutionRoots::with(roots, cx, |slots| match pending {
                    PendingReturn::Unset => Err(GcError::Storage("unset Native return")),
                    PendingReturn::Unit => slots.set(destination, ScopedValue::Unit),
                    PendingReturn::Scalar(value) => slots.set(destination, ScopedValue::I64(value)),
                    PendingReturn::ManagedLocal(slot) => slots.copy(
                        destination,
                        Location {
                            execution: self.execution,
                            bank: Bank::Local,
                            slot,
                        },
                    ),
                })
            })
            .map_err(|_| Box::new(RuntimeError::module_validation("proof return publication")))
    }

    pub fn collect(&mut self) -> Result<(), GcError> {
        assert_eq!(self.services.control.loans, 0);
        self.services.memory.collect()?;
        Ok(())
    }
}

impl Drop for NativeCall<'_, '_> {
    fn drop(&mut self) {
        self.services.control.loans = self.mark.loan_top;
        // On a faulted heap only shutdown is permitted; the source then retains
        // inert carriers. Ordinary error/unwind paths retire exactly this suffix.
        let _ = self.services.memory.scope(|cx, roots| {
            ExecutionRoots::with(roots, cx, |slots| {
                slots.truncate(self.execution, Bank::Local, self.mark.local_top)?;
                slots
                    .storage(self.execution)?
                    .logical_calls
                    .truncate(self.mark.logical_call_top);
                Ok(())
            })
        });
    }
}
