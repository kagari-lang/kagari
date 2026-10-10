//! Enter a shared function using the active, verified call instruction.
#[cfg(feature = "execution-diagnostics")]
use crate::diagnostics::{self, Event};

use crate::{
    Runtime,
    error::RuntimeError,
    frame::{
        ExecutionStack, FrameDispatch, FrameEntry,
        arguments::FrameArguments,
        types::{EnvironmentRecord, TypeEnvironment},
    },
    module::LoadedModule,
    native::context::LinkedCallable,
    value::Value,
};
use kagari_bytecode::{
    instruction::{BytecodeInstruction, CallTarget, Register},
    module::CallableTarget,
};
use kagari_common::identity::table::DefinitionId;
use kagari_contract::callable::shared::SharedCall;

impl ExecutionStack<'_> {
    pub fn push_selected_call(
        &self,
        runtime: &Runtime,
        caller: &LoadedModule,
        selected: &LinkedCallable,
        args: &[Value],
    ) -> Result<(), RuntimeError> {
        let owner = selected.owner(caller)?;
        runtime.validate_loaded_module(&owner)?;
        if args.len() != selected.params.len()
            || !args
                .iter()
                .enumerate()
                .all(|(index, value)| selected.matches_argument(runtime, &owner, index, value))
        {
            return Err(RuntimeError::module_validation("selected call arguments"));
        }
        self.push_arguments(
            runtime,
            owner,
            selected.target,
            FrameArguments::plain(args),
            None,
            FrameDispatch {
                prepared: None,
                entry: FrameEntry::Call,
                interface_method: None,
                environment: selected.environment.clone(),
            },
        )
    }

    pub fn push_shared_call(
        &self,
        runtime: &Runtime,
        args: &[Value],
        return_dst: Option<Register>,
    ) -> Result<(), RuntimeError> {
        self.validate_runtime(runtime)?;
        let invalid = || RuntimeError::module_validation("invalid shared call entry");
        let (loaded, target, environment) = {
            let caller = self.current()?;
            let Some(BytecodeInstruction::Call {
                dst,
                callee:
                    CallTarget::Shared {
                        module,
                        target,
                        contract,
                    },
                ..
            }) = caller
                .function()
                .and_then(|function| function.instructions.get(caller.instruction_offset()))
            else {
                return Err(invalid());
            };
            if *dst != return_dst {
                return Err(invalid());
            }
            let loaded = caller.loaded().member(*module).ok_or_else(invalid)?;
            runtime.validate_loaded_module(&loaded)?;
            let environment = runtime.prepare_shared_environment(
                caller.loaded(),
                caller.environment(),
                &loaded,
                *target,
                contract,
            )?;
            if args.len() != contract.signature.params.len() {
                return Err(invalid());
            }
            for (value, ty) in args.iter().zip(&contract.signature.params) {
                if !runtime.matches_type_in(
                    value,
                    ty,
                    caller.loaded(),
                    caller
                        .environment()
                        .as_ref()
                        .map(|environment| environment.types.as_ref()),
                ) {
                    return Err(invalid());
                }
            }
            (loaded, *target, runtime.gc.alloc_environment(environment)?)
        };
        self.push_arguments(
            runtime,
            loaded,
            target,
            FrameArguments::plain(args),
            return_dst,
            FrameDispatch {
                prepared: None,
                entry: FrameEntry::Call,
                interface_method: None,
                environment: Some(environment),
            },
        )
    }
}

impl Runtime {
    pub(crate) fn prepare_shared_environment(
        &self,
        caller: &LoadedModule,
        caller_environment: Option<TypeEnvironment>,
        owner: &LoadedModule,
        target: CallableTarget,
        contract: &SharedCall<DefinitionId>,
    ) -> Result<EnvironmentRecord, RuntimeError> {
        #[cfg(feature = "execution-diagnostics")]
        diagnostics::record(Event::SharedPreparation);
        self.validate_loaded_module(caller)?;
        self.validate_loaded_module(owner)?;
        let body = match target {
            CallableTarget::Script(target) => owner
                .bytecode
                .functions
                .get(target.index())
                .and_then(|function| function.metadata.semantic.generic.as_ref()),
            CallableTarget::Native(target) => owner
                .bytecode
                .native_imports
                .get(target.index())
                .and_then(|import| import.generic.as_ref()),
        }
        .ok_or_else(|| RuntimeError::module_validation("shared call body"))?;
        let arguments = self.type_arguments(
            caller,
            caller_environment
                .as_ref()
                .map(|environment| environment.types.clone()),
            &contract.arguments,
        )?;
        let mut environment = EnvironmentRecord::new(
            self.definition_context(),
            body.parameters.clone(),
            arguments,
        )?;
        environment.extend_operations(self.bind_operations_in(
            caller,
            caller_environment,
            &contract.operations,
        )?);
        Ok(environment)
    }
}
