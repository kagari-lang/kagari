//! Managed fields publish through their actual payload and protect child decoders.
use crate::{
    execution::roots::Slots,
    native::{
        binding::NativeResult,
        conversion::context::{
            ConversionContext,
            native::{NativeContract, revision_exhausted},
        },
        payload::{data::NativeData, managed::Managed},
    },
    value::{
        carriers::RootedValue, native::NativeObject, properties::ValueProperties, view::ValueView,
    },
    value_check,
};
use kagari_gc::{error::GcError, object::Gc};
use std::panic::resume_unwind;

fn admit<'gc, T: NativeData>(
    slots: &mut Slots<'_, 'gc>,
    root: &RootedValue,
    contract: &NativeContract<'_>,
) -> Result<(Gc<'gc, NativeObject>, Gc<'gc, Managed<T>>), GcError> {
    let value = root.get(slots.scope())?;
    let object = slots
        .scope()
        .downcast::<NativeObject>(value.object().ok_or(GcError::Representation)?)?;
    if !slots.scope().read(object, |object| contract.check(object))
        || !contract.storage.accepts_traced::<Managed<T>>()
    {
        return Err(GcError::Representation);
    }
    let payload = NativeObject::payload(slots.scope(), object);
    let payload = slots.scope().downcast::<Managed<T>>(payload)?;
    Ok((object, payload))
}

impl<'runtime> ConversionContext<'runtime> {
    pub(crate) fn managed_field<T: NativeData>(
        &mut self,
        root: &RootedValue,
        contract: NativeContract<'_>,
        slot: usize,
    ) -> NativeResult<ValueView<'runtime>> {
        value_check::candidate_root(self.runtime, root)?;
        self.poll()?;
        let execution = self.execution_id()?;
        self.with_values(|slots| {
            let (_, payload) = admit::<T>(slots, root, &contract)?;
            let value = Managed::get(slots.scope(), payload, slot)?;
            let location = if value.object().is_some() {
                Some(slots.push_local(execution, value)?)
            } else {
                None
            };
            ValueView::protected(value, location)
        })
    }

    pub(crate) fn set_managed_field<T: NativeData>(
        &mut self,
        root: &RootedValue,
        contract: NativeContract<'_>,
        slot: usize,
        value: &ValueView<'_>,
    ) -> NativeResult<()> {
        value_check::candidate_root(self.runtime, root)?;
        value_check::candidate_protected(self.runtime, &[value.reborrow()])?;
        self.poll()?;
        self.with_values(|slots| {
            let value = value.get(slots)?;
            if !ValueProperties::of(slots.scope(), value)?.storable {
                return Err(GcError::Representation);
            }
            let (object, payload) = admit::<T>(slots, root, &contract)?;
            if slots
                .scope()
                .read(object, |object| object.revision() == u64::MAX)
            {
                return Ok(Err(revision_exhausted()));
            }
            Managed::set(slots.scope(), payload, slot, value)?;
            NativeObject::advance_revision(slots.scope(), object)?;
            Ok(Ok(()))
        })?
    }

    pub(crate) fn edit_managed_data<T: NativeData, R>(
        &mut self,
        root: &RootedValue,
        contract: NativeContract<'_>,
        edit: impl for<'data> FnOnce(&'data mut T) -> NativeResult<R>,
    ) -> NativeResult<R> {
        value_check::candidate_root(self.runtime, root)?;
        self.poll()?;
        let result = self.with_values(|slots| {
            let (object, payload) = admit::<T>(slots, root, &contract)?;
            if slots
                .scope()
                .read(object, |object| object.revision() == u64::MAX)
            {
                return Ok(Ok(Err(revision_exhausted())));
            }
            NativeObject::advance_revision(slots.scope(), object)?;
            Managed::edit_data(slots.scope(), payload, edit)
        })?;
        match result {
            Ok(result) => result,
            Err(panic) => resume_unwind(panic),
        }
    }
}
