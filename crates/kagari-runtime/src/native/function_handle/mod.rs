//! Typed call routes select current implementations; prepared calls retain exact entries.
pub mod asynchronous;
pub(crate) mod cache;
mod constructors;
mod conversion;
mod evidence;
mod execution;
mod selected;

use crate::{
    Runtime,
    error::RuntimeError,
    frame::types::arguments::ScopedSignature,
    gc::roots::RootedValue,
    module::{ExecutableModule, dispatch::CallRoute, retention::ProgramLease},
    native::{
        binding::NativeResult,
        callable::PreparedClosure,
        context::{LinkedCallable, ScriptCall},
        conversion::{
            FromKagari,
            arguments::{IntoKagariArguments, encode_arguments},
            context::ConversionContext,
        },
        function_handle::{evidence::EntryBinding, selected::SelectedBinding},
        typed::NativeContext,
    },
    session::ExecutionOptions,
    source::FunctionSourceTarget,
};
use kagari_bytecode::module::CallableTarget;
use std::{marker::PhantomData, sync::Arc};

/// One admitted executable selection. Clones of its Arc retain the same body,
/// checked conversion signature and program lease through completion.
#[derive(Debug)]
pub struct PreparedFunction {
    source_entry: CallableTarget,
    owner: ExecutableModule,
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

#[derive(Debug)]
enum FunctionBinding {
    Entry(EntryBinding),
    Selected(SelectedBinding),
    NamedClosure { route: CallRoute, root: RootedValue },
    Closure(Arc<PreparedFunction>),
}

/// A checked callable contract. Ordinary calls select the current implementation
/// of the originally bound declaration, without repeating public-name lookup.
/// Actual closure values retain their own exact computation instead.
#[derive(Debug)]
pub struct Function<A, R> {
    binding: Arc<FunctionBinding>,
    mapping: PhantomData<fn(A) -> R>,
}

impl<A, R> Clone for Function<A, R> {
    fn clone(&self) -> Self {
        Self {
            binding: self.binding.clone(),
            mapping: PhantomData,
        }
    }
}

impl<A, R> Function<A, R> {
    /// Copy diagnostic selection policy without retaining executable admission.
    pub fn source_target(&self) -> FunctionSourceTarget {
        match self.binding.as_ref() {
            FunctionBinding::Closure(prepared) => {
                FunctionSourceTarget::exact(&prepared.owner, prepared.source_entry)
            }
            _ => FunctionSourceTarget::current(
                self.call_route()
                    .expect("ordinary binding has a route")
                    .clone(),
            ),
        }
    }

    /// Inspect the original checked route without selecting or retaining code.
    /// Exact closure computations have no ordinary call route.
    pub fn call_route(&self) -> Option<&CallRoute> {
        match self.binding.as_ref() {
            FunctionBinding::Entry(entry) => Some(entry.route()),
            FunctionBinding::Selected(binding) => Some(&binding.route),
            FunctionBinding::NamedClosure { route, .. } => Some(route),
            FunctionBinding::Closure(_) => None,
        }
    }

    /// Original rooted callable value, when this handle came from script conversion.
    /// Inspection does not select code or change named-versus-exact call policy.
    pub fn closure_value(&self) -> Option<&RootedValue> {
        match self.binding.as_ref() {
            FunctionBinding::NamedClosure { root, .. } => Some(root),
            FunctionBinding::Closure(prepared) => match &prepared.target {
                Target::Closure { root, .. } => Some(root),
                Target::Entry(_) => None,
            },
            _ => None,
        }
    }

    /// Select once, before argument conversion, and retain that exact entry until
    /// execution and result conversion finish. Admission validates Runtime ownership.
    pub fn prepare_call(&self, runtime: &Runtime) -> NativeResult<Arc<PreparedFunction>> {
        runtime.gc().ensure_no_storage_borrow()?;
        match self.binding.as_ref() {
            FunctionBinding::Entry(entry) => entry.prepare(runtime).map(Arc::new),
            FunctionBinding::Selected(binding) => binding.prepare(runtime).map(Arc::new),
            FunctionBinding::NamedClosure { root, .. } => {
                self.prepare_named_closure(runtime, root).map(Arc::new)
            }
            FunctionBinding::Closure(prepared) => {
                prepared.validate(runtime)?;
                Ok(prepared.clone())
            }
        }
    }
}

impl<A: IntoKagariArguments, R: FromKagari> Function<A, R> {
    pub fn call(&self, cx: &mut NativeContext<'_>, arguments: A) -> NativeResult<R> {
        self.call_with_options(cx, arguments, cx.runtime().execution_options())
    }

    /// Nested calls inherit the active session's cancellation, depth and phase.
    pub fn call_with_options(
        &self,
        cx: &mut NativeContext<'_>,
        arguments: A,
        options: ExecutionOptions,
    ) -> NativeResult<R> {
        self.prepare_call(cx.runtime())?
            .call_with_options(cx, arguments, options)
    }
}

impl PreparedFunction {
    pub fn owner(&self) -> &ExecutableModule {
        &self.owner
    }

    /// Execute this exact selection, including conversions and result retention.
    /// This never resolves a live route a second time.
    pub fn call_with_options<A: IntoKagariArguments, R: FromKagari>(
        &self,
        cx: &mut NativeContext<'_>,
        arguments: A,
        options: ExecutionOptions,
    ) -> NativeResult<R> {
        let runtime = cx.runtime();
        self.validate(runtime)?;
        let invoke = cx.invoke_script.ok_or_else(|| {
            RuntimeError::module_validation("call context has no execution backend")
        })?;
        let _session = runtime.begin_pinned_execution_with_options(self, options)?;
        let mut conversion = ConversionContext::new(runtime, self.owner())?;
        // This public backend seam erases the binding's Rust result mapping.
        conversion.check_type::<R>(&self.signature.result)?;
        let (_roots, values) =
            encode_arguments(&mut conversion, &self.signature.params, arguments)?;
        let value = match &self.target {
            Target::Entry(target) if target.primitive.is_some() => runtime
                .invoke_standard_builtin(self.owner(), target.primitive.unwrap(), &values)
                .map_err(|error| error.into_runtime_error())?,
            _ => invoke.invoke(runtime, self.owner(), ScriptCall::Pinned(self), &values)?,
        };
        // Retention is established before conversion can poll, collect or reenter.
        conversion.decode_prepared(&self.signature.result, &value)
    }
}
