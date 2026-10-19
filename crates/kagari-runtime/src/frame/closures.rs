//! Closure selection is call policy; argument publication belongs to frame storage.
use crate::{
    Runtime,
    closure::{ClosureTarget, ClosureValueSnapshot},
    error::{RuntimeError, RuntimeErrorKind},
    frame::{
        ExecutionFrame, ExecutionStack, FrameDispatch, FrameEntry, arguments::FrameArguments,
        driver::ExecutionAction,
    },
    module::{
        execution::calls::{PreparedCall, PreparedClosureCall, PreparedClosureSignature},
        linked_execution::calls::LinkedCall,
    },
    value::Value,
};
use kagari_bytecode::{instruction::Slot, module::CallableTarget};
use kagari_contract::representation::semantic_representation;
use kagari_types::ty::Ty;

impl ExecutionStack<'_> {
    pub fn push_closure(
        &self,
        runtime: &Runtime,
        value: &Value,
        args: &[Value],
        return_dst: Option<Slot>,
    ) -> Result<(), RuntimeError> {
        self.validate_runtime(runtime)?;
        let closure = runtime.resolve_closure(value)?;
        self.push_resolved_closure(
            runtime,
            &closure,
            FrameArguments::plain(args),
            return_dst,
            None,
        )
        .map(|_| ())
    }

    pub(super) fn push_resolved_closure(
        &self,
        runtime: &Runtime,
        closure: &ClosureValueSnapshot,
        explicit: FrameArguments<'_>,
        return_dst: Option<Slot>,
        prepared: Option<&PreparedCall>,
    ) -> Result<ExecutionAction, RuntimeError> {
        runtime.validate_loaded_module(&closure.implementation)?;
        let script_function = match &closure.target {
            ClosureTarget::Native(native) => {
                if !closure.captures.is_empty()
                    || native.signature.params.len() != explicit.len()
                    || !explicit.all(runtime, |index, value| {
                        native.signature.params[index].matches(
                            runtime,
                            value,
                            &closure.implementation,
                        )
                    })?
                {
                    return Err(RuntimeError::module_validation("native closure arguments"));
                }
                let owner = closure.implementation.clone();
                let target = CallableTarget::Native(native.import);
                let environment = closure.environment.clone();
                return self.push_admitted_arguments(
                    runtime,
                    owner,
                    target,
                    explicit,
                    return_dst,
                    FrameDispatch {
                        prepared,
                        entry: FrameEntry::Call,
                        invocation: None,
                        environment,
                    },
                );
            }
            ClosureTarget::Script(function) => *function,
        };
        let function = closure
            .implementation
            .bytecode
            .functions
            .get(script_function.index())
            .ok_or_else(|| RuntimeError::module_validation("invalid closure function"))?;
        let all = explicit.with_prefix(&closure.captures)?;
        if all.len() != function.metadata.params.len()
            || !all.all(runtime, |index, value| {
                value.has_representation(function.metadata.params[index])
            })?
        {
            return Err(RuntimeError::new(
                RuntimeErrorKind::ScriptTrap,
                "closure call contract mismatch",
            ));
        }
        if let Some(environment) = &closure.environment
            && !all.all(runtime, |index, value| {
                function
                    .metadata
                    .semantic
                    .params
                    .get(&index)
                    .is_none_or(|ty| {
                        if index < closure.captures.len() {
                            runtime.matches_capture_type(
                                value,
                                ty,
                                &closure.implementation,
                                &environment.types,
                            )
                        } else {
                            runtime.matches_type_in(
                                value,
                                ty,
                                &closure.implementation,
                                Some(&environment.types),
                            )
                        }
                    })
            })?
        {
            return Err(RuntimeError::module_validation(
                "closure semantic argument mismatch",
            ));
        }
        self.push_admitted_arguments(
            runtime,
            closure.implementation.clone(),
            CallableTarget::Script(script_function),
            all,
            return_dst,
            FrameDispatch {
                prepared,
                entry: FrameEntry::Call,
                invocation: None,
                environment: closure.environment.clone(),
            },
        )
    }
}

impl ExecutionFrame {
    pub(super) fn validate_closure_call(
        &self,
        closure: &ClosureValueSnapshot,
        contract: &PreparedClosureCall,
        linked: Option<&LinkedCall>,
    ) -> Result<(), RuntimeError> {
        let matches = match &contract.signature {
            PreparedClosureSignature::Physical { params, result } => {
                closure.matches_physical_signature(params.iter().copied(), *result)?
            }
            PreparedClosureSignature::Scoped => {
                let Some(LinkedCall::Closure(semantic)) = linked else {
                    return Err(RuntimeError::module_validation(
                        "missing linked closure contract",
                    ));
                };
                let semantic = semantic.as_ref().ok_or_else(|| {
                    RuntimeError::module_validation("shared closure call signature")
                })?;
                let resolved = self.resolve_type(semantic)?;
                let Ty::Function { params, result } = resolved.as_ref() else {
                    return Err(RuntimeError::module_validation("shared closure call type"));
                };
                closure.matches_physical_signature(
                    params.iter().map(semantic_representation),
                    semantic_representation(result),
                )?
            }
        };
        if !matches {
            return Err(RuntimeError::new(
                RuntimeErrorKind::ScriptTrap,
                "closure call contract",
            ));
        }
        Ok(())
    }
}
