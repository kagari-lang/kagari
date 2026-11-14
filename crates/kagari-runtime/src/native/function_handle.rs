//! Typed call routes select current implementations; prepared calls retain exact entries.
pub mod asynchronous;
pub(crate) mod cache;
mod conversion;
mod evidence;
mod execution;
mod selected;

use crate::{
    Runtime,
    error::RuntimeError,
    execution_metadata::managed::environments::EnvironmentObject,
    frame::types::arguments::ScopedSignature,
    module::{
        ExecutableModule, dispatch::CallRoute, managed::source::RevisionSource,
        retention::ProgramLease,
    },
    native::{
        binding::NativeResult,
        context::{LinkedCallable, ScriptCall},
        conversion::{
            FromKagari,
            arguments::{IntoKagariArguments, PreparedArguments},
            context::ConversionContext,
        },
        function_handle::{evidence::EntryBinding, selected::SelectedBinding},
        typed::NativeContext,
    },
    session::ExecutionOptions,
    source::FunctionSourceTarget,
    value::carriers::RootedValue,
};
use kagari_bytecode::module::CallableTarget;
use kagari_gc::root::Root;
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
    environment: Option<Root<EnvironmentObject>>,
}

#[derive(Debug)]
enum Target {
    Entry(LinkedCallable),
    Closure { root: RootedValue },
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

/// One exact execution selection paired with its already checked Rust mapping.
/// Only a typed Function can construct this witness; clones retain the same body.
/// Actual values, Runtime authority and execution policy are checked on each call.
///
/// The result mapping cannot be replaced after preparation:
/// ```compile_fail,E0308
/// use kagari_runtime::native::{function_handle::PreparedHostCall, typed::NativeContext};
/// fn wrong(call: &PreparedHostCall<(), i32>, cx: &mut NativeContext<'_>) {
///     let _: bool = call.call_with_options(cx, (), Default::default()).unwrap();
/// }
/// ```
#[derive(Debug)]
pub struct PreparedHostCall<A, R> {
    selection: Arc<PreparedFunction>,
    mapping: PhantomData<fn(A) -> R>,
}

impl<A, R> Clone for PreparedHostCall<A, R> {
    fn clone(&self) -> Self {
        Self {
            selection: self.selection.clone(),
            mapping: PhantomData,
        }
    }
}

impl<A, R> PreparedHostCall<A, R> {
    pub fn owner(&self) -> &ExecutableModule {
        self.selection.owner()
    }

    /// Inspect or enter the exact backend selection without changing its mapping.
    pub fn selection(&self) -> &PreparedFunction {
        &self.selection
    }
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
    pub fn prepare_call(&self, runtime: &mut Runtime) -> NativeResult<PreparedHostCall<A, R>> {
        runtime.resources().ensure_synchronous_thread()?;
        runtime.resources().ensure_execution_allowed()?;
        let selection = match self.binding.as_ref() {
            FunctionBinding::Entry(entry) => entry.prepare(runtime).map(Arc::new),
            FunctionBinding::Selected(binding) => binding.prepare(runtime).map(Arc::new),
            FunctionBinding::NamedClosure { root, .. } => {
                self.prepare_named_closure(runtime, root).map(Arc::new)
            }
            FunctionBinding::Closure(prepared) => {
                prepared.validate(runtime)?;
                Ok(prepared.clone())
            }
        }?;
        Ok(PreparedHostCall {
            selection,
            mapping: PhantomData,
        })
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
        self.prepare_call(cx.runtime_mut())?
            .call_with_options(cx, arguments, options)
    }
}

impl PreparedFunction {
    pub fn owner(&self) -> &ExecutableModule {
        &self.owner
    }

    pub(crate) fn body_origin(&self) -> RevisionSource<'_> {
        RevisionSource::Host(self._program.revision())
    }
}

impl<A: IntoKagariArguments, R: FromKagari> PreparedHostCall<A, R> {
    /// Execute this exact selection, including conversions and result publication.
    /// This never resolves a live route a second time.
    pub fn call_with_options(
        &self,
        cx: &mut NativeContext<'_>,
        arguments: A,
        options: ExecutionOptions,
    ) -> NativeResult<R> {
        let selected = self.selection();
        selected.validate(cx.runtime_mut())?;
        let invoke = cx.invoke_script.ok_or_else(|| {
            RuntimeError::module_validation("call context has no execution backend")
        })?;
        let mut session = cx
            .runtime_mut()
            .begin_pinned_execution_with_options(selected, options)?;
        let mut conversion = ConversionContext::new(session.runtime_mut(), self.owner())?;
        let mapping = PreparedArguments::<A>::from_binding(&conversion, &selected.signature.params);
        let values = mapping.encode(&mut conversion, arguments)?;
        let (location, destination) = conversion.result_place()?;
        let published = match &selected.target {
            Target::Entry(target) if target.primitive.is_some() => conversion
                .runtime_mut()
                .invoke_standard_builtin(
                    self.owner(),
                    target.primitive.unwrap(),
                    values,
                    destination,
                )
                .map_err(|error| error.into_runtime_error())?,
            _ => invoke(
                conversion.runtime_mut(),
                self.owner(),
                ScriptCall::Pinned(selected),
                values,
                destination,
            )?,
        };
        let value = conversion.published_result(published, location)?;
        // The caller destination protects the result before conversion can poll or reenter.
        conversion.decode_prepared(&selected.signature.result, &value)
    }
}
