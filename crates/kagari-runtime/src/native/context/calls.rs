//! Selected Native callbacks share source packs and caller-owned publication.
use crate::{
    error::RuntimeError,
    execution_metadata::{
        applications::ApplicationRecord,
        managed::{ProtectedMetadata, sources::MetadataSource},
    },
    native::{
        arguments::CallArguments,
        binding::NativeResult,
        context::{LinkedCallable, NativeCall, NativeEntry},
        conversion::{arguments::ArgumentValues, context::ConversionContext},
        scalar::NativeScalar,
    },
    value::view::ValueView,
};
use kagari_bytecode::module::CallableTarget;
use kagari_types::ty::Ty;
use std::slice;

impl<'call> NativeCall<'call> {
    /// Invoke relationship evidence retained by the actual Native argument.
    /// Original arguments are admitted before selection grows any source suffix.
    pub fn call_stored_values(
        &mut self,
        argument: usize,
        slot: usize,
        values: &[ValueView<'_>],
    ) -> NativeResult<ValueView<'call>> {
        self.poll()?;
        let receiver = self.argument(argument)?;
        let invoke = self.invoke_script;
        let execution = self.pending.execution();
        let mut preparation = ConversionContext::in_native_call(self)?;
        let arguments = preparation.argument_values(values)?;
        let source = MetadataSource::NativeSelections(
            receiver
                .location()
                .ok_or_else(|| RuntimeError::module_validation("stored callable receiver"))?,
        );
        let record = preparation
            .runtime_mut()
            .with_metadata_graph(&[source], |cx, graph, _| {
                let Some(ProtectedMetadata::Application(value)) = graph.single_root() else {
                    return Err(RuntimeError::module_validation("stored Native metadata").into());
                };
                Ok(cx.read(value, |value| value.record().clone()))
            })?;
        let ApplicationRecord::StoredSelections(selections) = record.as_ref() else {
            return Err(RuntimeError::module_validation(
                "stored Native selection record",
            ));
        };
        let target = selections
            .get(slot)
            .ok_or_else(|| RuntimeError::module_validation("stored callable slot"))?;
        let selected = target.retain(preparation.runtime_mut(), execution, &[source])?;
        let value = {
            let mut conversion = ConversionContext::for_selected(
                preparation.runtime_mut(),
                &selected.owner,
                execution,
                selected.origin,
                selected.metadata(),
            )?;
            let value = selected
                .target
                .call_source(&mut conversion, invoke, arguments)?;
            conversion.into_native_value(execution, value)?
        };
        let value = value.reborrow();
        let value = preparation.into_native_value(execution, value)?;
        Ok(value.reborrow())
    }

    /// Protect original arguments before selection can grow dependency storage.
    /// Both preparation and execution belong to this call's conversion suffix.
    pub fn call_values(
        &mut self,
        target: &LinkedCallable,
        arguments: &[ValueView<'_>],
    ) -> NativeResult<ValueView<'call>> {
        self.poll()?;
        let caller = self.owner;
        let invoke = self.invoke_script;
        let execution = self.pending.execution();
        let mut preparation = ConversionContext::in_native_call(self)?;
        let arguments = preparation.argument_values(arguments)?;
        let revision = MetadataSource::Revision {
            owner: caller,
            source: preparation.owner_origin(),
        };
        let pair = preparation
            .owner_metadata()
            .map(|metadata| [revision, metadata]);
        let protected = pair
            .as_ref()
            .map_or(slice::from_ref(&revision), |pair| pair.as_slice());
        let selected =
            target.select_current(preparation.runtime_mut(), caller, execution, protected)?;
        let value = {
            let mut conversion = ConversionContext::for_selected(
                preparation.runtime_mut(),
                &selected.owner,
                execution,
                selected.origin,
                selected.metadata(),
            )?;
            let value = selected
                .target
                .call_source(&mut conversion, invoke, arguments)?;
            conversion.into_native_value(execution, value)?
        };
        let value = value.reborrow();
        let value = preparation.into_native_value(execution, value)?;
        Ok(value.reborrow())
    }

    pub fn call<R: NativeScalar, A: CallArguments>(
        &mut self,
        target: &LinkedCallable,
        arguments: A,
    ) -> NativeResult<R> {
        self.poll()?;
        let caller = self.owner;
        let invoke_script = self.invoke_script;
        let execution = self.pending.execution();
        let mut preparation = ConversionContext::in_native_call(self)?;
        let revision = MetadataSource::Revision {
            owner: caller,
            source: preparation.owner_origin(),
        };
        let pair = preparation
            .owner_metadata()
            .map(|metadata| [revision, metadata]);
        let protected = pair
            .as_ref()
            .map_or(slice::from_ref(&revision), |pair| pair.as_slice());
        let selected =
            target.select_current(preparation.runtime_mut(), caller, execution, protected)?;
        let target = &selected.target;
        if Ty::Builtin(R::abi_type()) != target.result || !A::matches(target.params.iter()) {
            return Err(RuntimeError::module_validation(
                "native callback conversion differs from its declared result",
            ));
        }
        arguments.with_values(|arguments| {
            let mut conversion = ConversionContext::for_selected(
                preparation.runtime_mut(),
                &selected.owner,
                execution,
                selected.origin,
                selected.metadata(),
            )?;
            if target.primitive.is_none()
                && target.environment.is_none()
                && let CallableTarget::Native(import) = target.target
            {
                let function = conversion
                    .runtime()
                    .modules
                    .native_binding(&selected.owner, import)
                    .ok_or_else(|| RuntimeError::module_validation("unlinked native callback"))?;
                let (location, destination) = conversion.result_place()?;
                let mut context = NativeCall::new(
                    conversion.runtime_mut(),
                    &selected.owner,
                    NativeEntry::Closed(&function),
                    selected.origin,
                    ArgumentValues::scalars(arguments),
                    invoke_script,
                )?;
                let published = context.invoke(destination)?;
                drop(context);
                let value = conversion.published_result(published, location)?;
                return R::decode(value);
            }
            let value = target.call_source(
                &mut conversion,
                invoke_script,
                ArgumentValues::scalars(arguments),
            )?;
            R::decode(value)
        })
    }
}
