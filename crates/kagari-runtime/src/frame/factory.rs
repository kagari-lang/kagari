//! A task factory and its returned Future share one owned execution lifetime.
use crate::{
    Runtime,
    closure::ClosureTarget,
    error::RuntimeError,
    frame::{ExecutionStack, types::arguments::TypeArgument, waiting::QueuedFuture},
    gc::roots::RootedValue,
    module::LoadedModule,
    native::binding::NativeResult,
    value::Value,
};
use kagari_types::{
    declaration::{TypeDefKind, native::NativeStorageLayout},
    ty::Ty,
};
use std::slice;

#[derive(Debug)]
pub(crate) struct QueuedFactory {
    pub owner: LoadedModule,
    pub value: RootedValue,
    pub future: TypeArgument,
}

impl Runtime {
    pub(crate) fn prepare_future_factory(&self, value: &Value) -> NativeResult<QueuedFactory> {
        self.gc().validate_async_values(slice::from_ref(value))?;
        let closure = self.resolve_closure(value)?;
        let signature = closure.signature(self)?;
        let invalid = || RuntimeError::module_validation("task factory requires Fn() -> Future<T>");
        if let ClosureTarget::Script(function) = closure.target
            && closure.implementation.bytecode.functions[function.index()]
                .metadata
                .effects
                .may_suspend
        {
            return Err(invalid());
        }
        let Ty::NativeObject(nominal) = signature.result.ty() else {
            return Err(invalid());
        };
        if !signature.params.is_empty()
            || nominal.arguments.len() != 1
            || !nominal.associated_types.is_empty()
            || self
                .native_entries
                .catalog
                .types
                .get_id(nominal.declaration)
                .is_none_or(|ty| ty.kind != TypeDefKind::NativeStorage(NativeStorageLayout::Future))
        {
            return Err(invalid());
        }
        Ok(QueuedFactory {
            owner: closure.implementation.clone(),
            value: self.root_value(value.clone()).ok_or_else(invalid)?,
            future: signature.result.clone(),
        })
    }
}

impl ExecutionStack<'_> {
    pub(super) fn start_queued_factory(&self, runtime: &Runtime) -> NativeResult<()> {
        let queued = self.session.state().queued_factory.borrow_mut().take();
        let Some(queued) = queued else {
            return Ok(());
        };
        if !self.can_park(runtime)? || !self.frames()?.is_empty() {
            return Err(RuntimeError::module_validation(
                "invalid factory activation",
            ));
        }
        let value = queued
            .value
            .value(runtime.gc())
            .ok_or_else(|| RuntimeError::module_validation("factory root"))?;
        runtime
            .gc()
            .validate_async_values(slice::from_ref(&value))?;
        self.push_closure(runtime, &value, &[], None)?;
        *self.session.state().factory_output.borrow_mut() = Some(queued.future);
        Ok(())
    }

    pub(super) fn finish_factory_result(
        &self,
        runtime: &Runtime,
        value: Value,
    ) -> NativeResult<Option<Value>> {
        // A synchronous callback can finish its local stack scope while the
        // factory and native caller are still present in the owning session.
        if !self.frames()?.is_empty() {
            return Ok(Some(value));
        }
        let future = self.session.state().factory_output.borrow_mut().take();
        let Some(future) = future else {
            return Ok(Some(value));
        };
        runtime.resources().poll_execution()?;
        let owner = self.session.root();
        if !future.matches(runtime, &value, &owner) {
            return Err(RuntimeError::module_validation(
                "task factory returned an invalid Future",
            ));
        }
        let queued = QueuedFuture {
            output: future.parameter(runtime, &owner, 0)?,
            value: runtime
                .root_value(value)
                .ok_or_else(|| RuntimeError::module_validation("factory result root"))?,
        };
        *self.session.state().queued_future.borrow_mut() = Some(queued);
        Ok(None)
    }
}
