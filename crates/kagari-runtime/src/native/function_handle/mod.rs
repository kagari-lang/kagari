//! Typed callable handles retain verified targets and their original program.
pub(crate) mod cache;
mod constructors;
mod conversion;
mod evidence;
mod execution;
mod selected;

use crate::{
    error::RuntimeError,
    frame::types::arguments::ScopedSignature,
    gc::roots::RootedValue,
    module::{LoadedModule, retention::ProgramLease},
    native::{
        binding::NativeResult,
        callable::PreparedClosure,
        context::{LinkedCallable, ScriptCall},
        conversion::{
            FromKagari,
            arguments::{IntoKagariArguments, encode_arguments},
            context::ConversionContext,
        },
        typed::NativeContext,
    },
    session::ExecutionOptions,
};
use std::{marker::PhantomData, sync::Arc};

/// A checked backend entry. Construction is private; execution rechecks the
/// owning runtime, live generation and candidate isolation.
#[derive(Debug)]
pub struct PreparedFunction {
    owner: LoadedModule,
    signature: Arc<ScopedSignature>,
    target: Target,
    _program: ProgramLease,
}

#[derive(Debug)]
enum Target {
    Entry(LinkedCallable),
    Closure {
        prepared: PreparedClosure,
        root: RootedValue,
    },
}

/// Retain once, call repeatedly. Clones share the checked descriptor and lease.
#[derive(Debug)]
pub struct PinnedFunction<A, R> {
    prepared: Arc<PreparedFunction>,
    mapping: PhantomData<fn(A) -> R>,
}

impl<A, R> Clone for PinnedFunction<A, R> {
    fn clone(&self) -> Self {
        Self {
            prepared: self.prepared.clone(),
            mapping: PhantomData,
        }
    }
}

impl<A, R> PinnedFunction<A, R> {
    pub fn owner(&self) -> &LoadedModule {
        &self.prepared.owner
    }
}

impl<A: IntoKagariArguments, R: FromKagari> PinnedFunction<A, R> {
    pub fn call(&self, cx: &mut NativeContext<'_>, arguments: A) -> NativeResult<R> {
        self.call_with_options(cx, arguments, cx.runtime().execution_options())
    }

    /// Host drivers may supply root policy; synchronous nested calls still
    /// inherit the active session's cancellation, depth and execution phase.
    pub fn call_with_options(
        &self,
        cx: &mut NativeContext<'_>,
        arguments: A,
        options: ExecutionOptions,
    ) -> NativeResult<R> {
        let runtime = cx.runtime();
        self.prepared.validate(runtime)?;
        let invoke = cx.invoke_script.ok_or_else(|| {
            RuntimeError::module_validation("call context has no execution backend")
        })?;
        let _session = runtime.begin_pinned_execution_with_options(&self.prepared, options)?;
        let mut conversion = ConversionContext::new(runtime, self.owner())?;
        let (_roots, values) =
            encode_arguments(&mut conversion, &self.prepared.signature.params, arguments)?;
        let value = match &self.prepared.target {
            Target::Entry(target) if target.primitive.is_some() => runtime
                .invoke_standard_builtin(self.owner(), target.primitive.unwrap(), &values)
                .map_err(|error| error.into_runtime_error())?,
            _ => invoke(
                runtime,
                self.owner(),
                ScriptCall::Pinned(&self.prepared),
                &values,
            )?,
        };
        // Retention is established before conversion can poll, collect or reenter.
        conversion.decode_prepared(&self.prepared.signature.result, &value)
    }
}
