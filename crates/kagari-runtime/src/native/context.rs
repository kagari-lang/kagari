//! Synchronous native access borrows stable, already rooted caller slots.
use crate::{
    Runtime,
    error::RuntimeError,
    frame::types::{
        TypeEnvironment,
        arguments::{ScopedSignature, TypeArgument, type_parameter},
        compatibility::TypeView,
    },
    gc::{GcCollection, GcHeap, HeapObjectId, RootSet, custom_keys::KeyLookupGuard},
    module::{LoadedModule, RetainedRuntimeProgram},
    native::{
        arguments::CallArguments,
        binding::{LinkedNativeFunction, NativeResult},
        callable::{CallableHandle, PreparedClosure},
        declarations::SelectedCall,
        scalar::NativeScalar,
        sequence::{NativeElement, SequencePayload},
        storage::{NativePayload, StorageContext},
        storage_type::StorageType,
    },
    value::Value,
};
use kagari_abi::{native_import::callables::NativeCallableRequirement, types::AbiType};
use kagari_abi::{operations::IterOp, standard::RuntimePrimitive};
use kagari_bytecode::{instruction::Register, module::CallableTarget, program::ModuleRef};
use kagari_common::identity::reference::DefinitionReference;
use kagari_common::identity::table::DefinitionId;
use std::{rc::Rc, slice};

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
    roots: Option<&'call RootSet>,
    slots: ArgumentSlots<'call>,
}

impl<'call> ArgumentView<'call> {
    pub(crate) fn new(roots: &'call RootSet, slots: ArgumentSlots<'call>) -> Self {
        Self {
            roots: Some(roots),
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
        self.rooted_slot(index)
            .is_some_and(|slot| self.roots.is_some_and(|roots| roots.contains_slot(slot)))
    }

    // Read immutable scalar data without an owning clone. The closure cannot
    // retain a slot reference; callers must not reenter while it is borrowed.
    pub(crate) fn with_value<R>(&self, index: usize, read: impl FnOnce(&Value) -> R) -> Option<R> {
        if let ArgumentSlots::Scalars(values) = self.slots {
            return values.get(index).map(read);
        }
        self.roots?.with_value(self.rooted_slot(index)?, read)
    }

    pub fn get(&self, index: usize) -> Option<Value> {
        if let ArgumentSlots::Scalars(values) = self.slots {
            return values.get(index).cloned();
        }
        self.roots?.get(self.rooted_slot(index)?)
    }
}

/// A target selected from a verified program during linking, without source lookup.
#[derive(Debug, Clone)]
pub struct LinkedCallable {
    pub(crate) environment: Option<Rc<TypeEnvironment>>,
    pub(crate) scoped_signature: Option<Rc<ScopedSignature>>,
    pub(crate) owner: CallableOwner,
    pub(crate) target: CallableTarget,
    pub(crate) params: Box<[AbiType<DefinitionId>]>,
    pub(crate) result: AbiType<DefinitionId>,
    pub(crate) primitive: Option<RuntimePrimitive>,
}

#[derive(Debug, Clone)]
pub(crate) enum CallableOwner {
    Program(ModuleRef),
    Pinned(LoadedModule, Rc<RetainedRuntimeProgram>),
}

impl LinkedCallable {
    pub(crate) fn owner(&self, caller: &LoadedModule) -> NativeResult<LoadedModule> {
        match &self.owner {
            CallableOwner::Program(module) => caller
                .member(*module)
                .ok_or_else(|| RuntimeError::module_validation("native callable generation")),
            CallableOwner::Pinned(owner, _retention) => Ok(owner.clone()),
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

    pub(crate) fn begin_key_lookup(&self, index: usize) -> NativeResult<KeyLookupGuard<'call>> {
        let roots = self.arguments.roots.ok_or_else(|| {
            RuntimeError::module_validation("key lookup requires rooted frame arguments")
        })?;
        self.heap().begin_key_lookup(&self.argument(index)?, roots)
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
            AbiType::NativeObject(nominal) => self
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
            scope: self
                .function
                .scoped_signature
                .as_ref()
                .map(|signature| &signature.result),
            selected: &self.function.selected,
        })?;
        let id = self.heap().alloc_native(object)?;
        Ok(match ty {
            AbiType::Array(..) => Value::Array(id),
            AbiType::Map { .. } => Value::Map(id),
            AbiType::Set(..) => Value::Set(id),
            _ => Value::GcHandle(id),
        })
    }

    pub fn allocate_result_payload<S: NativePayload>(&self, payload: S) -> NativeResult<Value> {
        self.heap().ensure_no_native_borrow()?;
        let ty = self.result_type();
        let AbiType::NativeObject(nominal) = ty else {
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
        object.scope = self
            .function
            .scoped_signature
            .as_ref()
            .map(|signature| signature.result.clone());
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
    pub fn resolve_type<I: DefinitionReference>(
        &self,
        ty: &AbiType<I>,
    ) -> NativeResult<TypeArgument> {
        self.runtime
            .resolve_type_arguments(self.owner, slice::from_ref(ty))?
            .pop()
            .ok_or_else(|| RuntimeError::module_validation("native type scope"))
    }

    fn argument_type_argument(&self, index: usize) -> NativeResult<TypeArgument> {
        match &self.function.scoped_signature {
            Some(signature) => signature
                .params
                .get(index)
                .cloned()
                .ok_or_else(|| RuntimeError::module_validation("native argument type scope")),
            None => self.resolve_type(self.argument_type(index)?),
        }
    }

    pub fn argument_type_parameter(
        &self,
        index: usize,
        parameter: usize,
    ) -> NativeResult<TypeArgument> {
        self.argument_type_argument(index)?
            .parameter(self.runtime, self.owner, parameter)
    }

    pub(crate) fn iter_operation(&self, index: usize, op: IterOp) -> NativeResult<Value> {
        self.runtime.iter_operation_with_type(
            self.owner,
            &self.argument(index)?,
            &self.argument_type_argument(index)?,
            op,
        )
    }

    pub fn result_type_parameter(&self, index: usize) -> NativeResult<TypeArgument> {
        match &self.function.scoped_signature {
            Some(signature) => signature.result.parameter(self.runtime, self.owner, index),
            None => {
                self.resolve_type(type_parameter(self.result_type(), index).ok_or_else(|| {
                    RuntimeError::module_validation("native result type parameter")
                })?)
            }
        }
    }

    pub fn allocate_sequence(
        &self,
        element: TypeArgument,
        elements: Vec<Value>,
    ) -> NativeResult<Value> {
        self.heap().ensure_no_native_borrow()?;
        element.validate(self.runtime)?;
        self.runtime.validate_heap_payloads(&elements)?;
        let contract = Rc::new(StorageType::prepare_scoped(element, self.owner)?);
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

    pub fn argument_type(&self, index: usize) -> NativeResult<&'call AbiType<DefinitionId>> {
        self.function
            .signature
            .params
            .get(index)
            .ok_or_else(|| RuntimeError::module_validation("native argument type"))
    }

    pub(crate) fn argument_type_view(&self, index: usize) -> NativeResult<TypeView<'call>> {
        match &self.function.scoped_signature {
            Some(signature) => signature.params.get(index).map(|ty| ty.view(self.owner)),
            None => self
                .function
                .signature
                .params
                .get(index)
                .map(|ty| TypeView::new(ty, self.owner, None)),
        }
        .and_then(TypeView::normalized)
        .ok_or_else(|| RuntimeError::module_validation("native argument scope"))
    }

    pub fn result_type(&self) -> &'call AbiType<DefinitionId> {
        &self.function.signature.result
    }

    pub fn selected(&self, key: SelectedCall) -> NativeResult<&'call LinkedCallable> {
        self.function
            .selected
            .get(key.slot)
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
                .invoke_standard_builtin(primitive, arguments)
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
                let function = owner
                    .native_binding(import)
                    .ok_or_else(|| RuntimeError::module_validation("unlinked native callback"))?;
                let mut context = CallContext {
                    runtime: self.runtime,
                    owner: &owner,
                    function: &function,
                    arguments: ArgumentView {
                        roots: None,
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
