//! Owned tuple adapters use stack scratch and protected immutable fields.
use crate::{
    Runtime,
    execution::roots::{ExecutionRoots, Slots},
    native::{binding::NativeResult, conversion::context::ConversionContext},
    value::{aggregates::tuples::TupleObject, carriers::ScopedValue, view::ValueView},
};
#[cfg(feature = "serde")]
use crate::{execution::roots::ValueBank, native::conversion::arguments::ArgumentValues};
use kagari_gc::{error::GcError, object::Gc};

impl<'runtime> ConversionContext<'runtime> {
    /// Build a tuple from protected values in this conversion source.
    ///
    /// Inputs must belong to a live, accessible source in this Runtime. The
    /// returned view stays protected until this context retires; retain it only
    /// when it must escape to the Host. Declared field types and Host ownership
    /// are checked by typed conversion or the receiving call's result boundary.
    /// Invalid source coordinates, allocation failure and cancellation return
    /// their ordinary Runtime errors. Fixed input preparation uses stack scratch.
    pub fn allocate_tuple<const N: usize>(
        &mut self,
        values: &[ValueView<'_>; N],
    ) -> NativeResult<ValueView<'runtime>> {
        self.poll()?;
        let execution = self.execution_id()?;
        let Runtime {
            memory, resources, ..
        } = &mut *self.runtime;
        let result = memory.scope(resources, |cx, roots| {
            ExecutionRoots::with(roots, cx, |slots| {
                let mut input = [ScopedValue::unit(); N];
                for (index, (input, value)) in input.iter_mut().zip(values).enumerate() {
                    if index.is_multiple_of(1024)
                        && let Err(error) = resources.poll_closed()
                    {
                        return Ok(Err(error));
                    }
                    *input = value.get(slots)?;
                }
                if let Err(error) = resources.poll_closed() {
                    return Ok(Err(error));
                }
                let tuple = TupleObject::alloc(slots.scope(), &input)?;
                let value = ScopedValue::managed(tuple);
                let location = slots.push_local(execution, value)?;
                Ok(Ok(ValueView::protected(value, Some(location))?))
            })
        });
        result.map_err(|error| self.gc_error(error))?
    }

    /// Dynamic Serde tuples keep scoped preparation separate from the final
    /// provider's owned fields. Fixed Rust tuples retain their stack path above.
    #[cfg(feature = "serde")]
    pub(in crate::native) fn allocate_tuple_values(
        &mut self,
        values: &[ValueView<'_>],
    ) -> NativeResult<ValueView<'runtime>> {
        // The streaming pack admitted its complete count before any child
        // converter. Rechecking the remaining node quota here would count the
        // already completed children a second time.
        self.poll()?;
        let execution = self.execution_id()?;
        let Runtime {
            memory, resources, ..
        } = &mut *self.runtime;
        let result = memory.scope(resources, |cx, roots| {
            ExecutionRoots::with(roots, cx, |slots| {
                let mut input = Vec::new();
                input.try_reserve_exact(values.len())?;
                for (index, value) in values.iter().enumerate() {
                    if index != 0
                        && index.is_multiple_of(1024)
                        && let Err(error) = resources.poll_closed()
                    {
                        return Ok(Err(error));
                    }
                    input.push(value.get(slots)?);
                }
                let object = TupleObject::alloc(slots.scope(), &input)?;
                let value = ScopedValue::managed(object);
                let location = slots.push_local(execution, value)?;
                Ok(Ok(ValueView::protected(value, Some(location))?))
            })
        });
        result.map_err(|error| self.gc_error(error))?
    }

    /// Protect the complete immediate-child prefix before a Serde visitor runs.
    /// Its enclosing conversion suffix retires the snapshot on every exit.
    #[cfg(feature = "serde")]
    pub(in crate::native) fn tuple_arguments(
        &mut self,
        value: &ValueView<'_>,
    ) -> NativeResult<ArgumentValues<'runtime>> {
        let count = self.tuple_len(value)?;
        self.check_elements(count)?;
        let execution = self.execution_id()?;
        let Runtime {
            memory, resources, ..
        } = &mut *self.runtime;
        let result = memory.scope(resources, |cx, roots| {
            ExecutionRoots::with(roots, cx, |slots| {
                // Re-admit the protected parent before any source growth.
                let object = tuple(slots, *value)?;
                let start = slots.len(execution, ValueBank::Local)?;
                slots.reserve_locals(execution, count)?;
                for index in 0..count {
                    if index != 0
                        && index.is_multiple_of(1024)
                        && let Err(error) = resources.poll_closed()
                    {
                        return Ok(Err(error));
                    }
                    let value = TupleObject::get(slots.scope(), object, index)?;
                    slots.push_local(execution, value)?;
                }
                Ok(Ok(ArgumentValues::new(execution, start, count)))
            })
        });
        result.map_err(|error| self.gc_error(error))?
    }

    pub fn tuple_len(&mut self, value: &ValueView<'_>) -> NativeResult<usize> {
        self.with_values(|slots| {
            let tuple = tuple(slots, *value)?;
            Ok(slots.scope().read(tuple, TupleObject::len))
        })
    }

    /// Snapshot the selected immutable member into this conversion's local area
    /// before any child adapter can collect or reenter. No payload borrow escapes.
    pub fn tuple_element(
        &mut self,
        value: &ValueView<'_>,
        index: usize,
    ) -> NativeResult<ValueView<'runtime>> {
        let execution = self.execution_id()?;
        self.with_values(|slots| {
            let tuple = tuple(slots, *value)?;
            let value = TupleObject::get(slots.scope(), tuple, index)?;
            let location = if value.object().is_some() {
                Some(slots.push_local(execution, value)?)
            } else {
                None
            };
            ValueView::protected(value, location)
        })
    }
}

fn tuple<'gc>(
    slots: &mut Slots<'_, 'gc>,
    value: ValueView<'_>,
) -> Result<Gc<'gc, TupleObject>, GcError> {
    let value = value.get(slots)?.object().ok_or(GcError::Representation)?;
    slots.scope().downcast::<TupleObject>(value)
}
