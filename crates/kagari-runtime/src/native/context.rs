//! Synchronous Native access borrows stable, source-protected caller slots.
mod allocation;
mod calls;
mod enums;
mod invocation;
pub mod operations;
mod ranges;
mod reads;
mod references;
mod result;
mod scratch;
mod tasks;
#[cfg(test)]
mod tests;
mod values;
use crate::{
    RootedInterfaceMethod, Runtime,
    closure::calls::SelectedClosure,
    error::RuntimeError,
    execution::roots::{DependencyId, ExecutionRoots, LocalMark, SessionId},
    execution_metadata::{
        MetadataEdge,
        applications::{AdmittedApplication, ApplicationRecord},
        managed::{environments::EnvironmentObject, sources::MetadataSource},
    },
    frame::types::{
        TypeEnvironment,
        arguments::{ScopedSignature, TypeArgument},
        compatibility::TypeView,
    },
    module::{ExecutableModule, managed::source::RevisionSource},
    native::{
        binding::{LinkedNativeFunction, NativeResult},
        callable::CallableHandle,
        context::result::{PendingReturn, ReturnSource},
        conversion::{IntoKagari, arguments::ArgumentValues, context::ConversionContext},
        declarations::SelectedCall as DeclaredCall,
        function_handle::PreparedFunction,
        storage::NativePayload,
    },
    resource::native_stack::NativeStackMark,
    value::{
        place::{PublishedResult, ResultPlace},
        view::ValueView,
    },
};
use kagari_bytecode::{module::CallableTarget, program::ModuleRef};
use kagari_contract::standard::RuntimePrimitive;
use kagari_gc::{error::GcError, heap::CollectionReport};
use kagari_types::{declaration::requirement::NativeCallableRequirement, ty::Ty};
use std::{
    marker::PhantomData,
    rc::Rc,
    slice,
    sync::{Arc, OnceLock},
};

/// A target selected from a verified program during linking, without source lookup.
#[derive(Debug, Clone)]
pub struct LinkedCallable {
    pub(crate) environment: Option<TypeEnvironment>,
    pub(crate) scoped_signature: OnceLock<Arc<ScopedSignature>>,
    pub(crate) owner: CallableOwner,
    pub(crate) target: CallableTarget,
    pub(crate) params: Box<[Ty]>,
    pub(crate) result: Ty,
    pub(crate) primitive: Option<RuntimePrimitive>,
}

#[derive(Debug, Clone)]
pub(crate) enum CallableOwner {
    Program(ModuleRef),
    /// An executable edge retained by a descriptor or active execution window.
    Resolved(ExecutableModule),
}

impl LinkedCallable {
    /// Prepare closed lexical facts once for this immutable linked record.
    /// Dynamic applications initialize the same field with their applied facts.
    pub(crate) fn prepare_signature(
        &self,
        runtime: &Runtime,
        caller: &ExecutableModule,
    ) -> NativeResult<()> {
        if self.scoped_signature.get().is_some() {
            return Ok(());
        }
        if self.environment.is_some() {
            return Err(RuntimeError::module_validation(
                "applied callable signature is not prepared",
            ));
        }
        let owner = self.owner(caller)?;
        let params = runtime.type_arguments(&owner, None, &self.params)?;
        let result = runtime
            .type_arguments(&owner, None, slice::from_ref(&self.result))?
            .pop()
            .ok_or_else(|| RuntimeError::module_validation("selected callback result scope"))?;
        self.scoped_signature
            .set(Arc::new(ScopedSignature { params, result }))
            .map_err(|_| {
                RuntimeError::module_validation("duplicate callable signature preparation")
            })
    }

    /// Select under the enclosing conversion's preparation mark. Actual sources
    /// retain lexical environments; the selected body uses its exact origin.
    /// This nonescaping invocation creates no Host root or retention pack.
    pub(crate) fn select_current(
        &self,
        runtime: &mut Runtime,
        caller: &ExecutableModule,
        execution: SessionId,
        protected: &[MetadataSource<'_>],
    ) -> NativeResult<CallSelection> {
        let source = self.owner(caller)?;
        let selected = runtime.select_call_route(&source.bind_call_route(self.target)?)?;
        let environment = self
            .environment
            .as_ref()
            .map(|facts| {
                let environment = runtime
                    .copy_environment_in(execution, facts, protected)?
                    .ok_or_else(|| {
                        RuntimeError::module_validation("callback environment is not protected")
                    })?;
                runtime.transport_environment(execution, &source, &selected, &environment)
            })
            .transpose()?;
        self.prepare_signature(runtime, &source)?;
        let scoped_signature = self
            .scoped_signature
            .get()
            .expect("prepared callable signature")
            .clone();
        let origin = runtime.selection_origin(selected.owner())?;
        Ok(CallSelection {
            owner: selected.owner().clone(),
            target: Self {
                owner: CallableOwner::Resolved(selected.owner().clone()),
                target: selected.target(),
                environment: environment
                    .as_ref()
                    .map(|environment| environment.facts().clone()),
                scoped_signature: OnceLock::from(scoped_signature),
                params: self.params.clone(),
                result: self.result.clone(),
                primitive: self.primitive,
            },
            origin,
            environment: environment.map(|environment| environment.source()),
        })
    }

    pub(crate) fn trace<'a, E, F>(
        &'a self,
        caller: &'a ExecutableModule,
        visit: &mut F,
    ) -> Result<(), E>
    where
        F: FnMut(MetadataEdge<'a>) -> Result<(), E> + ?Sized,
    {
        let owner = match &self.owner {
            CallableOwner::Program(_) => caller,
            CallableOwner::Resolved(owner) => owner,
        };
        visit(MetadataEdge::Program(owner))?;
        if let Some(environment) = &self.environment {
            visit(MetadataEdge::EnvironmentFacts(environment))?;
        }
        Ok(())
    }

    pub(crate) fn owner(&self, caller: &ExecutableModule) -> NativeResult<ExecutableModule> {
        match &self.owner {
            CallableOwner::Program(module) => caller
                .member(*module)
                .ok_or_else(|| RuntimeError::module_validation("native callable generation")),
            CallableOwner::Resolved(owner) => Ok(owner.clone()),
        }
    }
}

/// One ordinary selection, valid only while its conversion/source owner lives.
/// Facts and coordinates survive growth, but do not own independent GC roots.
pub(crate) struct CallSelection {
    pub(in crate::native) owner: ExecutableModule,
    pub(in crate::native) target: LinkedCallable,
    pub(in crate::native) origin: RevisionSource<'static>,
    environment: Option<DependencyId<EnvironmentObject>>,
}

impl CallSelection {
    pub(in crate::native) fn metadata(&self) -> Option<MetadataSource<'static>> {
        self.environment.map(MetadataSource::Environment)
    }
}

#[derive(Debug, Clone)]
pub(crate) enum LinkedOperation {
    Ready(LinkedCallable),
    Forward(NativeCallableRequirement),
}

impl LinkedOperation {
    pub(crate) fn ready(&self) -> Option<&LinkedCallable> {
        match self {
            Self::Ready(callable) => Some(callable),
            Self::Forward(_) => None,
        }
    }
}

/// One ordinary call already selected under the enclosing synchronous activation.
/// It borrows checked metadata and its actual origin; it creates no Host root.
/// Backends must enter this sealed selection before publishing a callee frame.
pub struct SelectedCall<'target> {
    owner: &'target ExecutableModule,
    target: &'target LinkedCallable,
    origin: RevisionSource<'target>,
    environment: Option<MetadataSource<'target>>,
}

impl<'target> SelectedCall<'target> {
    pub(in crate::native) fn new(
        owner: &'target ExecutableModule,
        target: &'target LinkedCallable,
        origin: RevisionSource<'target>,
        environment: Option<MetadataSource<'target>>,
    ) -> Self {
        Self {
            owner,
            target,
            origin,
            environment,
        }
    }

    pub(crate) fn environment_source(&self) -> Option<MetadataSource<'target>> {
        self.environment
    }

    pub fn owner(&self) -> &'target ExecutableModule {
        self.owner
    }

    pub fn target(&self) -> &'target LinkedCallable {
        self.target
    }

    pub(crate) fn origin(&self) -> RevisionSource<'target> {
        self.origin
    }
}

pub enum ScriptCall<'target, 'source> {
    Selected(&'target SelectedCall<'source>),
    Closure(&'target SelectedClosure<'source>),
    Pinned(&'target PreparedFunction),
    Interface(&'target RootedInterfaceMethod),
}

/// Execute the checked target without holding a frame/storage loan. A backend
/// enters its authenticated scope before publishing a frame and completes result
/// publication before retiring that activation's source suffix.
pub type ScriptInvoker = for<'args, 'result> fn(
    &mut Runtime,
    &ExecutableModule,
    ScriptCall<'_, '_>,
    ArgumentValues<'args>,
    ResultPlace<'result>,
) -> NativeResult<PublishedResult<'result>>;

pub struct NativeCall<'call> {
    pub(crate) _thread: PhantomData<Rc<()>>,
    pub(crate) pending: ReturnSource<'call>,
    locals: Option<LocalMark>,
    native_stack: Option<NativeStackMark>,
    pub(crate) runtime: &'call mut Runtime,
    pub(crate) owner: &'call ExecutableModule,
    pub(crate) function: &'call LinkedNativeFunction,
    pub(crate) arguments: ArgumentValues<'call>,
    pub(crate) invoke_script: ScriptInvoker,
}

/// Closed bindings need no application object. Applied bindings borrow the
/// checked record while the Native source owns its actual managed dependency.
pub(crate) enum NativeEntry<'entry> {
    Closed(&'entry LinkedNativeFunction),
    Applied(&'entry AdmittedApplication),
}

impl<'call> NativeCall<'call> {
    /// Admit one synchronous logical Native suffix without a script frame or
    /// copied argument roots. Body state comes from its explicit current/retained
    /// origin; source publication remains exclusive with Runtime.
    pub(crate) fn new(
        runtime: &'call mut Runtime,
        owner: &'call ExecutableModule,
        entry: NativeEntry<'call>,
        body: RevisionSource<'_>,
        arguments: ArgumentValues<'call>,
        invoke_script: ScriptInvoker,
    ) -> NativeResult<Self> {
        let (function, application) = match entry {
            NativeEntry::Closed(function) => (function, None),
            NativeEntry::Applied(application) => {
                let ApplicationRecord::Native(facts) = application.record().as_ref() else {
                    return Err(RuntimeError::module_validation(
                        "Native entry application kind",
                    ));
                };
                if facts.owner.key() != owner.key() {
                    return Err(RuntimeError::module_validation(
                        "Native entry application owner",
                    ));
                }
                (&facts.function, Some(application))
            }
        };
        runtime.resources.ensure_synchronous_thread()?;
        runtime.resources.poll_execution()?;
        if arguments.len() != function.signature.params.len() {
            return Err(RuntimeError::module_validation(
                "native invocation argument count",
            ));
        }
        runtime.validate_executable_module(owner)?;
        let pending = PendingReturn::new(runtime, owner, body, arguments, application)?;
        Ok(Self {
            _thread: Default::default(),
            pending: ReturnSource::Owned(pending),
            locals: None,
            native_stack: None,
            runtime,
            owner,
            function,
            arguments,
            invoke_script,
        })
    }

    pub(in crate::native) fn gc_error(&self, error: GcError) -> RuntimeError {
        match error {
            GcError::ForeignHeap
            | GcError::InvalidRoot
            | GcError::Index
            | GcError::Representation => RuntimeError::module_validation(error.to_string()),
            error => self.runtime.managed_memory_error(error),
        }
    }

    pub fn callable(&mut self, index: usize) -> NativeResult<CallableHandle<'call>> {
        CallableHandle::from_argument(self, index)
    }

    /// Inspect a checked payload while excluding mutable Runtime services.
    /// The callback may query metadata and cannot retain the Rust reference.
    /// ```compile_fail,E0500
    /// use kagari_runtime::native::{context::NativeCall, storage::NativePayload};
    /// fn reenter<T: NativePayload>(call: &mut NativeCall<'_>) {
    ///     call.with_payload::<T, _>(0, |_| { call.collect_garbage()?; Ok(()) }).unwrap();
    /// }
    /// ```
    pub fn with_payload<S: NativePayload, R>(
        &mut self,
        index: usize,
        access: impl for<'payload> FnOnce(&'payload S) -> NativeResult<R>,
    ) -> NativeResult<R> {
        let value = self.argument(index)?;
        self.read_payload(value, access)
    }

    /// Collection sees this call's existing argument/frame roots. A payload borrow
    /// cannot span collection, just as it cannot span script reentry.
    pub fn collect_garbage(&mut self) -> NativeResult<CollectionReport> {
        self.runtime.collect_garbage()
    }

    /// Resolve a scoped type in this native function's lexical program. Result parameters
    /// must use result_type_parameter() to retain their supplying scopes.
    pub fn resolve_type(&self, ty: &Ty) -> NativeResult<TypeArgument> {
        self.runtime
            .resolve_type_arguments(self.owner, slice::from_ref(ty))?
            .pop()
            .ok_or_else(|| RuntimeError::module_validation("native type scope"))
    }

    /// Retain the declared argument's supplying type/layout generation.
    pub fn argument_type_argument(&self, index: usize) -> NativeResult<TypeArgument> {
        self.function
            .type_signature()?
            .params
            .get(index)
            .cloned()
            .ok_or_else(|| RuntimeError::module_validation("native argument type scope"))
    }

    pub fn argument_type_parameter(
        &self,
        index: usize,
        parameter: usize,
    ) -> NativeResult<TypeArgument> {
        self.argument_type_argument(index)?
            .parameter(self.runtime, self.owner, parameter)
    }

    pub fn result_type_parameter(&self, index: usize) -> NativeResult<TypeArgument> {
        self.function
            .type_signature()?
            .result
            .parameter(self.runtime, self.owner, index)
    }

    /// Convert an owned Rust result after all scoped input views are released.
    /// This uses the typed boundary's limits and publishes into this call's
    /// pending source cell before encoder scratch retires.
    pub fn encode_result<T: IntoKagari>(&mut self, value: T) -> NativeResult<()> {
        let signature = self.function.type_signature()?;
        let destination = self.pending_place()?;
        let published = {
            let mut conversion = ConversionContext::in_native_call(self)?;
            let value = conversion.encode_value(&signature.result, value)?;
            destination.publish(conversion.runtime_mut(), &value)?
        };
        self.accept_staged(published)
    }

    /// Retain the complete result type, including nominal payload scopes.
    pub fn result_type_argument(&self) -> NativeResult<TypeArgument> {
        Ok(self.function.type_signature()?.result.clone())
    }

    /// Derive a nested type parameter while retaining its supplying scope.
    pub fn type_parameter(
        &self,
        applied: &TypeArgument,
        index: usize,
    ) -> NativeResult<TypeArgument> {
        applied.validate(self.runtime)?;
        applied.parameter(self.runtime, self.owner, index)
    }

    /// Forward optional enum failure provenance after checked construction.
    /// Both inputs and the returned value stay protected by this call's source.
    pub fn forward_enum_origin(
        &mut self,
        original: &ValueView<'_>,
        value: &ValueView<'_>,
    ) -> NativeResult<ValueView<'call>> {
        self.convert_value(|conversion| conversion.forward_enum_origin(original, value))
    }

    /// Allocate a fixed-length builtin array with an explicit element contract.
    /// The returned view stays protected by this Native call across reentry.
    pub fn allocate_array(
        &mut self,
        element: TypeArgument,
        elements: &[ValueView<'_>],
    ) -> NativeResult<ValueView<'call>> {
        self.convert_value(|conversion| conversion.allocate_array(&element, elements))
    }

    /// Construct a fixed tuple directly from protected call values. Its managed
    /// fields and result remain in this call's source, without escaping Host roots.
    pub fn allocate_tuple<const N: usize>(
        &mut self,
        elements: &[ValueView<'_>; N],
    ) -> NativeResult<ValueView<'call>> {
        self.convert_value(|conversion| conversion.allocate_tuple(elements))
    }

    pub fn owner(&self) -> &ExecutableModule {
        self.owner
    }

    pub fn arguments(&self) -> ArgumentValues<'call> {
        self.arguments
    }

    pub fn argument(&mut self, index: usize) -> NativeResult<ValueView<'call>> {
        self.runtime.resources.ensure_synchronous_thread()?;
        self.runtime.resources.ensure_execution_allowed()?;
        self.runtime
            .memory
            .scope(&self.runtime.resources, |cx, roots| {
                ExecutionRoots::with(roots, cx, |slots| self.arguments.view(slots, index))
            })
            .map_err(|error| self.gc_error(error))
    }

    pub fn argument_type(&self, index: usize) -> NativeResult<&'call Ty> {
        self.function
            .signature
            .params
            .get(index)
            .ok_or_else(|| RuntimeError::module_validation("native argument type"))
    }

    pub(crate) fn argument_type_view(&self, index: usize) -> NativeResult<TypeView<'call>> {
        self.function
            .type_signature()?
            .params
            .get(index)
            .map(|ty| ty.view(self.owner))
            .and_then(TypeView::normalized)
            .ok_or_else(|| RuntimeError::module_validation("native argument scope"))
    }

    pub fn result_type(&self) -> &'call Ty {
        &self.function.signature.result
    }

    /// Borrow checked evidence retained by this call. Escaping typed handles
    /// explicitly promote the selection to independent ownership.
    pub fn selected(&self, key: &DeclaredCall) -> NativeResult<&'call LinkedCallable> {
        self.function.check_requirement(self.owner, key)?;
        self.selected_at(key.slot)
    }

    /// Resolve a declared callback slot, checking that its selected target is ready.
    pub fn selected_at(&self, slot: usize) -> NativeResult<&'call LinkedCallable> {
        self.function
            .selected
            .get(slot)
            .and_then(LinkedOperation::ready)
            .ok_or_else(|| RuntimeError::module_validation("native selected callable slot"))
    }

    pub fn poll(&self) -> NativeResult<()> {
        self.runtime.resources().poll_execution()
    }
}

impl Drop for NativeCall<'_> {
    fn drop(&mut self) {
        if let Some(mark) = self.native_stack.take() {
            self.runtime.resources.leave_native_stack(mark);
        }
        if let Some(mark) = self.locals.take() {
            self.runtime.retire_execution_locals(mark);
        }
        self.pending.retire_owned(self.runtime);
    }
}
