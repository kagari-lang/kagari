//! Synchronous native access borrows stable, already rooted caller slots.
mod enums;
pub mod operations;
mod tasks;
use crate::{
    RootedInterfaceMethod, Runtime,
    error::RuntimeError,
    execution_metadata::MetadataEdge,
    frame::{
        types::{
            TypeEnvironment,
            arguments::{ScopedSignature, TypeArgument},
            compatibility::TypeView,
        },
        values::FrameSlots,
    },
    gc::{GcCollection, GcHeap, HeapObjectId, custom_keys::KeyLookupRoots, roots::RootSet},
    module::LoadedModule,
    native::{
        arguments::CallArguments,
        binding::{LinkedNativeFunction, NativeResult},
        callable::{CallableHandle, PreparedClosure},
        context::operations::NativeKeyLookupGuard,
        conversion::{IntoKagari, context::ConversionContext},
        declarations::SelectedCall,
        function_handle::PreparedFunction,
        scalar::NativeScalar,
        sequence::{NativeElement, SequencePayload},
        storage::{NativePayload, StorageContext},
        storage_type::StorageType,
    },
    value::Value,
};
use kagari_bytecode::{instruction::Register, module::CallableTarget, program::ModuleRef};
use kagari_common::identity::{reference::DefinitionReference, table::DefinitionId};
use kagari_contract::{operations::IterOp, standard::RuntimePrimitive};
use kagari_types::{declaration::requirement::NativeCallableRequirement, ty::Ty};
use std::{slice, sync::Arc};

#[derive(Clone, Copy)]
pub enum ArgumentSlots<'call> {
    Registers(&'call [Register]),
    Contiguous {
        start: usize,
        count: usize,
    },
    /// Only sealed scalar argument packs can construct this unrooted view.
    Scalars(&'call [Value]),
}

#[derive(Clone, Copy)]
pub struct ArgumentView<'call> {
    heap: &'call GcHeap,
    frame: Option<FrameSlots>,
    slots: ArgumentSlots<'call>,
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
        }
    }

    pub fn len(&self) -> usize {
        match self.slots {
            ArgumentSlots::Registers(slots) => slots.len(),
            ArgumentSlots::Contiguous { count, .. } => count,
            ArgumentSlots::Scalars(values) => values.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn rooted_slot(&self, index: usize) -> Option<usize> {
        match self.slots {
            ArgumentSlots::Registers(slots) => Some(slots.get(index)?.index()),
            ArgumentSlots::Contiguous { start, count } if index < count => start.checked_add(index),
            ArgumentSlots::Contiguous { .. } | ArgumentSlots::Scalars(_) => None,
        }
    }

    pub(crate) fn contains(&self, index: usize) -> bool {
        if let ArgumentSlots::Scalars(values) = self.slots {
            return index < values.len();
        }
        self.with_value(index, |_| ()).is_some()
    }

    /// Read rooted immutable slot data without an owning clone. The closure cannot
    /// retain a slot reference; it must not reenter while this borrow is active.
    pub fn with_value<R>(&self, index: usize, read: impl FnOnce(&Value) -> R) -> Option<R> {
        if let ArgumentSlots::Scalars(values) = self.slots {
            return values.get(index).map(read);
        }
        let slot = self.rooted_slot(index)?;
        self.frame?.with_value(self.heap, slot, read)
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
    pub(crate) params: Box<[Ty<DefinitionId>]>,
    pub(crate) result: Ty<DefinitionId>,
    pub(crate) primitive: Option<RuntimePrimitive>,
}

#[derive(Debug, Clone)]
pub(crate) enum CallableOwner {
    Program(ModuleRef),
    /// An executable edge retained by a descriptor or active execution window.
    Resolved(LoadedModule),
    Pinned(LoadedModule, RootSet),
}

impl LinkedCallable {
    pub(crate) fn trace<'a>(
        &'a self,
        caller: &'a LoadedModule,
        pending: &mut Vec<MetadataEdge<'a>>,
    ) {
        let owner = match &self.owner {
            CallableOwner::Program(_) => caller,
            CallableOwner::Resolved(owner) | CallableOwner::Pinned(owner, _) => owner,
        };
        pending.push(MetadataEdge::Program(owner));
        if let Some(environment) = &self.environment {
            pending.push(MetadataEdge::Environment(environment.id));
        }
    }

    pub(crate) fn owner(&self, caller: &LoadedModule) -> NativeResult<LoadedModule> {
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
    Forward(NativeCallableRequirement<DefinitionId>),
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
/// A backend enters Runtime's direct or closure execution scope before pushing
/// the target; the closure scope owns runtime/liveness/candidate entry validation.
pub enum ScriptCall<'target> {
    Selected(&'target LinkedCallable),
    Closure(&'target PreparedClosure),
    Pinned(&'target PreparedFunction),
    Interface(&'target RootedInterfaceMethod),
}

pub type ScriptInvoker =
    fn(&Runtime, &LoadedModule, ScriptCall<'_>, &[Value]) -> NativeResult<Value>;

pub struct CallContext<'call> {
    pub(crate) runtime: &'call Runtime,
    pub(crate) owner: &'call LoadedModule,
    pub(crate) function: &'call LinkedNativeFunction,
    pub(crate) arguments: ArgumentView<'call>,
    pub(crate) invoke_script: ScriptInvoker,
}

impl<'call> CallContext<'call> {
    pub fn callable(&self, index: usize) -> NativeResult<CallableHandle<'call>> {
        CallableHandle::from_argument(self, index)
    }

    /// Root a key and block structural mutation until its synchronous lookup ends.
    pub fn begin_key_lookup(&self, index: usize) -> NativeResult<NativeKeyLookupGuard<'call>> {
        let roots = self.arguments.frame.ok_or_else(|| {
            RuntimeError::module_validation("key lookup requires rooted frame arguments")
        })?;
        Ok(NativeKeyLookupGuard {
            _guard: self
                .heap()
                .begin_key_lookup(&self.argument(index)?, KeyLookupRoots::Frame(roots))?,
        })
    }

    fn sequence_id(&self, index: usize) -> NativeResult<HeapObjectId> {
        match self.argument(index)? {
            Value::GcHandle(id) | Value::Array(id) => Ok(id),
            _ => Err(RuntimeError::module_validation("native sequence argument")),
        }
    }

    pub fn sequence_push(&self, index: usize, value: Value) -> NativeResult<()> {
        match self.argument(index)? {
            Value::Array(id) => self.heap().array_push(id, value),
            Value::GcHandle(id) => {
                self.heap()
                    .sequence_push(id, self.argument_type_view(index)?, value)
            }
            _ => Err(RuntimeError::module_validation("native sequence argument")),
        }
    }

    pub fn with_sequence<E: NativeElement, R>(
        &self,
        index: usize,
        access: impl for<'slice> FnOnce(&'slice [E]) -> NativeResult<R>,
    ) -> NativeResult<R> {
        self.heap()
            .with_native::<SequencePayload, R>(self.sequence_id(index)?, |sequence| {
                let values = E::slice(&sequence.values)
                    .ok_or_else(|| RuntimeError::module_validation("sequence scalar layout"))?;
                access(values)
            })
    }

    pub fn with_sequence_mut<E: NativeElement, R>(
        &self,
        index: usize,
        access: impl for<'slice> FnOnce(&'slice mut [E]) -> NativeResult<R>,
    ) -> NativeResult<R> {
        self.heap()
            .with_sequence_mut(self.sequence_id(index)?, access)
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
        self.runtime.validate_loaded_module(self.owner)?;
        let object = storage.create(&StorageContext {
            runtime: self.runtime,
            owner: self.owner,
            ty,
            scope: Some(
                &self
                    .function
                    .type_signature(self.runtime, self.owner)?
                    .result,
            ),
            selected: &self.function.selected,
        })?;
        let id = self.heap().alloc_native(object)?;
        Ok(match ty {
            Ty::Array(..) => Value::Array(id),
            Ty::Map { .. } => Value::Map(id),
            Ty::Set(..) => Value::Set(id),
            _ => Value::GcHandle(id),
        })
    }

    pub fn allocate_result_payload<S: NativePayload>(&self, payload: S) -> NativeResult<Value> {
        self.heap().ensure_no_native_borrow()?;
        let ty = self.result_type();
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
        self.runtime.validate_loaded_module(self.owner)?;
        let mut object = storage.prepare_payload(self.heap(), ty, payload, self.owner)?;
        object.scope = Some(
            self.function
                .type_signature(self.runtime, self.owner)?
                .result
                .clone(),
        );
        self.heap().alloc_native(object).map(Value::GcHandle)
    }

    pub fn with_payload<S: NativePayload, R>(
        &self,
        index: usize,
        access: impl for<'payload> FnOnce(&'payload S) -> NativeResult<R>,
    ) -> NativeResult<R> {
        let Value::GcHandle(id) = self.argument(index)? else {
            return Err(RuntimeError::module_validation("native storage argument"));
        };
        self.heap().with_native(id, access)
    }

    /// Collection sees this call's existing argument/frame roots. A payload borrow
    /// cannot span collection, just as it cannot span script reentry.
    pub fn collect_garbage(&self) -> NativeResult<GcCollection> {
        self.runtime.collect_garbage()
    }

    pub fn heap(&self) -> &'call GcHeap {
        self.runtime.gc()
    }

    /// Resolve a type in this native function's lexical program. Result parameters
    /// must use result_type_parameter() to retain their supplying scopes.
    pub fn resolve_type<I: DefinitionReference>(&self, ty: &Ty<I>) -> NativeResult<TypeArgument> {
        self.runtime
            .resolve_type_arguments(self.owner, slice::from_ref(ty))?
            .pop()
            .ok_or_else(|| RuntimeError::module_validation("native type scope"))
    }

    /// Retain the declared argument's supplying type/layout generation.
    pub fn argument_type_argument(&self, index: usize) -> NativeResult<TypeArgument> {
        self.function
            .type_signature(self.runtime, self.owner)?
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

    /// Apply a checked built-in iterator operation using the argument's declared scope.
    pub fn iter_operation(&self, index: usize, op: IterOp) -> NativeResult<Value> {
        self.runtime.iter_operation_with_type(
            self.owner,
            &self.argument(index)?,
            &self.argument_type_argument(index)?,
            op,
        )
    }

    pub fn result_type_parameter(&self, index: usize) -> NativeResult<TypeArgument> {
        self.function
            .type_signature(self.runtime, self.owner)?
            .result
            .parameter(self.runtime, self.owner, index)
    }

    /// Convert an owned Rust result after all scoped input views are released.
    /// This uses the typed boundary's limits and temporary roots. The caller
    /// publishes the returned value before the next safepoint.
    pub fn encode_result<T: IntoKagari>(&self, value: T) -> NativeResult<Value> {
        let signature = self.function.type_signature(self.runtime, self.owner)?;
        ConversionContext::in_native_call(self)?.encode_value(&signature.result, value)
    }

    /// Retain the complete result type, including nominal payload scopes.
    pub fn result_type_argument(&self) -> NativeResult<TypeArgument> {
        Ok(self
            .function
            .type_signature(self.runtime, self.owner)?
            .result
            .clone())
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

    pub fn allocate_sequence(
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

    pub fn owner(&self) -> &LoadedModule {
        self.owner
    }

    pub fn arguments(&self) -> ArgumentView<'call> {
        self.arguments
    }

    pub fn argument(&self, index: usize) -> NativeResult<Value> {
        self.arguments
            .get(index)
            .ok_or_else(|| RuntimeError::module_validation("native argument slot"))
    }

    pub fn argument_type(&self, index: usize) -> NativeResult<&'call Ty<DefinitionId>> {
        self.function
            .signature
            .params
            .get(index)
            .ok_or_else(|| RuntimeError::module_validation("native argument type"))
    }

    pub(crate) fn argument_type_view(&self, index: usize) -> NativeResult<TypeView<'call>> {
        self.function
            .type_signature(self.runtime, self.owner)?
            .params
            .get(index)
            .map(|ty| ty.view(self.owner))
            .and_then(TypeView::normalized)
            .ok_or_else(|| RuntimeError::module_validation("native argument scope"))
    }

    pub fn result_type(&self) -> &'call Ty<DefinitionId> {
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
            None => (self.invoke_script)(
                self.runtime,
                &owner,
                ScriptCall::Selected(target),
                arguments,
            )?,
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
        if R::abi_type_in() != target.result || !A::matches(&target.params) {
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
                    },
                    invoke_script: self.invoke_script,
                };
                return function.invoke(&mut context).and_then(R::decode);
            }
            self.call_values(target, arguments).and_then(R::decode)
        })
    }

    pub fn poll(&self) -> NativeResult<()> {
        self.runtime.resources().poll_execution()
    }
}
