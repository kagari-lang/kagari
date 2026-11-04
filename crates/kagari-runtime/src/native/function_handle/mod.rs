//! Typed call routes select current implementations; prepared calls retain exact entries.
pub mod asynchronous;
pub(crate) mod cache;
mod constructors;
mod conversion;
mod evidence;
mod execution;
mod prepared;
mod selected;

use crate::{
    Runtime,
    frame::types::arguments::ScopedSignature,
    gc::roots::RootedValue,
    module::{ExecutableModule, dispatch::CallRoute, retention::ProgramLease},
    native::{
        binding::NativeResult,
        callable::PreparedClosure,
        context::LinkedCallable,
        conversion::{FromKagari, arguments::IntoKagariArguments},
        function_handle::{evidence::EntryBinding, selected::SelectedBinding},
        typed::NativeContext,
    },
    session::ExecutionOptions,
    source::FunctionSourceTarget,
};
use kagari_bytecode::module::CallableTarget;
use std::{marker::PhantomData, sync::Arc};

/// An erased backend selection retaining its exact body, signature and program.
/// Typed invocation is owned by PreparedCall; this object supplies backend entry.
#[derive(Debug)]
pub struct PreparedFunction {
    source_entry: CallableTarget,
    owner: ExecutableModule,
    signature: Arc<ScopedSignature>,
    target: Target,
    _program: ProgramLease,
}

/// One exact executable selection with its admitted Rust argument/result mapping.
///
/// Obtained from Function::prepare_call. Arguments and results cannot be rebound
/// to different Rust types. Actual values and mutable execution state stay checked.
///
/// ```compile_fail
/// use kagari_runtime::native::{function_handle::PreparedCall, typed::NativeContext};
/// fn wrong_result(call: &PreparedCall<(), u64>, cx: &mut NativeContext<'_>) -> bool {
///     call.call_with_options(cx, (), Default::default()).unwrap()
/// }
/// ```
#[derive(Debug)]
pub struct PreparedCall<A, R> {
    selection: Arc<PreparedFunction>,
    mapping: PhantomData<fn(A) -> R>,
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
    pub fn prepare_call(&self, runtime: &Runtime) -> NativeResult<PreparedCall<A, R>> {
        Ok(PreparedCall {
            selection: self.prepare_selection(runtime)?,
            mapping: PhantomData,
        })
    }

    fn prepare_selection(&self, runtime: &Runtime) -> NativeResult<Arc<PreparedFunction>> {
        runtime.gc().ensure_no_native_borrow()?;
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
}
