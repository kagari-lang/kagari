//! Prepared calls share physical argument admission, dependencies and publication.
#[cfg(feature = "execution-diagnostics")]
use crate::diagnostics::{self, Event};
use crate::{
    Runtime,
    error::{RuntimeError, fault::EngineFault},
    execution::roots::{ExecutionRoots, LocalMark},
    execution_metadata::{call_contracts::InterfaceCallSite, managed::sources::MetadataSource},
    frame::{
        FrameDispatch, FrameEntry, FrameEnvironment, FrameStack,
        arguments::FrameArguments,
        driver::ExecutionAction,
        native::NativeCallSite,
        types::{bindings::TypeBindings, compatibility::TypeView},
    },
    module::{
        ExecutableModule, execution::calls::PreparedCallTarget,
        linked_execution::calls::LinkedCall, managed::source::RevisionSource,
    },
    native::{context::ScriptInvoker, conversion::context::ConversionContext},
    value::{EphemeralValue, host_values::EphemeralObject, kind::ObjectKind},
    value_check::scoped::{self, CheckError},
};

use kagari_bytecode::module::CallableTarget;
use kagari_contract::callable::shared::SharedCall;
use kagari_gc::error::GcError;

pub(super) enum PreparedCallOutcome {
    Frame(ExecutionAction),
    NativeComplete,
}

impl FrameStack {
    /// Only the executing sealed instruction selects a call record; external
    /// callers cannot supply a transfer plan or substitute a target generation.
    pub(super) fn execute_prepared_call(
        &self,
        runtime: &mut Runtime,
        index: usize,
        invoke_script: ScriptInvoker,
    ) -> Result<PreparedCallOutcome, RuntimeError> {
        self.validate_top(runtime)?;
        let mut preparation = None;
        let result =
            self.execute_prepared_call_inner(runtime, index, invoke_script, &mut preparation);
        let result = match result {
            Ok(PreparedCallOutcome::Frame(action)) if preparation.is_some() => {
                let Runtime {
                    memory, resources, ..
                } = &mut *runtime;
                let adopted = (|| -> Result<(), GcError> {
                    let mut frames =
                        resources
                            .sessions
                            .frames_mut(self.session)
                            .ok_or(GcError::Storage(
                                "frame control is borrowed during preparation adoption",
                            ))?;
                    let storage = frames
                        .last_mut()
                        .and_then(|frame| frame.storage.as_mut())
                        .ok_or(GcError::Storage("call preparation has no callee"))?;
                    memory.scope(resources, |cx, roots| {
                        ExecutionRoots::with(roots, cx, |slots| {
                            slots.adopt_frame_preparation(storage.mark_mut(), &mut preparation)
                        })
                    })
                })();
                match adopted {
                    Ok(()) => Ok(PreparedCallOutcome::Frame(action)),
                    Err(error) => {
                        self.pop(runtime)?;
                        Err(runtime.managed_memory_error(error))
                    }
                }
            }
            Ok(outcome) => Ok(outcome),
            Err(error) => Err(error),
        };
        if let Some(prepared) = preparation {
            runtime
                .memory
                .scope(&runtime.resources, |cx, roots| {
                    ExecutionRoots::with(roots, cx, |slots| slots.retire_locals(prepared))
                })
                .map_err(|error| {
                    runtime
                        .resources
                        .quarantine(EngineFault::ExecutionStorageRetirementFailed(error))
                })?;
        }
        result
    }

    /// Only semantic preparation can append a temporary source prefix. Ordinary
    /// static calls lend their caller banks directly; their callee/Native protocol
    /// owns all new storage, so they need no additional local mark or adoption.
    fn begin_call_preparation(
        &self,
        runtime: &mut Runtime,
        preparation: &mut Option<LocalMark>,
    ) -> Result<(), RuntimeError> {
        debug_assert!(preparation.is_none());
        let mark = runtime
            .memory
            .scope(&runtime.resources, |cx, roots| {
                ExecutionRoots::with(roots, cx, |slots| slots.mark_locals(self.session))
            })
            .map_err(|error| runtime.managed_memory_error(error))?;
        *preparation = Some(mark);
        Ok(())
    }

    fn execute_prepared_call_inner(
        &self,
        runtime: &mut Runtime,
        index: usize,
        invoke_script: ScriptInvoker,
        preparation: &mut Option<LocalMark>,
    ) -> Result<PreparedCallOutcome, RuntimeError> {
        #[cfg(feature = "execution-diagnostics")]
        diagnostics::record(Event::CallSelection);
        // execute_region admitted this runtime/session and selected the ordinal.
        // Ending its cursor borrows cannot change authority: no callback,
        // suspension or collection occurs before this private transition.
        // Callee availability, dynamic values and depth remain checked below.
        let (owner, function, slots, environment, links) = {
            let frames = self.frames(runtime)?;
            let caller = frames
                .last()
                .filter(|_| frames.len() > self.base)
                .ok_or_else(|| {
                    runtime
                        .resources()
                        .quarantine(EngineFault::MissingExecutionFrame)
                })?;
            let CallableTarget::Script(function) = caller.target else {
                return Err(runtime
                    .resources()
                    .quarantine(EngineFault::ScriptCallRequiresAScriptCaller));
            };
            (
                caller.loaded.clone(),
                function,
                caller.slots(),
                caller.admitted_environment().cloned(),
                caller.links.clone(),
            )
        };
        let call = owner.execution().functions[function.index()]
            .calls
            .get(index)
            .ok_or_else(|| {
                runtime
                    .resources()
                    .quarantine(EngineFault::MissingPreparedCall)
            })?;
        let (route, shared) = match &call.target {
            PreparedCallTarget::Static { .. } => {
                let Some(route) = links
                    .as_ref()
                    .and_then(|links| links.call_contract(index))
                    .and_then(LinkedCall::direct)
                else {
                    return Err(runtime
                        .resources()
                        .quarantine(EngineFault::MissingLinkedDirectRoute));
                };
                (route, None)
            }
            PreparedCallTarget::Shared { .. } => {
                self.begin_call_preparation(runtime, preparation)?;
                let Some(LinkedCall::Shared { route, contract }) =
                    links.as_ref().and_then(|links| links.call_contract(index))
                else {
                    return Err(runtime
                        .resources()
                        .quarantine(EngineFault::MissingLinkedSharedContract));
                };
                (route, Some(contract.as_ref()))
            }
            PreparedCallTarget::Closure(contract) => {
                self.begin_call_preparation(runtime, preparation)?;
                let location = self.current(runtime)?.slot_location(
                    runtime.resources(),
                    contract.register,
                    true,
                )?;
                let value = slots
                    .inspect(&runtime.memory, location)
                    .map_err(|error| runtime.managed_memory_error(error))?;
                let closure = runtime.resolve_closure(&value)?;
                self.current(runtime)?.validate_closure_call(
                    &closure,
                    contract,
                    links.as_ref().and_then(|links| links.call_contract(index)),
                )?;
                let selected = runtime.select_closure_call(self.session, &value)?;
                return self
                    .push_selected_closure(
                        runtime,
                        &selected,
                        FrameArguments::frame(slots, &call.arguments),
                        None,
                        Some(call),
                    )
                    .map(PreparedCallOutcome::Frame);
            }
            PreparedCallTarget::Interface {
                receiver, closed, ..
            } => {
                self.begin_call_preparation(runtime, preparation)?;
                let receiver = receiver
                    .map(|register| {
                        let location = self.current(runtime)?.slot_location(
                            runtime.resources(),
                            register,
                            true,
                        )?;
                        slots
                            .inspect(&runtime.memory, location)
                            .map_err(|error| runtime.managed_memory_error(error))
                    })
                    .transpose()?;
                let mut protected = Vec::new();
                protected
                    .try_reserve(2)
                    .map_err(|_| RuntimeError::resource_limit("interface call sources"))?;
                protected.push(MetadataSource::Revision {
                    owner: &owner,
                    source: RevisionSource::Execution(slots.body),
                });
                if let Some(environment) = &environment {
                    protected.push(MetadataSource::AdmittedEnvironment(environment));
                }
                let scoped = if *closed {
                    let prepared = links
                        .as_ref()
                        .and_then(|links| links.call(index))
                        .cloned()
                        .ok_or_else(|| {
                            RuntimeError::module_validation("invalid linked interface ordinal")
                        })?;
                    if !runtime.interface_call_is_protected(&protected, &prepared)? {
                        return Err(RuntimeError::module_validation(
                            "closed interface call lost its linked dependencies",
                        ));
                    }
                    prepared
                } else {
                    runtime.prepare_scoped_interface_call(
                        self.session,
                        &owner,
                        environment
                            .as_ref()
                            .map(|environment| environment.facts().clone())
                            .ok_or_else(|| {
                                RuntimeError::module_validation("missing interface call scope")
                            })?,
                        InterfaceCallSite {
                            function,
                            ordinal: index,
                        },
                        &mut protected,
                    )?
                };
                let invocation = {
                    let mut conversion = ConversionContext::for_selected(
                        runtime,
                        &owner,
                        self.session,
                        RevisionSource::Execution(slots.body),
                        environment
                            .as_ref()
                            .map(MetadataSource::AdmittedEnvironment),
                    )?;
                    let invocation = conversion.prepare_interface_invocation(
                        &scoped,
                        receiver.as_ref(),
                        protected,
                    )?;
                    conversion.into_method_invocation(invocation)?
                };
                let computation = invocation.computation().ok_or_else(|| {
                    RuntimeError::module_validation("interface computation is not admitted")
                })?;
                let metadata = MetadataSource::AdmittedApplication(computation);
                let arguments = if invocation.interface_receiver() {
                    let explicit = call.arguments.get(1..).ok_or_else(|| {
                        RuntimeError::module_validation("interface receiver argument")
                    })?;
                    FrameArguments::frame(slots, explicit)
                        .with_method_receiver(computation)
                        .map_err(|error| runtime.managed_memory_error(error))?
                } else {
                    FrameArguments::frame(slots, &call.arguments)
                };
                let view = invocation.view(runtime, &[metadata])?;
                view.validate_arguments(runtime, arguments)?;
                let loaded = view.implementation().clone();
                let target = view.target();
                let environment = view.environment().map(|facts| FrameEnvironment {
                    facts: facts.clone(),
                    source: metadata,
                });
                return self
                    .push_admitted_arguments(
                        runtime,
                        loaded,
                        target,
                        arguments,
                        None,
                        FrameDispatch {
                            origin: RevisionSource::Application(computation.source()),
                            metadata: Some(metadata),
                            prepared: Some(call),
                            entry: FrameEntry::Call,
                            invocation: Some(invocation.clone()),
                            environment,
                        },
                    )
                    .map(PreparedCallOutcome::Frame);
            }
        };
        let arguments = FrameArguments::frame(slots, &call.arguments);
        let (loaded, target, environment) = if let Some(contract) = shared {
            let selected = runtime.select_prepared_call(&route.selected)?;
            let mut protected = Vec::new();
            protected
                .try_reserve(3)
                .map_err(|_| RuntimeError::resource_limit("shared call sources"))?;
            protected.push(MetadataSource::Revision {
                owner: &owner,
                source: RevisionSource::Execution(slots.body),
            });
            protected.push(MetadataSource::Revision {
                owner: selected.owner(),
                source: runtime.selection_origin(selected.owner())?,
            });
            if let Some(environment) = &environment {
                protected.push(MetadataSource::AdmittedEnvironment(environment));
            }
            let prepared = runtime.prepare_selected_shared_environment(
                self.session,
                &owner,
                environment
                    .as_ref()
                    .map(|environment| environment.facts().clone()),
                &selected,
                contract,
                &mut protected,
            )?;
            if !shared_arguments(
                runtime,
                arguments,
                &owner,
                environment
                    .as_ref()
                    .map(|environment| environment.facts().types.as_ref()),
                contract,
            )? {
                return Err(RuntimeError::module_validation("invalid shared call entry"));
            }
            drop(protected);
            let (loaded, target) = selected.into_entry();
            (loaded, target, Some(prepared))
        } else {
            let (loaded, target) = runtime.select_prepared_entry(&route.selected)?;
            (loaded, target, None)
        };
        let origin = runtime.selection_origin(&loaded)?;
        if let CallableTarget::Native(import) = target
            && environment.is_none()
            && !self.session_state(runtime).observer_attached.get()
            && loaded
                .execution()
                .callee(target)
                .is_some_and(|callee| !callee.generic && !callee.may_suspend)
        {
            self.invoke_selected_native(
                runtime,
                NativeCallSite {
                    owner: &loaded,
                    import,
                    caller: slots,
                    call,
                    origin,
                },
                invoke_script,
            )?;
            return Ok(PreparedCallOutcome::NativeComplete);
        }
        self.push_admitted_arguments(
            runtime,
            loaded,
            target,
            arguments,
            None,
            FrameDispatch {
                origin,
                metadata: None,
                prepared: Some(call),
                entry: FrameEntry::Call,
                invocation: None,
                environment: environment.map(FrameEnvironment::admitted),
            },
        )
        .map(PreparedCallOutcome::Frame)
    }
}

/// Shared-entry policy stays outside the generic bank copy loop. The matcher
/// consumes original protected values and lexical caller scopes in one closed read.
fn shared_arguments(
    runtime: &mut Runtime,
    arguments: FrameArguments<'_, '_>,
    owner: &ExecutableModule,
    environment: Option<&TypeBindings>,
    contract: &SharedCall,
) -> Result<bool, RuntimeError> {
    if arguments.len() != contract.signature.params.len() {
        return Ok(false);
    }
    let Runtime {
        memory,
        host,
        host_borrows,
        resources,
        ..
    } = runtime;
    let checked = memory.scope(resources, |cx, roots| {
        ExecutionRoots::with(roots, cx, |slots| {
            Ok((|| -> Result<bool, CheckError> {
                arguments.check_bounds(slots)?;
                for (index, expected) in contract.signature.params.iter().enumerate() {
                    let value = arguments.get(slots, index)?;
                    let expected = TypeView::new(expected, owner, environment);
                    if scoped::matches(
                        slots.scope(),
                        value,
                        expected,
                        host,
                        host_borrows,
                        resources,
                    )? {
                        continue;
                    }
                    let Some(access) = contract.receiver_access.filter(|_| index == 0) else {
                        return Ok(false);
                    };
                    if value.kind() != Some(ObjectKind::Ephemeral)
                        || value.as_runtime_ephemeral().is_some()
                    {
                        return Ok(false);
                    }
                    let object = slots.scope().downcast::<EphemeralObject>(
                        value.object().ok_or(GcError::Representation)?,
                    )?;
                    let EphemeralValue::HostView(view) =
                        slots.scope().read(object, EphemeralObject::value)
                    else {
                        return Ok(false);
                    };
                    if !host_borrows.scoped_matches_view(view, expected, access) {
                        return Ok(false);
                    }
                }
                Ok(true)
            })())
        })
    });
    checked
        .map_err(|error| runtime.managed_memory_error(error))?
        .map_err(|error| {
            let error = error.into_runtime(runtime);
            runtime.resources.admit_execution_error(error)
        })
}
