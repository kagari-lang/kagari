//! Native identity and concrete payload access share one scoped source interval.
pub(crate) mod loans;
mod managed;
use crate::{
    error::RuntimeError,
    execution::roots::{SessionId, Slots},
    execution_metadata::{
        applications::AdmittedApplication, managed::applications::ApplicationObject,
    },
    frame::types::arguments::TypeArgument,
    module::ExecutableModule,
    native::{
        binding::NativeResult,
        conversion::context::ConversionContext,
        payload::data::NativeData,
        storage::{NativeStorage, initializer::NativeInit},
    },
    resource::candidate::CandidateOrigin,
    value::{
        carriers::{RootedValue, ScopedValue},
        native::{NativeAllocation, NativeObject, data::DataObject},
        view::ValueView,
    },
    value_check,
};
use kagari_gc::{
    error::GcError,
    object::{AnyObject, Gc, GcObject},
};
use kagari_types::ty::Ty;
use std::{any::Any, fmt::Debug, panic::resume_unwind};

/// Borrow prepared facts; ordinary access adds no descriptor/lease clone.
pub(crate) struct NativeContract<'a> {
    pub(crate) storage: &'a NativeStorage,
    pub(crate) owner: &'a ExecutableModule,
    pub(crate) applied: &'a TypeArgument,
}

impl NativeContract<'_> {
    fn check(&self, object: &NativeObject) -> bool {
        self.storage.matches_descriptor(object.storage())
            && object.matches(self.applied.view(self.owner))
    }
}

impl<'runtime> ConversionContext<'runtime> {
    fn check_native_construction(&self, contract: &NativeContract<'_>) -> NativeResult<()> {
        self.poll()?;
        self.runtime.validate_executable_module(contract.owner)?;
        contract.applied.validate(self.runtime)?;
        let Ty::NativeObject(nominal) = contract.applied.ty() else {
            return Err(RuntimeError::module_validation("native construction type"));
        };
        if !self
            .runtime
            .native_entries
            .storage
            .get_id(nominal.declaration)
            .is_some_and(|installed| installed.same_registration(contract.storage))
        {
            return Err(RuntimeError::module_validation(
                "native construction registration",
            ));
        }
        Ok(())
    }

    pub(crate) fn create_native_data<T: NativeData>(
        &mut self,
        contract: NativeContract<'_>,
        data: T,
    ) -> NativeResult<RootedValue> {
        let value = self.allocate_native_data(contract, data, None)?;
        self.retain(&value)
    }

    /// Fixed data has no script edges. Optional stored selections are admitted
    /// from their actual source before allocating and become a header-owned edge.
    pub(in crate::native) fn allocate_native_data<T: NativeData>(
        &mut self,
        contract: NativeContract<'_>,
        data: T,
        selections: Option<&AdmittedApplication>,
    ) -> NativeResult<ValueView<'runtime>> {
        self.check_native_construction(&contract)?;
        if !contract.storage.is_editable_data() || !contract.storage.accepts_payload::<T>() {
            return Err(RuntimeError::module_validation("native fixed data mapping"));
        }
        let mut frame = self.scratch()?;
        let execution = frame.execution_id()?;
        let origin = frame.runtime.resources.candidate_origin();
        let value = frame.with_values(|slots| {
            let selections = selections.map(|source| source.resolve(slots)).transpose()?;
            let payload = DataObject::alloc(slots.scope(), data)?.erase();
            publish(slots, execution, contract, payload, selections, origin)
        })?;
        frame.preserve(value)
    }

    pub(crate) fn allocate_native_factory(
        &mut self,
        contract: NativeContract<'_>,
        factory: impl FnOnce(&mut ConversionContext<'runtime>) -> NativeResult<Box<dyn Any + Send>>,
    ) -> NativeResult<ValueView<'runtime>> {
        self.check_native_construction(&contract)?;
        // Complete fixed mapping/owner admission before user factory effects.
        // This conversion owns the exclusive activation throughout the callback;
        // no publication can invalidate the prepared contract before allocation.
        let mut frame = self.scratch()?;
        let execution = frame.execution_id()?;
        let data = factory(&mut frame)?;
        let origin = frame.runtime.resources.candidate_origin();
        let value = frame.with_values(|slots| {
            let payload = contract.storage.allocate_data(slots.scope(), data)?;
            publish(slots, execution, contract, payload, None, origin)
        })?;
        frame.preserve(value)
    }

    pub(crate) fn create_native_with<T: GcObject + Debug>(
        &mut self,
        contract: NativeContract<'_>,
        initialize: impl for<'loan, 'source, 'gc> FnOnce(
            &mut NativeInit<'loan, 'source, 'gc>,
        ) -> Result<Gc<'gc, T>, GcError>,
    ) -> NativeResult<RootedValue> {
        let value = self.allocate_native_with(contract, None, initialize)?;
        self.retain(&value)
    }

    /// Construct under a branded noncollecting interval and publish the header's
    /// metadata edge before the enclosing preparation suffix can retire.
    pub(in crate::native) fn allocate_native_with<T: GcObject + Debug>(
        &mut self,
        contract: NativeContract<'_>,
        selections: Option<&AdmittedApplication>,
        initialize: impl for<'loan, 'source, 'gc> FnOnce(
            &mut NativeInit<'loan, 'source, 'gc>,
        ) -> Result<Gc<'gc, T>, GcError>,
    ) -> NativeResult<ValueView<'runtime>> {
        self.check_native_construction(&contract)?;
        if !contract.storage.accepts_traced::<T>() {
            return Err(RuntimeError::module_validation("native traced mapping"));
        }
        let mut frame = self.scratch()?;
        let execution = frame.execution_id()?;
        let origin = frame.runtime.resources.candidate_origin();
        let value = frame.with_values(|slots| {
            let selections = selections.map(|source| source.resolve(slots)).transpose()?;
            let payload = initialize(&mut NativeInit {
                slots,
                _thread: Default::default(),
            })?;
            publish(
                slots,
                execution,
                contract,
                payload.erase(),
                selections,
                origin,
            )
        })?;
        frame.preserve(value)
    }

    pub(crate) fn replace_native_with<T: GcObject + Debug>(
        &mut self,
        target: &RootedValue,
        contract: NativeContract<'_>,
        initialize: impl for<'loan, 'source, 'gc> FnOnce(
            &mut NativeInit<'loan, 'source, 'gc>,
        ) -> Result<Gc<'gc, T>, GcError>,
    ) -> NativeResult<()> {
        value_check::candidate_root(self.runtime, target)?;
        self.poll()?;
        if !contract.storage.accepts_traced::<T>() {
            return Err(RuntimeError::module_validation("native traced mapping"));
        }
        self.with_values(|slots| {
            let value = target.get(slots.scope())?;
            let object = slots
                .scope()
                .downcast::<NativeObject>(value.object().ok_or(GcError::Representation)?)?;
            if !slots.scope().read(object, |object| contract.check(object)) {
                return Err(GcError::Representation);
            }
            if slots
                .scope()
                .read(object, |object| object.revision() == u64::MAX)
            {
                return Ok(Err(revision_exhausted()));
            }
            // Check the actual current Rust payload before constructor effects.
            let old = NativeObject::payload(slots.scope(), object);
            slots.scope().downcast::<T>(old)?;
            let payload = initialize(&mut NativeInit {
                slots,
                _thread: Default::default(),
            })?;
            NativeObject::replace_payload(slots.scope(), object, payload.erase())?;
            Ok(Ok(()))
        })?
    }

    pub(crate) fn check_native_view<T: 'static>(
        &mut self,
        value: &ValueView<'_>,
        contract: NativeContract<'_>,
    ) -> NativeResult<()> {
        value_check::candidate_protected(self.runtime, &[value.reborrow()])?;
        self.with_values(|slots| {
            let value = value.get(slots)?;
            let object = slots
                .scope()
                .downcast::<NativeObject>(value.object().ok_or(GcError::Representation)?)?;
            if !slots.scope().read(object, |object| contract.check(object)) {
                return Err(GcError::Representation);
            }
            let payload = NativeObject::payload(slots.scope(), object);
            contract
                .storage
                .read::<T, _>(slots.scope(), payload, |_| Ok(()))
        })?
    }

    pub(crate) fn read_native_root<T: 'static, R>(
        &mut self,
        root: &RootedValue,
        contract: NativeContract<'_>,
        read: impl for<'data> FnOnce(&'data T) -> NativeResult<R>,
    ) -> NativeResult<R> {
        value_check::candidate_root(self.runtime, root)?;
        self.poll()?;
        self.with_values(|slots| {
            let value = root.get(slots.scope())?;
            let object = slots
                .scope()
                .downcast::<NativeObject>(value.object().ok_or(GcError::Representation)?)?;
            if !slots.scope().read(object, |object| contract.check(object)) {
                return Err(GcError::Representation);
            }
            let payload = NativeObject::payload(slots.scope(), object);
            contract.storage.read(slots.scope(), payload, read)
        })?
    }

    pub(crate) fn edit_native_root<T: 'static, R>(
        &mut self,
        root: &RootedValue,
        contract: NativeContract<'_>,
        edit: impl for<'data> FnOnce(&'data mut T) -> NativeResult<R>,
    ) -> NativeResult<R> {
        value_check::candidate_root(self.runtime, root)?;
        self.poll()?;
        let result = self.with_values(|slots| {
            let value = root.get(slots.scope())?;
            let object = slots
                .scope()
                .downcast::<NativeObject>(value.object().ok_or(GcError::Representation)?)?;
            if !slots.scope().read(object, |object| contract.check(object))
                || !contract.storage.is_editable_data()
                || !contract.storage.accepts_payload::<T>()
            {
                return Err(GcError::Representation);
            }
            let payload = NativeObject::payload(slots.scope(), object);
            if slots
                .scope()
                .read(object, |object| object.revision() == u64::MAX)
            {
                return Ok(Ok(Err(revision_exhausted())));
            }
            // Revision commits before lending data, including error/unwind.
            NativeObject::advance_revision(slots.scope(), object)?;
            contract.storage.edit_data(slots.scope(), payload, edit)
        })?;
        match result {
            Ok(result) => result,
            Err(panic) => resume_unwind(panic),
        }
    }
}

fn publish<'call, 'gc>(
    slots: &mut Slots<'_, 'gc>,
    execution: SessionId,
    contract: NativeContract<'_>,
    payload: Gc<'gc, AnyObject>,
    selections: Option<Gc<'gc, ApplicationObject>>,
    origin: CandidateOrigin,
) -> Result<ValueView<'call>, GcError> {
    let object = NativeObject::alloc(
        slots.scope(),
        NativeAllocation {
            origin,
            storage: contract.storage.descriptor().clone(),
            owner: contract.owner.clone(),
            applied: contract.applied.clone(),
        },
        payload,
        selections,
        &[],
    )?;
    let value = ScopedValue::managed(object);
    let location = slots.push_local(execution, value)?;
    ValueView::protected(value, Some(location))
}

fn revision_exhausted() -> RuntimeError {
    RuntimeError::module_validation("native revision exhausted")
}
