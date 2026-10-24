//! Prepared calls share physical argument admission, dependencies and publication.
#[cfg(feature = "execution-diagnostics")]
use crate::diagnostics::{self, Event};

use crate::{
    Runtime,
    error::RuntimeError,
    execution_metadata::call_contracts::InterfaceCallSite,
    frame::{
        ExecutionStack, FrameDispatch, FrameEntry, arguments::FrameArguments,
        driver::ExecutionAction,
    },
    module::{execution::calls::PreparedCallTarget, linked_execution::calls::LinkedCall},
};
use kagari_bytecode::module::CallableTarget;
use std::slice;

impl ExecutionStack<'_> {
    /// Only the executing sealed instruction selects a call record; external
    /// callers cannot supply a transfer plan or substitute a target generation.
    pub(super) fn push_prepared_call(
        &self,
        runtime: &Runtime,
        index: usize,
    ) -> Result<ExecutionAction, RuntimeError> {
        #[cfg(feature = "execution-diagnostics")]
        diagnostics::record(Event::CallSelection);
        // execute_region admitted this runtime/session and selected the ordinal.
        // Ending its cursor borrows cannot change authority: no callback,
        // suspension or collection occurs before this private transition.
        // Callee availability, dynamic values and depth remain checked below.
        let (owner, function, slots, environment, links) = {
            let frames = self.frames()?;
            let caller = frames
                .last()
                .filter(|_| frames.len() > self.base)
                .ok_or_else(|| runtime.resources().quarantine("missing execution frame"))?;
            let CallableTarget::Script(function) = caller.target else {
                return Err(runtime
                    .resources()
                    .quarantine("script call requires a script caller"));
            };
            (
                caller.loaded.clone(),
                function,
                caller.slots,
                caller.environment(),
                caller.links.clone(),
            )
        };
        let call = owner.execution().functions[function.index()]
            .calls
            .get(index)
            .ok_or_else(|| runtime.resources().quarantine("missing prepared call"))?;
        let (route, shared) = match &call.target {
            PreparedCallTarget::Static { .. } => {
                let Some(LinkedCall::Direct(route)) =
                    links.as_ref().and_then(|links| links.call_contract(index))
                else {
                    return Err(runtime
                        .resources()
                        .quarantine("missing linked direct route"));
                };
                (route, None)
            }
            PreparedCallTarget::Shared { .. } => {
                let Some(LinkedCall::Shared { route, contract }) =
                    links.as_ref().and_then(|links| links.call_contract(index))
                else {
                    return Err(runtime
                        .resources()
                        .quarantine("missing linked shared contract"));
                };
                (route, Some(contract.as_ref()))
            }
            PreparedCallTarget::Closure(contract) => {
                let caller = self.current()?;
                let value = caller.read_slot(runtime, contract.register)?;
                let closure = runtime.resolve_closure(&value)?;
                caller.validate_closure_call(
                    &closure,
                    contract,
                    links.as_ref().and_then(|links| links.call_contract(index)),
                )?;
                drop(caller);
                return self.push_resolved_closure(
                    runtime,
                    &closure,
                    FrameArguments::frame(slots, &call.arguments),
                    None,
                    Some(call),
                );
            }
            PreparedCallTarget::Interface {
                receiver, closed, ..
            } => {
                let (invocation, receiver) = {
                    let caller = self.current()?;
                    let receiver = receiver
                        .map(|register| caller.read_slot(runtime, register))
                        .transpose()?;
                    let scoped_call;
                    let call = if *closed {
                        caller
                            .links
                            .as_ref()
                            .and_then(|links| links.call(index))
                            .map(|call| call.as_ref())
                            .ok_or_else(|| {
                                RuntimeError::module_validation("invalid linked interface ordinal")
                            })?
                    } else {
                        scoped_call = runtime.prepare_scoped_interface_call(
                            &owner,
                            environment.clone().ok_or_else(|| {
                                RuntimeError::module_validation("missing interface call scope")
                            })?,
                            InterfaceCallSite {
                                function,
                                ordinal: index,
                            },
                        )?;
                        scoped_call.as_ref()
                    };
                    runtime.resolve_interface_invocation(&caller, call, receiver.as_ref())?
                };
                let arguments = if let Some(receiver) = receiver.as_ref() {
                    FrameArguments::frame(slots, &call.arguments[1..])
                        .with_prefix(slice::from_ref(receiver))?
                } else {
                    FrameArguments::frame(slots, &call.arguments)
                };
                let invocation = invocation.admit(runtime)?;
                let view = invocation.view(runtime)?;
                view.validate_arguments(runtime, arguments)?;
                let loaded = view.implementation().clone();
                let target = view.target();
                let environment = view.environment().cloned();
                drop(view);
                // Preparation cannot collect or invoke user code. The caller keeps
                // the receiver alive until the callee publishes its own dependencies.
                return self.push_admitted_arguments(
                    runtime,
                    loaded,
                    target,
                    arguments,
                    None,
                    FrameDispatch {
                        prepared: Some(call),
                        entry: FrameEntry::Call,
                        invocation: Some(invocation),
                        environment,
                    },
                );
            }
        };
        let selected = runtime.select_call_route(route)?;
        let loaded = selected.owner().clone();
        let target = selected.target();
        let arguments = FrameArguments::frame(slots, &call.arguments);
        let environment = if let Some(contract) = shared {
            let prepared = runtime.prepare_selected_shared_environment(
                &owner,
                environment.clone(),
                &selected,
                contract,
            )?;
            if arguments.len() != contract.signature.params.len()
                || !arguments.all(runtime, |index, value| {
                    runtime.matches_type_in(
                        value,
                        &contract.signature.params[index],
                        &owner,
                        environment
                            .as_ref()
                            .map(|environment| environment.types.as_ref()),
                    ) || (index == 0
                        && contract.receiver_access.is_some_and(|access| {
                            runtime.matches_scoped_receiver(
                                value,
                                &contract.signature.params[index],
                                &owner,
                                environment
                                    .as_ref()
                                    .map(|environment| environment.types.as_ref()),
                                access,
                            )
                        }))
                })?
            {
                return Err(RuntimeError::module_validation("invalid shared call entry"));
            }
            Some(prepared)
        } else {
            None
        };
        self.push_admitted_arguments(
            runtime,
            loaded,
            target,
            arguments,
            None,
            FrameDispatch {
                prepared: Some(call),
                entry: FrameEntry::Call,
                invocation: None,
                environment,
            },
        )
    }
}
