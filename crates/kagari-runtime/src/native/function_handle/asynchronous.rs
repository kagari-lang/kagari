//! Checked Future-returning callables queue admission without executing their factory.
use crate::{
    Runtime,
    error::RuntimeError,
    frame::{
        factory::{FactoryEntry, QueuedFactory},
        types::arguments::TypeArgument,
    },
    module::LoadedModule,
    native::{
        binding::NativeResult,
        catalog::DeclarationCatalog,
        composition::future_output,
        conversion::{
            FromKagari, KagariType,
            arguments::{IntoKagariArguments, KagariArguments, encode_arguments},
            context::ConversionContext,
        },
        function_handle::PinnedFunction,
        types::Type,
    },
    scheduler::result::ScheduledResult,
    session::{ExecutionOptions, ExecutionPhase},
};
use std::marker::PhantomData;

/// A pinned callable plus the lexical output scope of exactly one Future layer.
#[derive(Debug)]
pub struct AsyncFunction<A, R> {
    pub(crate) function: PinnedFunction<A, FutureOutput<R>>,
    output: TypeArgument,
}

impl<A, R> Clone for AsyncFunction<A, R> {
    fn clone(&self) -> Self {
        Self {
            function: self.function.clone(),
            output: self.output.clone(),
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
        owner: &LoadedModule,
        path: &str,
    ) -> NativeResult<AsyncFunction<A, R>> {
        AsyncFunction::from_function(self, self.bind_function(owner, path)?)
    }
}

impl<A, R> AsyncFunction<A, R> {
    pub(crate) fn from_function(
        runtime: &Runtime,
        function: PinnedFunction<A, FutureOutput<R>>,
    ) -> NativeResult<Self> {
        let output = future_output(
            runtime,
            function.owner(),
            &function.prepared.signature.result,
        )?;
        Ok(Self { function, output })
    }

    pub fn owner(&self) -> &LoadedModule {
        self.function.owner()
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
        let prepared = self.function.prepared.clone();
        let session = runtime.begin_pinned_execution_with_options(&prepared, options)?;
        let execution = runtime.initialize_owned_factory(session, external, || {
            let mut cx = ConversionContext::new(runtime, self.owner())?;
            let (roots, values) = encode_arguments(&mut cx, &prepared.signature.params, arguments)?;
            runtime.gc().validate_async_values(&values)?;
            cx.poll()?;
            Ok(QueuedFactory {
                owner: self.owner().clone(),
                future: prepared.signature.result.clone(),
                entry: FactoryEntry::Pinned {
                    function: prepared.clone(),
                    arguments: values,
                    roots,
                },
            })
        })?;
        let output = self.output.clone();
        let owner = self.owner().clone();
        runtime.schedule_execution(execution, move |runtime, root| {
            let value = root
                .value(runtime.gc())
                .ok_or_else(|| RuntimeError::module_validation("async result root"))?;
            ConversionContext::new(runtime, &owner)?.decode_prepared(&output, &value)
        })
    }
}
