//! Enter a shared function using the active, verified call instruction.
use crate::{
    Runtime,
    error::RuntimeError,
    frame::{ExecutionStack, arguments::FrameArguments, types::TypeEnvironment},
    module::LoadedModule,
    native::context::LinkedCallable,
    value::Value,
};
use kagari_bytecode::{
    instruction::{BytecodeInstruction, CallTarget, Register},
    module::CallableTarget,
};
use std::rc::Rc;

impl ExecutionStack {
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
            owner,
            selected.target,
            FrameArguments::plain(args),
            None,
            None,
            selected.environment.clone(),
        )
    }

    pub fn push_shared_call(
        &self,
        runtime: &Runtime,
        args: &[Value],
        return_dst: Option<Register>,
    ) -> Result<(), RuntimeError> {
        self.validate_top()?;
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
            let body = match target {
                CallableTarget::Script(target) => loaded
                    .bytecode
                    .functions
                    .get(target.index())
                    .and_then(|function| function.metadata.semantic.generic.as_ref()),
                CallableTarget::Native(target) => loaded
                    .bytecode
                    .native_imports
                    .get(target.index())
                    .and_then(|import| import.generic.as_ref()),
            }
            .ok_or_else(invalid)?;
            let arguments = runtime.type_arguments(
                caller.loaded(),
                caller.environment(),
                &contract.arguments,
            )?;
            let mut environment = TypeEnvironment::new(
                runtime.definition_context(),
                body.parameters.clone(),
                arguments,
            )?;
            environment.operations = runtime.bind_operations(&caller, &contract.operations)?;
            if args.len() != contract.signature.params.len() {
                return Err(invalid());
            }
            for (value, ty) in args.iter().zip(&contract.signature.params) {
                if !runtime.matches_type_in(
                    value,
                    ty,
                    caller.loaded(),
                    caller.environment().as_deref(),
                ) {
                    return Err(invalid());
                }
            }
            (loaded, *target, Rc::new(environment))
        };
        self.push_arguments(
            loaded,
            target,
            FrameArguments::plain(args),
            return_dst,
            None,
            Some(environment),
        )
    }
}
