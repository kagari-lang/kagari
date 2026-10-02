//! Synchronous native access borrows stable, already rooted caller slots.
use crate::{
    Runtime,
    error::RuntimeError,
    gc::{GcCollection, GcHeap, HeapObjectId, RootSet, custom_keys::KeyLookupGuard},
    module::LoadedModule,
    native::{
        arguments::CallArguments,
        binding::{LinkedNativeFunction, NativeResult},
        callable::{CallableHandle, PreparedClosure},
        declarations::SelectedCall,
        scalar::NativeScalar,
        sequence::{NativeElement, SequencePayload},
        storage::{NativePayload, StorageContext},
    },
    value::Value,
};
use kagari_abi::standard::RuntimePrimitive;
use kagari_abi::types::AbiType;
use kagari_bytecode::{instruction::Register, module::CallableTarget, program::ModuleRef};

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
    pub(crate) module: ModuleRef,
    pub(crate) target: CallableTarget,
    pub(crate) params: Box<[AbiType]>,
    pub(crate) result: AbiType,
    pub(crate) primitive: Option<RuntimePrimitive>,
}

/// The backend supplies synchronous execution while Runtime owns the checked target.
/// No execution-frame or storage borrow may span this call.
/// A backend enters Runtime's direct or closure execution scope before pushing
/// the target; the closure scope owns runtime/liveness/candidate entry validation.
pub enum ScriptCall<'target> {
    Direct(CallableTarget),
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
            Value::GcHandle(id) => self.heap().sequence_push(id, self.owner, value),
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
                .get(&nominal.declaration),
            _ => self.heap().default_storage(ty),
        }
        .ok_or_else(|| RuntimeError::module_validation("native result storage is not installed"))?;
        self.runtime.validate_loaded_module(self.owner)?;
        let object = storage.create(&StorageContext {
            runtime: self.runtime,
            owner: self.owner,
            ty,
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
            .get(&nominal.declaration)
            .ok_or_else(|| RuntimeError::module_validation("native storage is not installed"))?;
        self.runtime.validate_loaded_module(self.owner)?;
        let object = storage.prepare_payload(self.heap(), ty, payload, self.owner)?;
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
    pub fn allocate_sequence(&self, element: AbiType, elements: Vec<Value>) -> NativeResult<Value> {
        self.runtime
            .alloc_array(self.owner, element, elements)
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
    pub fn argument_type(&self, index: usize) -> NativeResult<&'call AbiType> {
        self.function
            .signature
            .params
            .get(index)
            .ok_or_else(|| RuntimeError::module_validation("native argument type"))
    }
    pub fn result_type(&self) -> &'call AbiType {
        &self.function.signature.result
    }
    pub fn selected(&self, key: SelectedCall) -> NativeResult<&'call LinkedCallable> {
        self.function
            .selected
            .get(key.slot)
            .ok_or_else(|| RuntimeError::module_validation("native selected callable slot"))
    }
    pub fn call_values(
        &mut self,
        target: &LinkedCallable,
        arguments: &[Value],
    ) -> NativeResult<Value> {
        self.heap().ensure_no_native_borrow()?;
        let owner = self
            .owner
            .member(target.module)
            .ok_or_else(|| RuntimeError::module_validation("native callable generation"))?;
        if arguments.len() != target.params.len()
            || arguments
                .iter()
                .zip(target.params.iter())
                .any(|(value, ty)| !self.runtime.matches_interface_method_abi(value, ty, &owner))
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
                ScriptCall::Direct(target.target),
                arguments,
            )?,
        };
        if !self
            .runtime
            .matches_interface_method_abi(&value, &target.result, &owner)
        {
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
        if R::abi_type() != target.result || !A::matches(&target.params) {
            return Err(RuntimeError::module_validation(
                "native callback conversion differs from its declared result",
            ));
        }
        arguments.with_values(|arguments| {
            if target.primitive.is_none()
                && let CallableTarget::Native(import) = target.target
            {
                let owner = self
                    .owner
                    .member(target.module)
                    .ok_or_else(|| RuntimeError::module_validation("native callback generation"))?;
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
        self.runtime.resources().ensure_execution_allowed()
    }
}
