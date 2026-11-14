//! Linear result publication into caller-owned protected execution storage.
mod destination;
use crate::{
    Runtime,
    error::RuntimeError,
    execution::roots::{ExecutionRoots, Slots, ValueLocation},
    frame::values::FrameSlots,
    module::execution::layout::Location,
    native::binding::NativeResult,
    value::{carriers::ScopedValue, place::destination::Destination, view::ValueView},
};
use kagari_gc::error::GcError;
use std::{marker::PhantomData, rc::Rc};

/// One caller-provided destination. Consume it to publish before callee scratch
/// retires. This is a non-owning coordinate, not a Host root or payload pointer.
/// The caller owns storage and cleanup; a forgotten place publishes no result.
///
/// Publication consumes the destination even when a callback keeps executing:
/// ```compile_fail,E0382
/// use kagari_runtime::{Runtime, native::binding::NativeResult, value::{
///     place::ResultPlace, view::ValueView,
/// }};
/// fn publish_twice(runtime: &mut Runtime, place: ResultPlace<'_>) -> NativeResult<()> {
///     let _published = place.publish(runtime, &ValueView::unit())?;
///     let _again = place.publish(runtime, &ValueView::unit())?;
///     Ok(())
/// }
/// ```
pub struct ResultPlace<'call> {
    destination: Destination<'call>,
    brand: PhantomData<(&'call mut &'call (), Rc<()>)>,
}

/// Evidence of completed source publication. It neither owns the value nor
/// extends storage lifetime; the caller checks its own destination before use.
pub struct PublishedResult<'call> {
    destination: Destination<'call>,
    brand: PhantomData<(&'call mut &'call (), Rc<()>)>,
}

impl<'call> ResultPlace<'call> {
    /// The caller keeps this destination protected through result admission and
    /// consumes the returned proof before retiring the source suffix.
    pub(crate) fn new(location: ValueLocation) -> Self {
        Self {
            destination: Destination::Source(location),
            brand: PhantomData,
        }
    }

    /// The frame owner lends only copied coordinates and immutable prepared
    /// facts. No frame/source/payload borrow is held across arbitrary Native code.
    pub(crate) fn frame(frame: &'call FrameSlots, location: Option<&'call Location>) -> Self {
        Self {
            destination: Destination::Frame(&frame.banks, location),
            brand: PhantomData,
        }
    }

    /// Publish a protected value exactly once, without retaining a Host lease.
    /// Input/destination owner, generation and bounds are checked before commit.
    /// Expired/foreign coordinates return ModuleValidation; allocation, heap and
    /// Runtime lifecycle failures retain their normal termination/fault behavior.
    pub fn publish(
        self,
        runtime: &mut Runtime,
        value: &ValueView<'_>,
    ) -> NativeResult<PublishedResult<'call>> {
        runtime.resources.ensure_synchronous_thread()?;
        runtime.resources.ensure_execution_allowed()?;
        let result = runtime.memory.scope(&runtime.resources, |cx, roots| {
            ExecutionRoots::with(roots, cx, |slots| self.destination.publish(slots, value))
        });
        result.map_err(|error| match error {
            GcError::ForeignHeap
            | GcError::InvalidRoot
            | GcError::Index
            | GcError::Representation => RuntimeError::module_validation(error.to_string()),
            error => runtime.managed_memory_error(error),
        })?;
        Ok(self.published())
    }

    /// Publish a newly allocated/reflected result while its original closed GC
    /// scope is still live. This consumes the same destination and validation as
    /// publish; no temporary source cell or Host lease extends the value's life.
    pub(crate) fn publish_scoped<'gc>(
        self,
        slots: &mut Slots<'_, 'gc>,
        value: ScopedValue<'gc>,
    ) -> Result<PublishedResult<'call>, GcError> {
        self.destination.publish_scoped(slots, value)?;
        Ok(self.published())
    }

    fn published(self) -> PublishedResult<'call> {
        PublishedResult {
            destination: self.destination,
            brand: PhantomData,
        }
    }
}

impl PublishedResult<'_> {
    pub(crate) fn admit(self, expected: ValueLocation) -> NativeResult<ValueLocation> {
        if !self.destination.admits_source(expected) {
            return Err(RuntimeError::module_validation(
                "result published to another destination",
            ));
        }
        Ok(expected)
    }

    pub(crate) fn admit_frame(
        self,
        frame: &FrameSlots,
        location: Option<&Location>,
    ) -> NativeResult<()> {
        if !self.destination.admits_frame(&frame.banks, location) {
            return Err(RuntimeError::module_validation(
                "result published to another frame destination",
            ));
        }
        Ok(())
    }
}
