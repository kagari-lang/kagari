//! Ordinary native functions finish synchronously using the caller's existing roots.
#[cfg(feature = "execution-diagnostics")]
use crate::diagnostics::{self, Event};
use crate::{
    Runtime,
    error::{RuntimeError, fault::EngineFault},
    execution::roots::ExecutionRoots,
    execution_metadata::managed::sources::MetadataSource,
    frame::{
        FrameStack, NativeEntryState,
        arguments::FrameArguments,
        driver::{ExecutionAction, ReturnAction},
        transfer::ReturnValue,
        values::{FrameAllocationError, FrameSlots},
    },
    module::{ExecutableModule, execution::calls::PreparedCall, managed::source::RevisionSource},
    native::{
        context::{NativeCall, NativeEntry, ScriptInvoker},
        conversion::{arguments::ArgumentValues, context::ConversionContext},
    },
    session::native::NativeActivation,
    value::place::ResultPlace,
    value_check,
};

use kagari_bytecode::{instruction::NativeImportId, module::CallableTarget};

pub(super) struct NativeCallSite<'site> {
    pub(super) owner: &'site ExecutableModule,
    pub(super) import: NativeImportId,
    pub(super) caller: FrameSlots,
    pub(super) call: &'site PreparedCall,
    pub(super) origin: RevisionSource<'site>,
}

impl FrameStack {
    pub(super) fn invoke_selected_native(
        &self,
        runtime: &mut Runtime,
        site: NativeCallSite<'_>,
        invoke_script: ScriptInvoker,
    ) -> Result<(), RuntimeError> {
        #[cfg(feature = "execution-diagnostics")]
        diagnostics::record(Event::CallAdmission);
        let NativeCallSite {
            owner,
            import,
            caller,
            call,
            origin,
        } = site;
        let arguments = FrameArguments::frame(caller, &call.arguments);
        value_check::candidate_arguments(runtime, arguments)?;
        let depth = self.frames(runtime)?.len();
        let function = runtime
            .modules
            .native_binding(owner, import)
            .ok_or_else(|| RuntimeError::module_validation("unlinked selected Native callback"))?;
        let callee = owner
            .execution()
            .callee(CallableTarget::Native(import))
            .ok_or_else(|| {
                runtime
                    .resources
                    .quarantine(EngineFault::InvalidPreparedCalleeLayout)
            })?;
        NativeActivation::enter(runtime, owner.clone(), import, depth)?.invoke(
            |runtime| {
                runtime
                    .memory
                    .scope(&runtime.resources, |cx, roots| {
                        ExecutionRoots::with(roots, cx, |slots| {
                            Ok(FrameSlots::validate_arguments(
                                slots,
                                &callee.slot_layout,
                                &callee.parameter_slots,
                                arguments,
                                &runtime.resources,
                            ))
                        })
                    })
                    .map_err(|error| runtime.managed_memory_error(error))?
                    .map_err(|error| match error {
                        FrameAllocationError::Storage(error) => runtime.managed_memory_error(error),
                        FrameAllocationError::Execution(error) => error,
                    })
            },
            |runtime| {
                runtime.gc_safepoint()?;
                let mut context = NativeCall::new(
                    runtime,
                    owner,
                    NativeEntry::Closed(&function),
                    origin,
                    ArgumentValues::prepared(
                        caller.banks.execution,
                        caller.banks.managed,
                        caller.banks.scalars,
                        &call.arguments,
                    ),
                    invoke_script,
                )?;
                let published =
                    context.invoke(ResultPlace::frame(&caller, call.destination.as_ref()))?;
                published.admit_frame(&caller, call.destination.as_ref())?;
                Ok(())
            },
        )?;
        let mut frame = self.current_mut(runtime)?;
        frame.completed = frame.executing;
        Ok(())
    }

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
}
