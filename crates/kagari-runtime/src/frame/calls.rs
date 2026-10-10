//! Prepared calls share physical argument admission, dependencies and publication.
use crate::{
    Runtime,
    error::RuntimeError,
    execution_metadata::call_contracts::InterfaceCallSite,
    frame::{ExecutionStack, FrameDispatch, FrameEntry, arguments::FrameArguments},
    module::execution::calls::PreparedCallTarget,
};
use kagari_bytecode::{
    instruction::{BytecodeInstruction, CallTarget},
    module::CallableTarget,
};
use std::slice;

impl ExecutionStack<'_> {
    /// Only the executing sealed instruction selects a call record; external
    /// callers cannot supply a transfer plan or substitute a target generation.
    pub fn push_prepared_call(&self, runtime: &Runtime) -> Result<(), RuntimeError> {
        self.validate_runtime(runtime)?;
        let (owner, function, pc, slots, environment) = {
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
                caller.instruction_offset(),
                caller.slots,
                caller.environment(),
            )
        };
        let call = owner.execution().functions[function.index()]
            .calls
            .get(&pc)
            .ok_or_else(|| runtime.resources().quarantine("missing prepared call"))?;
        let (module, target, shared) = match call.target {
            PreparedCallTarget::Static {
                module,
                target,
                shared,
            } => (module, target, shared),
            PreparedCallTarget::Closure => {
                let BytecodeInstruction::Call {
                    callee:
                        CallTarget::ClosureRegister {
                            register,
                            params,
                            return_type,
                        },
                    ..
                } = &owner.bytecode.functions[function.index()].instructions[pc]
                else {
                    return Err(runtime
                        .resources()
                        .quarantine("missing prepared closure contract"));
                };
                let caller = self.current()?;
                let value = caller.read_register(runtime, *register)?;
                let closure = runtime.resolve_closure(&value)?;
                caller.validate_closure_call(&closure, *register, params, *return_type)?;
                drop(caller);
                return self.push_resolved_closure(
                    runtime,
                    &closure,
                    FrameArguments::frame(slots, &call.arguments),
                    None,
                    Some(call),
                );
            }
            PreparedCallTarget::Interface { index, closed } => {
                let BytecodeInstruction::Call {
                    callee: CallTarget::InterfaceMethod { .. },
                    args,
                    ..
                } = &owner.bytecode.functions[function.index()].instructions[pc]
                else {
                    return Err(runtime
                        .resources()
                        .quarantine("missing prepared interface contract"));
                };
                let (invocation, receiver) = {
                    let caller = self.current()?;
                    let register = args.first().ok_or_else(|| {
                        RuntimeError::module_validation("missing interface receiver")
                    })?;
                    let receiver = caller.read_register(runtime, *register)?;
                    let scoped_call;
                    let call = if closed {
                        caller
                            .closed_calls
                            .as_ref()
                            .and_then(|calls| calls.get(index))
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
                            InterfaceCallSite { function, pc },
                        )?;
                        scoped_call.as_ref()
                    };
                    runtime.resolve_interface_invocation(&caller, call, &receiver)?
                };
                let arguments = FrameArguments::frame(slots, &call.arguments[1..])
                    .with_prefix(slice::from_ref(&receiver))?;
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
        let loaded = owner.member(module).ok_or_else(|| {
            runtime
                .resources()
                .quarantine("invalid prepared script module")
        })?;
        let arguments = FrameArguments::frame(slots, &call.arguments);
        let environment = if shared {
            let BytecodeInstruction::Call {
                callee: CallTarget::Shared { contract, .. },
                ..
            } = &owner.bytecode.functions[function.index()].instructions[pc]
            else {
                return Err(runtime
                    .resources()
                    .quarantine("missing prepared shared contract"));
            };
            let prepared = runtime.prepare_shared_environment(
                &owner,
                environment.clone(),
                &loaded,
                target,
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
                    )
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
