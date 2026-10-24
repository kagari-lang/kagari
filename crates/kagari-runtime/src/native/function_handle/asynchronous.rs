//! Checked Future-returning callables queue admission without executing their factory.
use crate::{
    Runtime,
    error::RuntimeError,
    frame::{
        factory::{FactoryEntry, QueuedFactory},
        types::arguments::TypeArgument,
    },
    module::{Module, dispatch::CallRoute},
    native::{
        binding::NativeResult,
        catalog::DeclarationCatalog,
        composition::future_output,
        conversion::{
            FromKagari, KagariType,
            arguments::{IntoKagariArguments, KagariArguments, encode_arguments},
            context::ConversionContext,
        },
        function_handle::Function,
        types::Type,
    },
    scheduler::result::ScheduledResult,
    session::{ExecutionOptions, ExecutionPhase},
    source::FunctionSourceTarget,
};
use std::marker::PhantomData;

/// A current-call route returning exactly one Future layer.
#[derive(Debug)]
pub struct AsyncFunction<A, R> {
    pub(crate) function: Function<A, FutureOutput<R>>,
}

impl<A, R> Clone for AsyncFunction<A, R> {
    fn clone(&self) -> Self {
        Self {
            function: self.function.clone(),
        }
    }
}

/// Type-checking marker, not a Rust Future or a script value conversion.
#[derive(Debug)]
pub(crate) struct FutureOutput<R>(PhantomData<fn() -> R>);

impl<R: KagariType> KagariType for FutureOutput<R> {
    fn kagari_type(_: &DeclarationCatalog) -> NativeResult<Type> {
        Err(RuntimeError::module_validation(
            "async output requires an installed Future scope",
        ))
    }

    fn check_type(cx: &ConversionContext<'_>, expected: &TypeArgument) -> NativeResult<()> {
        cx.check_type::<R>(&future_output(cx.runtime(), cx.owner(), expected)?)
    }
}

impl Runtime {
    pub fn bind_async_function<A: KagariArguments + 'static, R: KagariType + 'static>(
        &self,
        owner: &Module,
        path: &str,
    ) -> NativeResult<AsyncFunction<A, R>> {
        AsyncFunction::from_function(self, self.bind_function(owner, path)?)
    }
}

impl<A, R> AsyncFunction<A, R> {
    pub fn source_target(&self) -> FunctionSourceTarget {
        self.function.source_target()
    }

    pub(crate) fn from_function(
        runtime: &Runtime,
        function: Function<A, FutureOutput<R>>,
    ) -> NativeResult<Self> {
        let prepared = function.prepare_call(runtime)?;
        future_output(runtime, prepared.owner(), &prepared.signature.result)?;
        Ok(Self { function })
    }

    pub fn call_route(&self) -> Option<&CallRoute> {
        self.function.call_route()
    }
}

impl<A: IntoKagariArguments, R: FromKagari + Send + 'static> AsyncFunction<A, R> {
    /// Convert and retain arguments now, then queue the checked factory. The
    /// receiver polls only results; a backend scheduler must drive the runtime.
    pub fn schedule(
        &self,
        runtime: &Runtime,
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
        let external = options.cancellation.clone();
        options.cancellation = Default::default();
        let prepared = self.function.prepare_call(runtime)?;
        let output = future_output(runtime, prepared.owner(), &prepared.signature.result)?;
        let session = runtime.begin_pinned_execution_with_options(&prepared, options)?;
        let execution = runtime.initialize_owned_factory(session, external, || {
            let mut cx = ConversionContext::new(runtime, prepared.owner())?;
            let (roots, values) = encode_arguments(&mut cx, &prepared.signature.params, arguments)?;
            runtime.gc().validate_async_values(&values)?;
            cx.poll()?;
            Ok(QueuedFactory {
                owner: prepared.owner().clone(),
                future: prepared.signature.result.clone(),
                entry: FactoryEntry::Pinned {
                    function: prepared.clone(),
                    arguments: values,
                    roots,
                },
            })
        })?;
        let owner = prepared.owner().clone();
        runtime.schedule_execution(execution, move |runtime, root| {
            let value = root
                .value(runtime.gc())
                .ok_or_else(|| RuntimeError::module_validation("async result root"))?;
            ConversionContext::new(runtime, &owner)?.decode_prepared(&output, &value)
        })
    }
}
