//! Cold script frames reuse closure metadata edges without exposing a callable resume body.
use crate::{
    Runtime,
    closure::ClosureTarget,
    error::RuntimeError,
    frame::{
        ExecutionStack, FrameDispatch, FrameEntry, arguments::FrameArguments,
        types::arguments::TypeArgument,
    },
    native::{
        binding::NativeResult,
        future::{ColdFuture, FuturePayload},
    },
    session::ExecutionPhase,
    value::Value,
};
use kagari_bytecode::{instruction::Register, module::CallableTarget};
use kagari_common::identity::table::DefinitionId;
use kagari_contract::ids::FunctionRef;
use kagari_types::ty::Ty;
use std::slice;

impl ExecutionStack<'_> {
    pub fn make_future(
        &self,
        runtime: &Runtime,
        function: FunctionRef,
        values: Vec<Value>,
        future: &Ty<DefinitionId>,
    ) -> NativeResult<Value> {
        self.validate_runtime(runtime)?;
        if runtime.execution_options().phase != ExecutionPhase::Ordinary {
            return Err(RuntimeError::execution_phase_violation(
                "cold Future construction",
            ));
        }
        runtime.gc().validate_async_values(&values)?;
        let frame = self.current()?;
        let owner = frame.loaded();
        let invalid = || RuntimeError::module_validation("cold script Future contract");
        let target = owner
            .bytecode
            .functions
            .get(function.index())
            .ok_or_else(invalid)?;
        if !target.metadata.effects.may_suspend || target.metadata.params.len() != values.len() {
            return Err(invalid());
        }
        let environment = frame.environment();
        let future = frame
            .type_arguments(runtime, slice::from_ref(future))?
            .pop()
            .ok_or_else(invalid)?;
        let output = future.parameter(runtime, owner, 0)?;
        let result = target
            .metadata
            .semantic
            .result
            .as_ref()
            .ok_or_else(invalid)?;
        let result = frame
            .type_arguments(runtime, slice::from_ref(result))?
            .pop()
            .ok_or_else(invalid)?;
        if !output.view(owner).compatible(result.view(owner)) {
            return Err(invalid());
        }
        for (index, value) in values.iter().enumerate() {
            let ty = target
                .metadata
                .semantic
                .params
                .get(&index)
                .ok_or_else(invalid)?;
            let ty = frame
                .type_arguments(runtime, slice::from_ref(ty))?
                .pop()
                .ok_or_else(invalid)?;
            let underlying = if let Value::Cell(id) = value {
                runtime.gc().captured_cell_value(*id).ok_or_else(invalid)?
            } else {
                value.clone()
            };
            if !ty.matches(runtime, &underlying, owner) {
                return Err(invalid());
            }
        }
        let closure = runtime.make_closure(owner, function, values, environment)?;
        let _root = runtime.root_value(closure.clone()).ok_or_else(invalid)?;
        let Ty::NativeObject(nominal) = future.ty() else {
            return Err(invalid());
        };
        let storage = runtime
            .native_entries
            .storage
            .get_id(nominal.declaration)
            .ok_or_else(invalid)?;
        let mut object = storage.prepare_payload(
            runtime.gc(),
            future.ty(),
            FuturePayload {
                cold: Some(ColdFuture::Script(closure)),
            },
            owner,
        )?;
        object.scope = Some(future);
        runtime.gc().alloc_native(object).map(Value::GcHandle)
    }

    pub(super) fn push_future(
        &self,
        runtime: &Runtime,
        value: Value,
        destination: Option<Register>,
        output: &TypeArgument,
    ) -> NativeResult<()> {
        let invalid = || RuntimeError::module_validation("script Future resume contract");
        if !self.can_park(runtime)? {
            return Err(invalid());
        }
        let closure = runtime.resolve_closure(&value)?;
        let ClosureTarget::Script(function) = closure.target else {
            return Err(invalid());
        };
        let target = closure
            .implementation
            .bytecode
            .functions
            .get(function.index())
            .ok_or_else(invalid)?;
        if !target.metadata.effects.may_suspend
            || target.metadata.params.len() != closure.captures.len()
        {
            return Err(invalid());
        }
        let signature = closure.signature(runtime)?;
        let owner = if destination.is_some() {
            self.current()?.loaded().clone()
        } else {
            self.session.root()
        };
        if !signature.params.is_empty()
            || !output
                .view(&owner)
                .compatible(signature.result.view(&closure.implementation))
        {
            return Err(invalid());
        }
        self.push_arguments(
            runtime,
            closure.implementation.clone(),
            CallableTarget::Script(function),
            FrameArguments::plain(&closure.captures),
            destination,
            FrameDispatch {
                entry: FrameEntry::Await,
                interface_method: None,
                environment: closure.environment.clone(),
            },
        )
    }
}
