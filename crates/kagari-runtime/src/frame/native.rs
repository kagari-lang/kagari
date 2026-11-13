//! Ordinary native functions finish synchronously using the caller's existing roots.
use crate::{
    Runtime,
    error::{RuntimeError, fault::EngineFault},
    execution::roots::ExecutionRoots,
    execution_metadata::managed::sources::MetadataSource,
    frame::{
        FrameStack, NativeEntryState,
        driver::{ExecutionAction, ReturnAction},
        transfer::ReturnValue,
    },
    module::{linked_execution::LinkedNativeBody, managed::source::RevisionSource},
    native::{
        context::{NativeCall, NativeEntry, ScriptInvoker},
        conversion::{arguments::ArgumentValues, context::ConversionContext},
    },
    value::place::ResultPlace,
};

use kagari_bytecode::module::CallableTarget;

impl FrameStack {
    pub fn start_native_entry(
        &self,
        runtime: &mut Runtime,
        invoke_script: ScriptInvoker,
    ) -> Result<ExecutionAction, RuntimeError> {
        self.validate_top(runtime)?;
        let (loaded, import, roots, environment, layout) = {
            let mut frame = self.current_mut(runtime)?;
            let CallableTarget::Native(import) = frame.target else {
                return Err(RuntimeError::module_validation(
                    "expected native callable frame",
                ));
            };
            if !matches!(frame.native_entry, NativeEntryState::Pending) {
                return Err(RuntimeError::module_validation(
                    "native callable already started",
                ));
            }
            frame.native_entry = NativeEntryState::Running;
            (
                frame.loaded.clone(),
                import,
                frame.slots(),
                frame.admitted_environment().cloned(),
                frame
                    .slot_layout
                    .clone()
                    .ok_or_else(|| RuntimeError::module_validation("native frame slot layout"))?,
            )
        };
        let function = runtime
            .modules
            .native_binding(&loaded, import)
            .ok_or_else(|| RuntimeError::module_validation("unlinked native callable"))?;
        let location = layout.location(0).ok_or_else(|| {
            runtime
                .resources()
                .quarantine(EngineFault::InvalidNativeReturnSlot)
        })?;
        let arguments = layout
            .locations
            .get(1..)
            .ok_or_else(|| RuntimeError::module_validation("native frame parameter locations"))?;
        let args = ArgumentValues::prepared(
            roots.banks.execution,
            roots.banks.managed,
            roots.banks.scalars,
            arguments,
        );
        let invoke = |runtime: &mut Runtime, entry: NativeEntry<'_>| {
            let mut context = NativeCall::new(
                runtime,
                &loaded,
                entry,
                RevisionSource::Execution(roots.body),
                args.reborrow(),
                invoke_script,
            )?;
            let published = context.invoke(ResultPlace::frame(&roots, Some(&location)))?;
            published.admit_frame(&roots, Some(&location))?;
            Ok::<_, RuntimeError>(())
        };
        if loaded.bytecode.native_imports[import.index()]
            .generic
            .is_some()
        {
            let environment = environment
                .ok_or_else(|| RuntimeError::module_validation("shared native environment"))?;
            let mut preparation = ConversionContext::for_selected(
                runtime,
                &loaded,
                roots.banks.execution,
                RevisionSource::Execution(roots.body),
                Some(MetadataSource::AdmittedEnvironment(&environment)),
            )?;
            let mut sources = vec![
                MetadataSource::Revision {
                    owner: &loaded,
                    source: RevisionSource::Execution(roots.body),
                },
                MetadataSource::AdmittedEnvironment(&environment),
            ];
            let application = preparation.runtime_mut().prepare_native_application(
                roots.banks.execution,
                &loaded,
                import,
                environment.facts().clone(),
                &mut sources,
            )?;
            invoke(
                preparation.runtime_mut(),
                NativeEntry::Applied(&application),
            )?;
        } else {
            invoke(runtime, NativeEntry::Closed(&function))?;
        }
        let mut frame = self.current_mut(runtime)?;
        frame.native_entry = NativeEntryState::Complete;
        Ok(ExecutionAction::NativeReturn)
    }

    /// Consume a completed native frame at an explicit return transition.
    pub fn finish_native_return(
        &self,
        runtime: &mut Runtime,
    ) -> Result<ReturnAction, RuntimeError> {
        self.validate_top(runtime)?;
        let (frame_slots, location) = {
            let frame = self.current(runtime)?;
            if !matches!(frame.native_entry, NativeEntryState::Complete) {
                return Err(runtime
                    .resources()
                    .quarantine(EngineFault::NativeReturnBeforeCompletion));
            }
            let location = frame
                .slot_layout
                .as_ref()
                .and_then(|layout| layout.location(0))
                .ok_or_else(|| {
                    runtime
                        .resources()
                        .quarantine(EngineFault::InvalidNativeReturnSlot)
                })?;
            (frame.slots(), location)
        };
        let packet = runtime
            .memory
            .scope(&runtime.resources, |cx, roots| {
                ExecutionRoots::with(roots, cx, |slots| {
                    let value = frame_slots.read(slots, location)?;
                    ReturnValue::protect(slots, self.session, value)
                })
            })
            .map_err(|error| runtime.managed_memory_error(error))?;
        self.finish_return(runtime, packet)
    }

    pub(super) fn invoke_prepared_native(
        &self,
        runtime: &mut Runtime,
        index: usize,
        invoke_script: ScriptInvoker,
    ) -> Result<(), RuntimeError> {
        // execute_region admitted this runtime/session; no callback or collection
        // intervened. Retain the linked descriptor and roots, releasing all frame
        // and bank borrows before invoking arbitrary Rust or reentrant script.
        let (loaded, roots, links) = {
            let frame = self.current(runtime)?;
            (frame.loaded.clone(), frame.slots(), frame.links.clone())
        };
        let operation = links
            .as_ref()
            .and_then(|links| links.native_call(index))
            .ok_or_else(|| {
                runtime
                    .resources()
                    .quarantine(EngineFault::MissingPreparedNativeCall)
            })?;
        let LinkedNativeBody::Callback(callback) = &operation.body else {
            return Err(runtime
                .resources()
                .quarantine(EngineFault::SealedNativeBodyReachedCallbackTransition));
        };
        let mut context = NativeCall::new(
            runtime,
            &loaded,
            NativeEntry::Closed(&callback.function),
            RevisionSource::Execution(roots.body),
            ArgumentValues::prepared(
                roots.banks.execution,
                roots.banks.managed,
                roots.banks.scalars,
                &callback.arguments,
            ),
            invoke_script,
        )?;
        let published =
            context.invoke(ResultPlace::frame(&roots, operation.destination.as_ref()))?;
        published.admit_frame(&roots, operation.destination.as_ref())?;
        drop(context);
        let mut frame = self.current_mut(runtime)?;
        frame.completed = frame.executing;
        Ok(())
    }
}
