//! A task factory and its returned Future share one owned execution lifetime.
use crate::{
    RootedInterfaceMethod, Runtime,
    error::RuntimeError,
    error_trace::asynchronous::CallableOrigin,
    execution::roots::{ExecutionRoots, pending::PendingSlots, queued::QueuedArguments},
    execution_metadata::managed::sources::MetadataSource,
    frame::{
        FrameStack, transfer::ReturnValue, types::arguments::TypeArgument, waiting::QueuedFuture,
    },
    module::ExecutableModule,
    native::{
        binding::NativeResult,
        composition::future_output,
        conversion::{arguments::ArgumentValues, context::ConversionContext},
        function_handle::PreparedFunction,
    },
    value::{carriers::RootedValue, view::ValueView},
    value_check::{data::async_protected, scoped},
};

use kagari_bytecode::module::CallableTarget;
use std::{slice, sync::Arc};

#[derive(Debug)]
pub(crate) struct QueuedFactory {
    pub owner: ExecutableModule,
    pub entry: FactoryEntry,
    pub future: TypeArgument,
}

#[derive(Debug)]
pub(crate) enum FactoryEntry {
    Closure {
        value: RootedValue,
        origin: CallableOrigin,
    },
    Pinned {
        function: Arc<PreparedFunction>,
        arguments: QueuedArguments,
    },
    Interface {
        method: Box<RootedInterfaceMethod>,
        arguments: QueuedArguments,
    },
}

impl QueuedFactory {
    pub(crate) fn origin(&self) -> CallableOrigin {
        match &self.entry {
            FactoryEntry::Closure { origin, .. } => *origin,
            _ => unreachable!("Task admission uses a closure factory"),
        }
    }
}

impl Runtime {
    pub(crate) fn prepare_future_factory(
        &mut self,
        root: &RootedValue,
    ) -> NativeResult<QueuedFactory> {
        let mut scope = self.host_scope(&[])?;
        let value = scope.import(root)?;
        scope
            .runtime_mut()
            .prepare_future_factory_view(&value, Some(root))
    }

    /// Native inputs already have an execution source. Only the queued factory
    /// acquires durable ownership; an existing exact Host root can share its lease.
    pub(crate) fn prepare_future_factory_view(
        &mut self,
        value: &ValueView<'_>,
        original: Option<&RootedValue>,
    ) -> NativeResult<QueuedFactory> {
        async_protected(self, slice::from_ref(value))?;
        let execution = value
            .location()
            .ok_or_else(|| RuntimeError::module_validation("factory source"))?
            .execution;
        let selected = self.select_closure_call(execution, value)?;
        let signature = selected.record().signature().clone();
        let invalid = || RuntimeError::module_validation("task factory requires Fn() -> Future<T>");
        if !signature.params.is_empty()
            || matches!(selected.target(), CallableTarget::Script(function)
            if selected.owner().bytecode.functions[function.index()].metadata.effects.may_suspend)
        {
            return Err(invalid());
        }
        future_output(self, selected.owner(), &signature.result)?;
        let root = if selected.record().call_route().is_some() {
            let mut conversion = ConversionContext::for_selected(
                self,
                selected.owner(),
                execution,
                selected.origin(),
                selected.environment().map(|(_, source)| source),
            )?;
            let frozen = conversion.freeze_closure(&selected)?;
            conversion.retain(&frozen)?
        } else if let Some(root) = original {
            root.clone()
        } else {
            let mut conversion = ConversionContext::for_selected(
                self,
                selected.owner(),
                execution,
                selected.origin(),
                selected.environment().map(|(_, source)| source),
            )?;
            conversion.retain(value)?
        };
        Ok(QueuedFactory {
            entry: FactoryEntry::Closure {
                value: root,
                origin: CallableOrigin {
                    epoch: selected.owner().epoch.0,
                    code_fingerprint: selected.owner().program_fingerprint(),
                    module: selected.owner().slot(),
                    target: selected.target(),
                },
            },
            owner: selected.owner().clone(),
            future: signature.result.clone(),
        })
    }
}

impl FrameStack {
    pub(super) fn start_queued_factory(&self, runtime: &mut Runtime) -> NativeResult<()> {
        let queued = self
            .session_state(runtime)
            .queued_factory
            .borrow_mut()
            .take();
        let Some(queued) = queued else {
            return Ok(());
        };
        if !self.can_park(runtime)? || !self.frames(runtime)?.is_empty() {
            return Err(RuntimeError::module_validation(
                "invalid factory activation",
            ));
        }
        match queued.entry {
            FactoryEntry::Closure { value, .. } => {
                let pending = PendingSlots::new(self.session);
                runtime
                    .memory
                    .scope(&runtime.resources, |cx, roots| {
                        ExecutionRoots::with(roots, cx, |slots| {
                            let value = value.get(slots.scope())?;
                            slots.set(pending.awaitable(), value)
                        })
                    })
                    .map_err(|error| runtime.managed_memory_error(error))?;
                let view = runtime
                    .memory
                    .inspect_value(pending.awaitable())
                    .map_err(|error| runtime.managed_memory_error(error))?;
                async_protected(runtime, slice::from_ref(&view))?;
                self.push_rooted_closure(runtime, &value, ArgumentValues::scalars(&[]))?;
                runtime
                    .memory
                    .scope(&runtime.resources, |cx, roots| {
                        ExecutionRoots::with(roots, cx, |slots| pending.release_awaitable(slots))
                    })
                    .map_err(|error| runtime.managed_memory_error(error))?;
            }
            FactoryEntry::Pinned {
                function,
                arguments,
            } => {
                let result =
                    self.push_pinned_call(runtime, &function, ArgumentValues::queued(&arguments));
                let retired = runtime.memory.scope(&runtime.resources, |cx, roots| {
                    ExecutionRoots::with(roots, cx, |slots| slots.retire_queue(arguments))
                });
                retired.map_err(|error| runtime.managed_memory_error(error))?;
                result?;
            }
            FactoryEntry::Interface { method, arguments } => {
                // Aliases may mutate parked data after queue admission. Check
                // receiver and explicit arguments again before frame publication.
                {
                    let selected = method.implementation(runtime)?;
                    let mut conversion = ConversionContext::for_selected(
                        runtime,
                        &selected,
                        self.session,
                        method.body_origin()?,
                        method
                            .computation_root
                            .as_ref()
                            .map(MetadataSource::HostApplication),
                    )?;
                    let receiver = conversion.import(&method._root)?;
                    async_protected(conversion.runtime_mut(), slice::from_ref(&receiver))?;
                }
                let result = self.push_interface_method(
                    runtime,
                    &method,
                    ArgumentValues::queued(&arguments),
                    None,
                );
                let retired = runtime.memory.scope(&runtime.resources, |cx, roots| {
                    ExecutionRoots::with(roots, cx, |slots| slots.retire_queue(arguments))
                });
                retired.map_err(|error| runtime.managed_memory_error(error))?;
                result?;
            }
        }
        *self.session_state(runtime).factory_output.borrow_mut() = Some(queued.future);
        Ok(())
    }

    pub(super) fn finish_factory_result(
        &self,
        runtime: &mut Runtime,
        value: ReturnValue,
    ) -> NativeResult<Option<ReturnValue>> {
        // A synchronous callback can finish its local stack scope while the
        // factory and native caller are still present in the owning session.
        if !self.frames(runtime)?.is_empty() {
            return Ok(Some(value));
        }
        let future = self
            .session_state(runtime)
            .factory_output
            .borrow_mut()
            .take();
        let Some(future) = future else {
            return Ok(Some(value));
        };
        runtime.resources().poll_execution()?;
        let owner = self.session_state(runtime).root.clone();
        let host = &runtime.host;
        let borrows = &runtime.host_borrows;
        let resources = &runtime.resources;
        let matches = runtime
            .memory
            .scope(&runtime.resources, |cx, roots| {
                ExecutionRoots::with(roots, cx, |slots| {
                    let actual = value.read(slots, self.session)?;
                    Ok(scoped::matches(
                        slots.scope(),
                        actual,
                        future.view(&owner),
                        host,
                        borrows,
                        resources,
                    ))
                })
            })
            .map_err(|error| runtime.managed_memory_error(error))?
            .map_err(|error| {
                let error = error.into_runtime(runtime);
                runtime.resources.admit_execution_error(error)
            })?;
        if !matches {
            return Err(RuntimeError::module_validation(
                "task factory returned an invalid Future",
            ));
        }
        let output = future.parameter(runtime, &owner, 0)?;
        let pending = PendingSlots::new(self.session);
        runtime
            .memory
            .scope(&runtime.resources, |cx, roots| {
                ExecutionRoots::with(roots, cx, |slots| {
                    let value = value.read(slots, self.session)?;
                    pending.clear(slots)?;
                    slots.set(pending.awaitable(), value)
                })
            })
            .map_err(|error| runtime.managed_memory_error(error))?;
        let queued = QueuedFuture::Source { output };
        *self.session_state(runtime).queued_future.borrow_mut() = Some(queued);
        Ok(None)
    }
}
