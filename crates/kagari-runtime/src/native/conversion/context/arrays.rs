//! Fixed-array operations keep preparation protected and commit after polling.
use crate::{
    Runtime,
    error::{RuntimeError, reason::IndexError, trap::TrapReason},
    execution::roots::{ExecutionRoots, Slots},
    frame::types::arguments::TypeArgument,
    native::{
        binding::NativeResult,
        conversion::context::{ConversionContext, inputs::ValueInput},
        storage_type::StorageType,
    },
    value::{
        arrays::{ArrayObject, storage::ArrayValues},
        carriers::ScopedValue,
        kind::ObjectKind,
        view::ValueView,
    },
    value_check::{
        self,
        scoped::{self, CheckError},
    },
};
use kagari_gc::{access::Scope, error::GcError, object::Gc};
use std::{
    ops::{Bound, Range},
    sync::Arc,
};

const CHUNK: usize = 1024;

impl<'runtime> ConversionContext<'runtime> {
    /// Construct a builtin fixed array from protected values in a checked element
    /// scope. No escaping Host root or static-mapping rebuild is needed per cell.
    pub fn allocate_array(
        &mut self,
        element: &TypeArgument,
        values: &[ValueView<'_>],
    ) -> NativeResult<ValueView<'runtime>> {
        element.validate(self.runtime)?;
        let contract = Arc::new(StorageType::prepare_scoped(element.clone(), self.owner)?);
        self.allocate_array_inputs(contract, values.iter().copied().map(ValueInput::View))
    }

    pub(crate) fn allocate_array_inputs<'root, 'call>(
        &mut self,
        contract: Arc<StorageType>,
        values: impl ExactSizeIterator<Item = ValueInput<'root, 'call>>,
    ) -> NativeResult<ValueView<'runtime>> {
        self.poll()?;
        let execution = self.execution_id()?;
        let Runtime {
            memory,
            resources,
            host,
            host_borrows,
            ..
        } = &mut *self.runtime;
        let result = memory.scope(resources, |cx, roots| {
            ExecutionRoots::with(roots, cx, |slots| {
                let mut prepared = ArrayValues::empty(&contract.ty);
                prepared.reserve(values.len())?;
                for (index, input) in values.enumerate() {
                    if index % CHUNK == 0
                        && let Err(error) = resources.poll_closed()
                    {
                        return Ok(Err(error));
                    }
                    let value = input.get(slots)?;
                    let matched = match scoped::matches(
                        slots.scope(),
                        value,
                        contract.view(),
                        host,
                        host_borrows,
                        resources,
                    ) {
                        Ok(matched) => matched,
                        Err(CheckError::Storage(error)) => return Err(error),
                        Err(CheckError::Execution(error)) => return Ok(Err(error)),
                    };
                    if !matched {
                        return Ok(Err(RuntimeError::from(
                            TrapReason::InvalidSequenceBufferOrPayload,
                        )));
                    }
                    prepared.push(slots.scope(), value)?;
                }
                if let Err(error) = resources.poll_closed() {
                    return Ok(Err(error));
                }
                let array = ArrayObject::alloc(
                    slots.scope(),
                    contract,
                    prepared,
                    resources.candidate_origin(),
                )?;
                slots
                    .protect_local(execution, ScopedValue::managed(array))
                    .map(Ok)
            })
        });
        result
            .map_err(|error| self.gc_error(error))?
            .map_err(|error| self.runtime.resources.admit_execution_error(error))
    }

    pub(crate) fn allocate_array_repeat_input(
        &mut self,
        contract: Arc<StorageType>,
        input: ValueInput<'_, '_>,
        count: u64,
    ) -> NativeResult<ValueView<'runtime>> {
        self.poll()?;
        let execution = self.execution_id()?;
        let Runtime {
            memory,
            resources,
            host,
            host_borrows,
            ..
        } = &mut *self.runtime;
        let result = memory.scope(resources, |cx, roots| {
            ExecutionRoots::with(roots, cx, |slots| {
                let value = input.get(slots)?;
                let matched = match scoped::matches(
                    slots.scope(),
                    value,
                    contract.view(),
                    host,
                    host_borrows,
                    resources,
                ) {
                    Ok(matched) => matched,
                    Err(CheckError::Storage(error)) => return Err(error),
                    Err(CheckError::Execution(error)) => return Ok(Err(error)),
                };
                if !matched {
                    return Ok(Err(RuntimeError::from(
                        TrapReason::InvalidArrayTargetOrPayload,
                    )));
                }
                if let Err(error) = resources.poll_closed() {
                    return Ok(Err(error));
                }
                let count = usize::try_from(count)
                    .map_err(|_| GcError::Limit("array length exceeds platform capacity"))?;
                let mut prepared = ArrayValues::empty(&contract.ty);
                prepared.reserve(count)?;
                for start in (0..count).step_by(CHUNK) {
                    if let Err(error) = resources.poll_closed() {
                        return Ok(Err(error));
                    }
                    prepared.repeat(slots.scope(), value, (count - start).min(CHUNK))?;
                }
                if let Err(error) = resources.poll_closed() {
                    return Ok(Err(error));
                }
                let array = ArrayObject::alloc(
                    slots.scope(),
                    contract,
                    prepared,
                    resources.candidate_origin(),
                )?;
                slots
                    .protect_local(execution, ScopedValue::managed(array))
                    .map(Ok)
            })
        });
        result
            .map_err(|error| self.gc_error(error))?
            .map_err(|error| self.runtime.resources.admit_execution_error(error))
    }

    pub fn array_len(&mut self, value: &ValueView<'_>) -> NativeResult<usize> {
        value_check::candidate_protected(self.runtime, &[value.reborrow()])?;
        self.with_values(|slots| {
            let Some(array) = array(slots, *value)? else {
                return Ok(Err(expected_array()));
            };
            Ok(Ok(slots.scope().read(array, ArrayObject::len)))
        })?
    }

    /// Failed machine conversion is an absent element after receiver admission.
    /// A selected managed child is published locally before this method returns.
    pub fn array_element(
        &mut self,
        value: &ValueView<'_>,
        index: impl TryInto<usize>,
    ) -> NativeResult<Option<ValueView<'runtime>>> {
        value_check::candidate_protected(self.runtime, &[value.reborrow()])?;
        let index = index.try_into().ok();
        let execution = self.execution_id()?;
        self.with_values(|slots| {
            let Some(array) = array(slots, *value)? else {
                return Ok(Err(expected_array()));
            };
            let length = slots.scope().read(array, ArrayObject::len);
            let Some(index) = index.filter(|index| *index < length) else {
                return Ok(Ok(None));
            };
            let value = ArrayObject::get(slots.scope(), array, index)?;
            slots
                .protect_local(execution, value)
                .map(|value| Ok(Some(value)))
        })?
    }

    pub fn array_set(
        &mut self,
        receiver: &ValueView<'_>,
        index: impl TryInto<usize>,
        value: &ValueView<'_>,
    ) -> NativeResult<()> {
        value_check::candidate_protected(self.runtime, &[receiver.reborrow(), value.reborrow()])?;
        let index = index.try_into().ok();
        self.poll()?;
        let Runtime {
            memory,
            host,
            host_borrows,
            resources,
            ..
        } = &mut *self.runtime;
        let result = memory.scope(resources, |cx, roots| {
            ExecutionRoots::with(roots, cx, |slots| {
                let Some(array) = array(slots, *receiver)? else {
                    return Ok(Err(expected_array()));
                };
                let value = value.get(slots)?;
                let cx: &Scope<'_> = slots.scope();
                let valid = match cx.read(array, |array| {
                    scoped::matches(
                        cx,
                        value,
                        array.contract().view(),
                        host,
                        host_borrows,
                        resources,
                    )
                }) {
                    Ok(valid) => valid,
                    Err(CheckError::Storage(error)) => return Err(error),
                    Err(CheckError::Execution(error)) => return Ok(Err(error)),
                };
                if !valid {
                    return Ok(Err(RuntimeError::from(
                        TrapReason::InvalidSequenceBufferOrPayload,
                    )));
                }
                let length = cx.read(array, ArrayObject::len);
                let Some(index) = index.filter(|index| *index < length) else {
                    return Ok(Err(RuntimeError::from(
                        IndexError::SequenceIndexIsOutOfBounds,
                    )));
                };
                ArrayObject::set(slots.scope(), array, index, value)?;
                Ok(Ok(()))
            })
        });
        result
            .map_err(|error| self.gc_error(error))?
            .map_err(|error| self.runtime.resources.admit_execution_error(error))
    }

    /// Stage a complete replacement, checking cancellation between preparation
    /// chunks. Publication/commit is an indivisible primitive operation; a later
    /// post-call cancellation preserves its completed mutation.
    pub fn array_fill(
        &mut self,
        receiver: &ValueView<'_>,
        value: &ValueView<'_>,
    ) -> NativeResult<()> {
        value_check::candidate_protected(self.runtime, &[receiver.reborrow(), value.reborrow()])?;
        self.poll()?;
        let Runtime {
            memory,
            resources,
            host,
            host_borrows,
            ..
        } = &mut *self.runtime;
        let result = memory.scope(resources, |cx, roots| {
            ExecutionRoots::with(roots, cx, |slots| {
                let Some(array) = array(slots, *receiver)? else {
                    return Ok(Err(expected_array()));
                };
                let value = value.get(slots)?;
                let cx: &Scope<'_> = slots.scope();
                let valid = match cx.read(array, |array| {
                    scoped::matches(
                        cx,
                        value,
                        array.contract().view(),
                        host,
                        host_borrows,
                        resources,
                    )
                }) {
                    Ok(valid) => valid,
                    Err(CheckError::Storage(error)) => return Err(error),
                    Err(CheckError::Execution(error)) => return Ok(Err(error)),
                };
                if !valid {
                    return Ok(Err(RuntimeError::from(
                        TrapReason::InvalidSequenceBufferOrPayload,
                    )));
                }
                Ok(Ok(cx.read(array, ArrayObject::len)))
            })
        });
        let count = result
            .map_err(|error| self.gc_error(error))?
            .map_err(|error| self.runtime.resources.admit_execution_error(error))?;
        let Runtime {
            memory, resources, ..
        } = &mut *self.runtime;
        // Quota polling may capture a trace. No source/control loan crosses this
        // non-reentrant service, so original shape/type admission remains valid.
        let temporary = resources.reserve_temporary_heap(count)?;
        let result = memory.scope(resources, |cx, roots| {
            ExecutionRoots::with(roots, cx, |slots| {
                let array = array(slots, *receiver)?.ok_or(GcError::Representation)?;
                let value = value.get(slots)?;
                let mut prepared = slots
                    .scope()
                    .read(array, |array| ArrayValues::empty(&array.contract().ty));
                prepared.reserve(count)?;
                for start in (0..count).step_by(CHUNK) {
                    if let Err(error) = resources.poll_closed() {
                        return Ok(Err(error));
                    }
                    prepared.repeat(slots.scope(), value, (count - start).min(CHUNK))?;
                }
                if let Err(error) = resources.poll_closed() {
                    return Ok(Err(error));
                }
                ArrayObject::replace(slots.scope(), array, prepared)?;
                Ok(Ok(()))
            })
        });
        drop(temporary);
        result
            .map_err(|error| self.gc_error(error))?
            .map_err(|error| self.runtime.resources.admit_execution_error(error))
    }

    pub fn array_copy_from(
        &mut self,
        receiver: &ValueView<'_>,
        source: &ValueView<'_>,
    ) -> NativeResult<()> {
        value_check::candidate_protected(self.runtime, &[receiver.reborrow(), source.reborrow()])?;
        self.poll()?;
        let result = self
            .runtime
            .memory
            .scope(&self.runtime.resources, |cx, roots| {
                ExecutionRoots::with(roots, cx, |slots| {
                    let Some(target) = array(slots, *receiver)? else {
                        return Ok(Err(expected_array()));
                    };
                    let Some(source) = array(slots, *source)? else {
                        return Ok(Err(expected_array()));
                    };
                    let count = slots.scope().read(target, ArrayObject::len);
                    if slots.scope().read(source, ArrayObject::len) != count {
                        return Ok(Err(RuntimeError::from(
                            TrapReason::ArrayCopyRequiresEqualLengths,
                        )));
                    }
                    let cx: &Scope<'_> = slots.scope();
                    if !cx.read(target, |target| {
                        cx.read(source, |source| {
                            source.contract().same_type(target.contract())
                        })
                    }) {
                        return Ok(Err(RuntimeError::from(
                            TrapReason::InvalidSequenceBufferOrPayload,
                        )));
                    }
                    Ok(Ok(count))
                })
            });
        let count = result.map_err(|error| self.gc_error(error))??;
        let Runtime {
            memory, resources, ..
        } = &mut *self.runtime;
        let temporary = resources.reserve_temporary_heap(count)?;
        let result = memory.scope(resources, |cx, roots| {
            ExecutionRoots::with(roots, cx, |slots| {
                let target = array(slots, *receiver)?.ok_or(GcError::Representation)?;
                let source = array(slots, *source)?.ok_or(GcError::Representation)?;
                let mut prepared = slots
                    .scope()
                    .read(target, |array| ArrayValues::empty(&array.contract().ty));
                prepared.reserve(count)?;
                for start in (0..count).step_by(CHUNK) {
                    if let Err(error) = resources.poll_closed() {
                        return Ok(Err(error));
                    }
                    ArrayObject::copy_into(
                        slots.scope(),
                        source,
                        start..(start + (count - start).min(CHUNK)),
                        &mut prepared,
                    )?;
                }
                if let Err(error) = resources.poll_closed() {
                    return Ok(Err(error));
                }
                ArrayObject::replace(slots.scope(), target, prepared)?;
                Ok(Ok(()))
            })
        });
        drop(temporary);
        result
            .map_err(|error| self.gc_error(error))?
            .map_err(|error| self.runtime.resources.admit_execution_error(error))
    }

    pub fn array_copy_within(
        &mut self,
        receiver: &ValueView<'_>,
        start: Bound<usize>,
        end: Bound<usize>,
        destination: usize,
    ) -> NativeResult<()> {
        value_check::candidate_protected(self.runtime, &[receiver.reborrow()])?;
        self.poll()?;
        let result = self
            .runtime
            .memory
            .scope(&self.runtime.resources, |cx, roots| {
                ExecutionRoots::with(roots, cx, |slots| {
                    let Some(target) = array(slots, *receiver)? else {
                        return Ok(Err(expected_array()));
                    };
                    let length = slots.scope().read(target, ArrayObject::len);
                    let range = match copy_range(length, start, end, destination) {
                        Ok(range) => range,
                        Err(error) => return Ok(Err(error)),
                    };
                    Ok(Ok(range))
                })
            });
        let range = result.map_err(|error| self.gc_error(error))??;
        let Runtime {
            memory, resources, ..
        } = &mut *self.runtime;
        let temporary = resources.reserve_temporary_heap(range.len())?;
        let result = memory.scope(resources, |cx, roots| {
            ExecutionRoots::with(roots, cx, |slots| {
                let target = array(slots, *receiver)?.ok_or(GcError::Representation)?;
                let mut prepared = slots
                    .scope()
                    .read(target, |array| ArrayValues::empty(&array.contract().ty));
                prepared.reserve(range.len())?;
                for start in (range.start..range.end).step_by(CHUNK) {
                    if let Err(error) = resources.poll_closed() {
                        return Ok(Err(error));
                    }
                    ArrayObject::copy_into(
                        slots.scope(),
                        target,
                        start..(start + (range.end - start).min(CHUNK)),
                        &mut prepared,
                    )?;
                }
                if let Err(error) = resources.poll_closed() {
                    return Ok(Err(error));
                }
                ArrayObject::overwrite(slots.scope(), target, destination, prepared)?;
                Ok(Ok(()))
            })
        });
        drop(temporary);
        result
            .map_err(|error| self.gc_error(error))?
            .map_err(|error| self.runtime.resources.admit_execution_error(error))
    }
}

fn array<'gc>(
    slots: &mut Slots<'_, 'gc>,
    value: ValueView<'_>,
) -> Result<Option<Gc<'gc, ArrayObject>>, GcError> {
    let value = value.get(slots)?;
    if value.kind() != Some(ObjectKind::Array) {
        return Ok(None);
    }
    let value = value.object().ok_or(GcError::Representation)?;
    slots.scope().downcast::<ArrayObject>(value).map(Some)
}

fn expected_array() -> RuntimeError {
    RuntimeError::from(TrapReason::ExpectedABuiltinArrayObject)
}

fn copy_range(
    length: usize,
    start: Bound<usize>,
    end: Bound<usize>,
    destination: usize,
) -> NativeResult<Range<usize>> {
    let bounds = || RuntimeError::from(IndexError::ArrayCopyRangeIsOutOfBounds);
    let start = match start {
        Bound::Unbounded => 0,
        Bound::Included(n) => n,
        Bound::Excluded(n) => n.checked_add(1).ok_or_else(bounds)?,
    };
    let end = match end {
        Bound::Unbounded => length,
        Bound::Excluded(n) => n,
        Bound::Included(n) => n.checked_add(1).ok_or_else(bounds)?,
    };
    if start > end || end > length || destination > length || end - start > length - destination {
        return Err(bounds());
    }
    Ok(start..end)
}

#[cfg(test)]
mod tests;
