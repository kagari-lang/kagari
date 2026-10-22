//! Queued interface calls retain the checked dispatch selection and result scope.
use crate::{
    Runtime,
    error::RuntimeError,
    frame::{
        factory::{FactoryEntry, QueuedFactory},
        types::arguments::TypeArgument,
    },
    native::{
        binding::NativeResult,
        composition::future_output,
        conversion::{
            FromKagari, KagariType,
            arguments::{IntoKagariArguments, KagariArguments, encode_arguments},
            context::ConversionContext,
        },
        function_handle::asynchronous::FutureOutput,
        interfaces::{
            Interface,
            binding::{InterfaceMember, InterfaceMethod},
        },
    },
    scheduler::result::ScheduledResult,
    session::{ExecutionOptions, ExecutionPhase},
};

/// A checked interface member returning one Future layer. The descriptor retains
/// dispatch type evidence and the lexical output scope, not a particular receiver.
#[derive(Debug)]
pub struct AsyncInterfaceMethod<A, R> {
    binding: InterfaceMethod<A, FutureOutput<R>>,
    output: TypeArgument,
}

impl<A, R> Clone for AsyncInterfaceMethod<A, R> {
    fn clone(&self) -> Self {
        Self {
            binding: self.binding.clone(),
            output: self.output.clone(),
        }
    }
}

impl Runtime {
    pub fn bind_async_interface_method<A: KagariArguments + 'static, R: KagariType + 'static>(
        &self,
        member: &InterfaceMember,
    ) -> NativeResult<AsyncInterfaceMethod<A, R>> {
        let binding = self.bind_interface_method(member)?;
        let output = future_output(
            self,
            &binding.record.member.owner,
            &binding.record.signature.result,
        )?;
        Ok(AsyncInterfaceMethod { binding, output })
    }
}

impl<S> Interface<S> {
    pub fn schedule_async<A: IntoKagariArguments, R: FromKagari + Send + 'static>(
        &self,
        runtime: &Runtime,
        binding: &AsyncInterfaceMethod<A, R>,
        arguments: A,
        mut options: ExecutionOptions,
    ) -> NativeResult<ScheduledResult<R>> {
        runtime.require_idle_driver()?;
        runtime.drain_retired_executions()?;
        if options.phase != ExecutionPhase::Ordinary {
            return Err(RuntimeError::execution_phase_violation(
                "owned candidate factory",
            ));
        }
        let method = self.prepare_call(runtime, &binding.binding)?;
        let member = &binding.binding.record.member;
        let external = options.cancellation.clone();
        options.cancellation = Default::default();
        let session = runtime.begin_interface_execution(&method, options)?;
        let execution = runtime.initialize_owned_factory(session, external, || {
            let mut cx = ConversionContext::new(runtime, &member.owner)?;
            let (_arguments, mut values) =
                encode_arguments(&mut cx, &binding.binding.record.signature.params, arguments)?;
            values
                .try_reserve(1)
                .map_err(|_| RuntimeError::resource_limit("interface receiver argument"))?;
            values.insert(0, *method.receiver());
            runtime.gc().validate_async_values(&values)?;
            let roots = runtime
                .gc()
                .root_execution_values(values.clone())
                .ok_or_else(|| RuntimeError::resource_limit("async interface arguments"))?;
            cx.poll()?;
            Ok(QueuedFactory {
                owner: member.owner.clone(),
                future: binding.binding.record.signature.result.clone(),
                entry: FactoryEntry::Interface {
                    method: Box::new(method),
                    arguments: values,
                    roots,
                },
            })
        })?;
        let owner = member.owner.clone();
        let output = binding.output.clone();
        runtime.schedule_execution(execution, move |runtime, root| {
            let value = root
                .value(runtime.gc())
                .ok_or_else(|| RuntimeError::module_validation("async interface output root"))?;
            ConversionContext::new(runtime, &owner)?.decode_prepared(&output, &value)
        })
    }
}
