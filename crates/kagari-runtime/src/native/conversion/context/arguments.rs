//! Parameter packs occupy immutable local prefixes in the authoritative source.
use crate::{
    Runtime,
    error::{RuntimeError, trap::TrapReason},
    execution::roots::{ExecutionRoots, Slots, ValueBank, queued::QueuedArguments},
    native::{
        binding::NativeResult,
        conversion::{
            arguments::ArgumentValues,
            context::{ConversionContext, Frame},
        },
    },
    value::{carriers::RootedValue, view::ValueView},
    value_check::{self, data::async_arguments, scoped::CheckError},
};
use kagari_gc::error::GcError;

impl<'call> ConversionContext<'call> {
    /// Copy Host inputs once into a nonescaping argument prefix. The original
    /// root slice supplies protection; no intermediate imported-view pack exists.
    pub(crate) fn root_arguments(
        &mut self,
        values: &[RootedValue],
    ) -> NativeResult<ArgumentValues<'call>> {
        for value in values {
            value_check::candidate_root(self.runtime, value)?;
        }
        let mut scratch = self.scratch()?;
        let execution = scratch.execution_id()?;
        let Runtime {
            memory, resources, ..
        } = &mut *scratch.runtime;
        let result = memory.scope(resources, |cx, roots| {
            ExecutionRoots::with(roots, cx, |slots| {
                let start = slots.len(execution, ValueBank::Local)?;
                slots.reserve_locals(execution, values.len())?;
                Ok((|| -> Result<_, CheckError> {
                    for (index, value) in values.iter().enumerate() {
                        if index.is_multiple_of(1024) {
                            resources.poll_closed()?;
                        }
                        let value = value.get(slots.scope())?;
                        slots.push_local(execution, value)?;
                    }
                    Ok(ArgumentValues::new(execution, start, values.len()))
                })())
            })
        });
        let pack = result
            .map_err(|error| scratch.gc_error(error))?
            .map_err(|error| {
                scratch
                    .runtime
                    .resources
                    .admit_execution_error(error.into_runtime(scratch.runtime))
            })?;
        scratch.preserve_arguments(pack)
    }

    /// Transfer a complete encoded pack into the eventual owned execution's cold
    /// queue. This owner survives conversion cleanup, but not session retirement.
    pub(in crate::native) fn queue_arguments(
        &mut self,
        pack: ArgumentValues<'_>,
    ) -> NativeResult<QueuedArguments> {
        if self.owns_execution {
            return Err(RuntimeError::module_validation(
                "queued arguments require an owned invocation",
            ));
        }
        let execution = self.execution_id()?;
        let resources = &self.runtime.resources;
        let result = self
            .runtime
            .memory
            .scope(&self.runtime.resources, |cx, roots| {
                ExecutionRoots::with(roots, cx, |slots| {
                    Ok((|| -> Result<_, CheckError> {
                        if !async_arguments(slots, pack, resources)? {
                            return Err(RuntimeError::from(
                                TrapReason::AsyncStateRetainsAnInvalidOrScopedValue,
                            )
                            .into());
                        }
                        let queued = slots.begin_queue(execution, pack.len())?;
                        let publish = (|| -> Result<(), CheckError> {
                            for index in 0..pack.len() {
                                if index.is_multiple_of(1024) {
                                    resources.poll_closed()?;
                                }
                                let value = pack.scoped(slots, index)?;
                                slots.set_queued(&queued, index, value)?;
                            }
                            Ok(())
                        })();
                        match publish {
                            Ok(()) => Ok(queued),
                            Err(error) => {
                                slots.retire_queue(queued)?;
                                Err(error)
                            }
                        }
                    })())
                })
            });
        result
            .map_err(|error| self.gc_error(error))?
            .map_err(|error| {
                self.runtime
                    .resources
                    .admit_execution_error(error.into_runtime(self.runtime))
            })
    }

    /// Protect a complete value pack before a callback can collect or reenter.
    /// The returned coordinates borrow this context's local lifetime, without a
    /// Host root or a second managed-value Vec. Scalars use the same pack slots.
    pub fn argument_values(
        &mut self,
        values: &[ValueView<'_>],
    ) -> NativeResult<ArgumentValues<'call>> {
        // Admit the complete original prefix before growth could reuse an
        // expired coordinate named by a later input. No callback intervenes.
        self.with_values(|slots| {
            for value in values {
                value.get(slots)?;
            }
            Ok(())
        })?;
        self.append_arguments(values.len(), |_, index| Ok(values[index].reborrow()))
    }

    pub(crate) fn validate_arguments(&mut self, pack: ArgumentValues<'_>) -> NativeResult<()> {
        self.execution_id()?;
        self.with_values(|slots| pack.check_bounds(slots))
    }

    pub(crate) fn argument_value(
        &mut self,
        pack: ArgumentValues<'_>,
        index: usize,
    ) -> NativeResult<ValueView<'call>> {
        self.execution_id()?;
        self.with_values(|slots| pack.view(slots, index))
    }

    pub(crate) fn prefix_arguments(
        &mut self,
        receiver: ValueView<'_>,
        arguments: ArgumentValues<'_>,
    ) -> NativeResult<ArgumentValues<'call>> {
        self.validate_arguments(arguments)?;
        // Admit the receiver before growth as well: reserving/appending a pack
        // must not make an expired receiver coordinate live again.
        self.with_values(|slots| receiver.get(slots).map(|_| ()))?;
        let count = arguments
            .len()
            .checked_add(1)
            .ok_or_else(|| RuntimeError::resource_limit("argument count"))?;
        self.append_arguments(count, |slots, index| {
            if index == 0 {
                Ok(receiver.reborrow())
            } else {
                arguments.view(slots, index - 1)
            }
        })
    }

    fn append_arguments(
        &mut self,
        count: usize,
        mut value: impl for<'gc> FnMut(&mut Slots<'_, 'gc>, usize) -> Result<ValueView<'call>, GcError>,
    ) -> NativeResult<ArgumentValues<'call>> {
        let mut scratch = self.scratch()?;
        let execution = scratch.execution_id()?;
        let pack = scratch.with_values(|slots| {
            let start = slots.len(execution, ValueBank::Local)?;
            // No custom converter runs during the closed source handoff. A
            // failure leaves the parent suffix owner responsible for cleanup.
            let end = start
                .checked_add(count)
                .ok_or(GcError::Limit("argument count"))?;
            slots.reserve_locals(execution, count)?;
            for index in 0..count {
                let value = value(slots, index)?.get(slots)?;
                slots.push_local(execution, value)?;
            }
            debug_assert_eq!(slots.len(execution, ValueBank::Local)?, end);
            Ok(ArgumentValues::new(execution, start, count))
        })?;
        scratch.preserve_arguments(pack)
    }

    pub(crate) fn encode_argument_pack(
        &mut self,
        encode: impl FnOnce(&mut Self) -> NativeResult<ArgumentValues<'call>>,
    ) -> NativeResult<ArgumentValues<'call>> {
        let mut frame = self.enter(None)?;
        let pack = encode(&mut frame)
            .map_err(|error| frame.runtime.resources().admit_execution_error(error))?;
        frame.preserve_arguments(pack)
    }
}

impl<'call> Frame<'_, 'call> {
    fn preserve_arguments(
        &mut self,
        pack: ArgumentValues<'call>,
    ) -> NativeResult<ArgumentValues<'call>> {
        let mark = self.locals.as_mut().expect("argument scope mark");
        let (execution, source, count) = pack.contiguous().ok_or_else(|| {
            RuntimeError::module_validation("converted pack does not own local storage")
        })?;
        if mark.execution() != execution {
            return Err(RuntimeError::module_validation("foreign argument pack"));
        }
        let start = mark.top();
        self.context
            .with_values(|slots| slots.preserve_local_pack(mark, source, count))?;
        Ok(ArgumentValues::new(execution, start, count))
    }
}
