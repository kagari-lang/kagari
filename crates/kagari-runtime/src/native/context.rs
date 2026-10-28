//! Synchronous native access borrows stable, already rooted caller slots.
mod enums;
mod iteration;
pub mod operations;
mod references;
mod tasks;
#[cfg(test)]
mod tests;
use crate::{
    RootedInterfaceMethod, Runtime,
    error::RuntimeError,
    execution_metadata::{MetadataEdge, MetadataRoot},
    frame::{
        types::{
            TypeEnvironment,
            arguments::{ScopedSignature, TypeArgument},
            compatibility::TypeView,
        },
        values::FrameSlots,
    },
    gc::{
        GcCollection, GcHeap,
        roots::{RootSet, RootedValue},
    },
    module::{ExecutableModule, execution::layout::Location},
    native::{
        arguments::CallArguments,
        binding::{LinkedNativeFunction, NativeResult},
        callable::{CallableHandle, PreparedClosure},
        conversion::{IntoKagari, context::ConversionContext},
        declarations::SelectedCall,
        function_handle::PreparedFunction,
        payload::read::ReadReceiver,
        scalar::NativeScalar,
        storage::{
            NativePayload, StorageContext,
            access::{EditPlan, StorageAccess},
        },
        storage_type::StorageType,
        stored_selection::StoredSelection,
        typed::NativeContext,
    },
    value::Value,
};
use kagari_bytecode::{module::CallableTarget, program::ModuleRef};
use kagari_contract::standard::RuntimePrimitive;
use kagari_types::{declaration::requirement::NativeCallableRequirement, ty::Ty};
use std::{slice, sync::Arc};

#[derive(Clone, Copy)]
pub(crate) enum ArgumentSlots<'call> {
    Prepared(&'call [Location]),
    Contiguous {
        start: usize,
        count: usize,
    },
    /// Only sealed scalar argument packs can construct this unrooted view.
    Scalars(&'call [Value]),
}

#[derive(Clone)]
pub struct ArgumentView<'call> {
    heap: &'call GcHeap,
    frame: Option<FrameSlots>,
    slots: ArgumentSlots<'call>,
    narrowed: Option<RootSet>,
}

impl<'call> ArgumentView<'call> {
    pub(crate) fn frame(
        heap: &'call GcHeap,
        frame: FrameSlots,
        slots: ArgumentSlots<'call>,
    ) -> Self {
        Self {
            heap,
            frame: Some(frame),
            slots,
            narrowed: None,
        }
    }

    pub fn len(&self) -> usize {
        match self.slots {
            ArgumentSlots::Prepared(slots) => slots.len(),
            ArgumentSlots::Contiguous { count, .. } => count,
            ArgumentSlots::Scalars(values) => values.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub(crate) fn contains(&self, index: usize) -> bool {
        if let Some(roots) = &self.narrowed {
            return roots.contains_slot(self.heap, index);
        }
        if let ArgumentSlots::Scalars(values) = self.slots {
            return index < values.len();
        }
        self.with_value(index, |_| ()).is_some()
    }

    /// Read rooted immutable slot data without an owning clone. The closure cannot
    /// retain a slot reference; it must not reenter while this borrow is active.
    pub fn with_value<R>(&self, index: usize, read: impl FnOnce(&Value) -> R) -> Option<R> {
        if let Some(roots) = &self.narrowed {
            return roots.with_value(self.heap, index, read);
        }
        match self.slots {
            ArgumentSlots::Scalars(values) => values.get(index).map(read),
            ArgumentSlots::Prepared(slots) => {
                self.frame?
                    .with_location(self.heap, *slots.get(index)?, read)
            }
            ArgumentSlots::Contiguous { start, count } if index < count => {
                self.frame?
                    .with_value(self.heap, start.checked_add(index)?, read)
            }
            ArgumentSlots::Contiguous { .. } => None,
        }
    }

    pub fn get(&self, index: usize) -> Option<Value> {
        self.with_value(index, Value::clone)
    }
}

/// A target selected from a verified program during linking, without source lookup.
#[derive(Debug, Clone)]
pub struct LinkedCallable {
    pub(crate) environment: Option<TypeEnvironment>,
    pub(crate) scoped_signature: Option<Arc<ScopedSignature>>,
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
    Pinned(ExecutableModule, RootSet),
}

impl LinkedCallable {
    /// Select once at invocation, including callers which bypass VM entry. The
    /// temporary root protects the exact computation through callback/result checks.
    pub(crate) fn select_current(
        &self,
        runtime: &Runtime,
        caller: &ExecutableModule,
    ) -> NativeResult<Self> {
        let source = self.owner(caller)?;

        let selected = runtime.select_call_route(&source.bind_call_route(self.target)?)?;
        let environment = self
            .environment
            .as_ref()
            .map(|environment| runtime.transport_environment(&source, &selected, environment))
            .transpose()?;
        let scoped_signature = match &self.scoped_signature {
            Some(signature) => signature.clone(),
            None => Arc::new(ScopedSignature {
                params: runtime.type_arguments(&source, None, &self.params)?,
                result: runtime
                    .type_arguments(&source, None, slice::from_ref(&self.result))?
                    .pop()
                    .ok_or_else(|| {
                        RuntimeError::module_validation("selected callback result scope")
                    })?,
            }),
        };
        let mut metadata = vec![MetadataRoot::Program(selected.owner().clone())];
        metadata.extend(
            environment
                .iter()
                .map(|environment| MetadataRoot::Environment(environment.id)),
        );
        let roots = runtime.root_metadata(metadata)?;
        Ok(Self {
            owner: CallableOwner::Pinned(selected.owner().clone(), roots),
            target: selected.target(),
            environment,
            scoped_signature: Some(scoped_signature),
            params: self.params.clone(),
            result: self.result.clone(),
            primitive: self.primitive,
        })
    }

    pub(crate) fn scan_field<'a>(
        &'a self,
        caller: &'a ExecutableModule,
        field: usize,
    ) -> Option<MetadataEdge<'a>> {
        match field {
            0 => Some(MetadataEdge::Program(match &self.owner {
                CallableOwner::Program(_) => caller,
                CallableOwner::Resolved(owner) | CallableOwner::Pinned(owner, _) => owner,
            })),
            1 => self
                .environment
                .as_ref()
                .map(|environment| MetadataEdge::Environment(environment.id)),
            _ => None,
        }
    }

    pub(crate) fn trace<'a>(
        &'a self,
        caller: &'a ExecutableModule,
        pending: &mut Vec<MetadataEdge<'a>>,
    ) {
        pending.extend((0..2).filter_map(|field| self.scan_field(caller, field)));
    }

    pub(crate) fn owner(&self, caller: &ExecutableModule) -> NativeResult<ExecutableModule> {
        match &self.owner {
            CallableOwner::Program(module) => caller
                .member(*module)
                .ok_or_else(|| RuntimeError::module_validation("native callable generation")),
            CallableOwner::Pinned(owner, _retention) => Ok(owner.clone()),
            CallableOwner::Resolved(owner) => Ok(owner.clone()),
        }
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

/// The backend supplies synchronous execution while Runtime owns the checked target.
/// No execution-frame or storage borrow may span this call.
/// A backend enters the target's authenticated retained execution scope before
/// pushing it; that scope owns runtime/liveness/candidate entry validation.
pub enum ScriptCall<'target> {
    Closure(&'target PreparedClosure),
    Pinned(&'target PreparedFunction),
    Interface(&'target RootedInterfaceMethod),
}

pub type ScriptInvoker =
    fn(&Runtime, &ExecutableModule, ScriptCall<'_>, &[Value]) -> NativeResult<Value>;

pub struct CallContext<'call> {
    pub(crate) runtime: &'call Runtime,
    pub(crate) owner: &'call ExecutableModule,
    pub(crate) function: &'call LinkedNativeFunction,
    pub(crate) arguments: ArgumentView<'call>,
    pub(crate) invoke_script: ScriptInvoker,
}

impl<'call> CallContext<'call> {
    pub fn callable(&self, index: usize) -> NativeResult<CallableHandle<'call>> {
        CallableHandle::from_argument(self, index)
    }

    pub fn allocate_result(&self) -> NativeResult<Value> {
        self.heap().ensure_no_native_borrow()?;
        let ty = self.result_type();
        let storage = match ty {
            Ty::NativeObject(nominal) => self
                .runtime
                .native_entries
                .storage
                .get_id(nominal.declaration),
            _ => self.heap().default_storage(ty),
        }
        .ok_or_else(|| RuntimeError::module_validation("native result storage is not installed"))?;
        self.runtime.validate_executable_module(self.owner)?;
        let object = storage.create(&StorageContext {
            runtime: self.runtime,
            owner: self.owner,
            ty,
            scope: Some(&self.function.type_signature()?.result),
            selected: &self.function.selected,
        })?;
        let id = self.heap().alloc_native(object)?;
        Ok(match ty {
            Ty::Array(..) => Value::Array(id),

            _ => Value::GcHandle(id),
        })
    }

    /// Build a payload for this call's exact admitted native result.
    ///
    /// Result occurrence, Rust payload mapping and executable publication are checked
    /// before `build` runs. The builder borrows the call's existing roots and receives
    /// its concrete result scope; no owning native type descriptor is constructed.
    /// No module-store or payload borrow spans the builder, so it may collect or
    /// reenter through the call's checked services. It must retain extracted edges
    /// across any such operation, just as other native code must.
    ///
    /// Selected slots are resolved after the builder succeeds, then stored as metadata
    /// edges. Allocation revalidates after builder effects and uses the ordinary
    /// payload scanner/publication path. Errors do not undo completed builder effects.
    /// The returned value is a terminal native-result handoff: publish it before any
    /// subsequent callback or safepoint, or retain it with an explicit root.
    pub fn allocate_result_payload<P: NativePayload>(
        &self,
        selected_slots: &[usize],
        build: impl FnOnce(&CallContext<'_>, &TypeArgument) -> NativeResult<P>,
    ) -> NativeResult<Value> {
        self.heap().ensure_no_native_borrow()?;
        let admitted = self
            .runtime
            .native_payload_result_argument(self.owner, self.function)?;
        let ty = admitted.ty();
        let Ty::NativeObject(nominal) = ty else {
            return Err(RuntimeError::module_validation(
                "provided payload requires a registered native object result",
            ));
        };
        let storage = self
            .runtime
            .native_entries
            .storage
            .get_id(nominal.declaration)
            .ok_or_else(|| RuntimeError::module_validation("native storage is not installed"))?;
        if !storage.accepts_payload::<P>() {
            return Err(RuntimeError::module_validation(
                "native allocation payload type mismatch",
            ));
        }
        // Availability alone permits diagnostic access after an explicit graph fault;
        // executing a builder requires the authoritative publication check as well.
        self.runtime.publish_executable_module(self.owner)?;
        let payload = build(self, &admitted)?;
        let selected = StoredSelection::slots(self.owner, &self.function.selected, selected_slots)?;
        let root = storage.allocate_payload(
            &StorageContext {
                runtime: self.runtime,
                owner: self.owner,
                ty,
                scope: Some(&admitted),
                selected: &[],
            },
            payload,
            selected,
        )?;
        // Raw native-result handoff: no callback, conversion or safepoint follows
        // extraction before the invoking frame publishes this returned Value.
        root.value(self.heap())
            .ok_or_else(|| RuntimeError::module_validation("native result retention"))
    }

    /// Invoke a stored selection from a native object, map or set argument.
    /// The argument roots its relationship evidence; temporary call authority pins
    /// the newly selected computation without changing the root session.
    /// Rejects missing slots, incompatible values and forbidden candidate reentry.
    /// The returned root survives callbacks, mutation and subsequent safepoints.
    pub fn call_stored_values(
        &mut self,
        argument: usize,
        slot: usize,
        values: &[Value],
    ) -> NativeResult<RootedValue> {
        let id = match self.argument(argument)? {
            Value::GcHandle(id) => id,
            _ => return Err(RuntimeError::module_validation("stored callable receiver")),
        };
        let selections = self.heap().native_selections(id)?;
        let target = selections
            .get(slot)
            .ok_or_else(|| RuntimeError::module_validation("stored callable slot"))?
            .retain(self.runtime)?;
        let function = PreparedFunction::selected(self.runtime, self.owner, target)?;
        let context = NativeContext::with_invoker(self.runtime, self.owner, self.invoke_script)?;
        function.call_values(&context, values)
    }

    /// # Safety
    /// Extracted edges must be retained before their owner can change or expire;
    /// interior writes must preserve publication and installed accounting and
    /// respect read-only operation protection.
    pub unsafe fn with_payload<S: NativePayload, R>(
        &self,
        index: usize,
        access: impl for<'payload> FnOnce(&'payload S) -> NativeResult<R>,
    ) -> NativeResult<R> {
        let Value::GcHandle(id) = self.argument(index)? else {
            return Err(RuntimeError::module_validation("native storage argument"));
        };
        self.heap().with_native(id, access)
    }

    /// Read provider storage while borrowing this call's existing argument root.
    ///
    /// # Safety
    /// Retain extracted edges before returning them beyond the call or any mutation
    /// that can remove their source slot. No payload reference may escape the loan.
    pub unsafe fn read_payload<S: NativePayload, R>(
        &self,
        index: usize,
        read: impl for<'payload> FnOnce(&'payload S, &mut StorageAccess<'payload>) -> NativeResult<R>,
    ) -> NativeResult<R> {
        self.heap().read_native(
            self.runtime,
            ReadReceiver::Argument(self, index),
            None,
            read,
        )
    }

    /// Edit provider storage using the call's rooted receiver and the ordinary
    /// preflight, revision, publication, and unwind-accounting protocol.
    ///
    /// # Safety
    /// Publish every imported edge through StorageAccess, keep work state traced,
    /// respect the plan's charge bound, and retain extracted edges before removal.
    pub unsafe fn edit_payload<S: NativePayload, R>(
        &self,
        index: usize,
        plan: EditPlan,
        edit: impl for<'payload> FnOnce(
            &'payload mut S,
            &mut StorageAccess<'payload>,
        ) -> NativeResult<R>,
    ) -> NativeResult<R> {
        self.poll()?;
        let Value::GcHandle(id) = self.argument(index)? else {
            return Err(RuntimeError::module_validation("native storage argument"));
        };
        self.heap().edit_native(self.runtime, id, None, plan, edit)
    }

    /// Collection sees this call's existing argument/frame roots. A payload borrow
    /// cannot span collection, just as it cannot span script reentry.
    pub fn collect_garbage(&self) -> NativeResult<GcCollection> {
        self.runtime.collect_garbage()
    }

    pub fn heap(&self) -> &'call GcHeap {
        self.runtime.gc()
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
    /// This uses the typed boundary's limits and temporary roots. The caller
    /// publishes the returned value before the next safepoint.
    pub fn encode_result<T: IntoKagari>(&self, value: T) -> NativeResult<Value> {
        let signature = self.function.type_signature()?;
        ConversionContext::in_native_call(self)?.encode_value(&signature.result, value)
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
    /// Both values must remain rooted across this allocation.
    pub fn forward_enum_origin(&self, original: &Value, value: &Value) -> NativeResult<Value> {
        self.runtime
            .forward_enum_origin(self.owner, original, value)
    }

    /// Allocate a fixed-length builtin array with an explicit element contract.
    pub fn allocate_array(
        &self,
        element: TypeArgument,
        elements: Vec<Value>,
    ) -> NativeResult<Value> {
        self.heap().ensure_no_native_borrow()?;
        element.validate(self.runtime)?;
        self.runtime.validate_heap_payloads(&elements)?;
        let contract = Arc::new(StorageType::prepare_scoped(element, self.owner)?);
        self.heap()
            .alloc_array_with_contract(contract, elements)
            .map(Value::Array)
    }

    /// Open typed conversion access with this call's selected functions and invoker.
    pub fn native_context(&self) -> NativeResult<NativeContext<'_>> {
        Ok(NativeContext {
            conversion: ConversionContext::in_native_call(self)?,
            invoke_script: Some(self.invoke_script),
            function: Some(self.function),
        })
    }

    pub fn owner(&self) -> &ExecutableModule {
        self.owner
    }

    pub fn arguments(&self) -> ArgumentView<'call> {
        self.arguments.clone()
    }

    pub fn argument(&self, index: usize) -> NativeResult<Value> {
        self.arguments
            .get(index)
            .ok_or_else(|| RuntimeError::module_validation("native argument slot"))
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
    pub fn selected(&self, key: &SelectedCall) -> NativeResult<&'call LinkedCallable> {
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

    pub fn call_values(
        &mut self,
        target: &LinkedCallable,
        arguments: &[Value],
    ) -> NativeResult<Value> {
        self.heap().ensure_no_native_borrow()?;
        let target = target.select_current(self.runtime, self.owner)?;
        self.call_selected_values(&target, arguments)
    }

    fn call_selected_values(
        &mut self,
        target: &LinkedCallable,
        arguments: &[Value],
    ) -> NativeResult<Value> {
        let owner = target.owner(self.owner)?;
        if arguments.len() != target.params.len()
            || arguments
                .iter()
                .enumerate()
                .any(|(index, value)| !target.matches_argument(self.runtime, &owner, index, value))
        {
            return Err(RuntimeError::module_validation("native callback arguments"));
        }
        let value = match target.primitive {
            Some(primitive) => self
                .runtime
                .invoke_standard_builtin(&owner, primitive, arguments)
                .map_err(|error| error.into_runtime_error())?,
            None => {
                // Selection already happened above. Carry exact retained-call
                // authority across the backend boundary, not an ordinary Program
                // entry that is restricted to the old native frame's graph.
                let prepared = PreparedFunction::selected(self.runtime, &owner, target.clone())?;
                (self.invoke_script)(
                    self.runtime,
                    &owner,
                    ScriptCall::Pinned(&prepared),
                    arguments,
                )?
            }
        };
        if !target.matches_result(self.runtime, &owner, &value) {
            return Err(RuntimeError::module_validation("native callback result"));
        }
        Ok(value)
    }

    pub fn call<R: NativeScalar, A: CallArguments>(
        &mut self,
        target: &LinkedCallable,
        arguments: A,
    ) -> NativeResult<R> {
        self.heap().ensure_no_native_borrow()?;
        let target = target.select_current(self.runtime, self.owner)?;
        if Ty::Builtin(R::abi_type()) != target.result || !A::matches(&target.params) {
            return Err(RuntimeError::module_validation(
                "native callback conversion differs from its declared result",
            ));
        }
        arguments.with_values(|arguments| {
            if target.primitive.is_none()
                && target.environment.is_none()
                && let CallableTarget::Native(import) = target.target
            {
                let owner = target.owner(self.owner)?;
                let function = self
                    .runtime
                    .modules
                    .native_binding(&owner, import)
                    .ok_or_else(|| RuntimeError::module_validation("unlinked native callback"))?;
                let mut context = CallContext {
                    runtime: self.runtime,
                    owner: &owner,
                    function: &function,
                    arguments: ArgumentView {
                        heap: self.heap(),
                        frame: None,
                        slots: ArgumentSlots::Scalars(arguments),
                        narrowed: None,
                    },
                    invoke_script: self.invoke_script,
                };
                return function.invoke(&mut context).and_then(R::decode);
            }
            self.call_selected_values(&target, arguments)
                .and_then(R::decode)
        })
    }

    pub fn poll(&self) -> NativeResult<()> {
        self.runtime.resources().poll_execution()
    }
}
