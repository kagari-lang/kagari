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
    module::{ExecutableModule, descriptors::SharedScope},
    native::context::LinkedCallable,
    value::Value,
};
use kagari_bytecode::module::CallableTarget;
use kagari_contract::callable::shared::SharedCall;

impl ExecutionStack<'_> {
    pub fn push_selected_call(
        &self,
        runtime: &Runtime,
        caller: &ExecutableModule,
        selected: &LinkedCallable,
        args: &[Value],
    ) -> Result<(), RuntimeError> {
        let owner = selected.owner(caller)?;
        runtime.validate_executable_module(&owner)?;
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
                invocation: None,
                environment: selected.environment.clone(),
            },
        )
    }
}

impl Runtime {
    pub(crate) fn prepare_shared_environment(
        &self,
        caller: &ExecutableModule,
        caller_environment: Option<TypeEnvironment>,
        owner: &ExecutableModule,
        target: CallableTarget,
        contract: &SharedCall,
    ) -> Result<TypeEnvironment, RuntimeError> {
        self.validate_executable_module(caller)?;
        self.validate_executable_module(owner)?;
        let scope = SharedScope {
            environment: caller_environment
                .as_ref()
                .map(|environment| environment.id),
            target_owner: owner.key(),
            target,
        };
        if let Some(id) = scope.environment
            && self.gc.environment(id).is_none()
        {
            return Err(RuntimeError::module_validation(
                "invalid shared preparation scope",
            ));
        }
        if let Some(prepared) = self.modules.shared_environment(caller, &scope, contract) {
            return Ok(prepared);
        }
        #[cfg(feature = "execution-diagnostics")]
        diagnostics::record(Event::SharedPreparation);
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
        environment.set_constructions(self.bind_constructions_in(
            caller,
            caller_environment.clone(),
            &contract.constructions,
        )?);
        environment.extend_operations(self.bind_operations_in(
            caller,
            caller_environment,
            &contract.operations,
        )?);
        let prepared = self.alloc_environment(environment)?;
        self.publish_shared_environment(caller, scope, contract, prepared.clone())?;
        Ok(prepared)
    }
}
