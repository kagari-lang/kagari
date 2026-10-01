//! A frame-owned invocation drives native entry state without identifying library methods.
pub mod api;
pub(crate) mod array;
pub mod array_api;
pub mod catalog;
pub mod cmp_api;
pub mod factory;
pub mod math_api;
pub mod numeric_api;
pub mod ops_api;
pub mod option_api;
pub mod packages;
pub mod registration;
pub mod result_api;
pub mod sorting;
pub mod string_api;
use crate::{
    RootedInterfaceMethod, Runtime,
    error::RuntimeError,
    gc::{ClosureValueSnapshot, GcHeap, RootSet},
    module::{LoadedModule, ModuleStore},
    native::registration::NativeRegistration,
    value::Value,
};

use kagari_abi::{
    callable::CallableImplementation,
    native_import::{NativeSignature, callables::NativeCallableApplication},
    operations::IterOp,
    types::{AbiType, NominalAbiType},
};
use kagari_bytecode::{
    instruction::{NativeImportId, Register},
    module::CallableTarget,
};
use std::{iter, rc::Rc};

pub struct NativeCallback {
    pub(crate) target: NativeCallbackTarget,
    pub(crate) arguments: Vec<Value>,
    result: AbiType,
    _roots: RootSet,
}
pub(crate) enum NativeCallbackTarget {
    Closure(ClosureValueSnapshot),
    Interface(Box<RootedInterfaceMethod>),
    Selected {
        implementation: LoadedModule,
        target: CallableTarget,
    },
}
pub enum NativeProgress {
    Continue,
    Callback(Box<NativeCallback>),
    Finished,
}
pub enum NativeAction {
    Continue,
    Callback(NativeCallback),
    Complete(Value),
}

/// Values retained across allocation/callbacks belong in explicit root slots.
/// Native invocation state must never retain a heap/host borrow or an execution-frame borrow.
/// Each advance/receive call performs bounded work. Longer loops must charge and
/// poll explicitly; returning Continue requests another charged driver step.
pub trait NativeInvocationState {
    fn advance(&mut self, context: &mut NativeContext<'_>) -> Result<NativeAction, RuntimeError>;
    fn receive(
        &mut self,
        _context: &mut NativeContext<'_>,
        _value: Value,
    ) -> Result<NativeAction, RuntimeError> {
        Err(RuntimeError::module_validation(
            "native entry received an unrequested callback result",
        ))
    }
}

pub struct NativeContext<'a> {
    runtime: &'a Runtime,
    owner: &'a LoadedModule,
    signature: &'a NativeSignature,
    callables: &'a [NativeCallableApplication],
    roots: &'a RootSet,
    arguments: usize,
}
impl NativeContext<'_> {
    pub(crate) fn iterator_operation(
        &self,
        owner: &LoadedModule,
        value: &Value,
        ty: &AbiType,
        operation: IterOp,
    ) -> Result<Value, RuntimeError> {
        self.runtime.iter_operation(owner, value, ty, operation)
    }
    pub(crate) fn heap_owner(&self) -> Rc<GcHeap> {
        self.runtime.gc.clone()
    }
    pub(crate) fn module_owner(&self) -> LoadedModule {
        self.owner.clone()
    }
    pub(crate) fn module_store(&self) -> ModuleStore {
        self.runtime.modules.clone()
    }
    pub(crate) fn selected_applications(&self) -> &[NativeCallableApplication] {
        self.callables
    }
    pub fn heap(&self) -> &GcHeap {
        self.runtime.gc()
    }
    pub fn charge(&self, steps: u64) -> Result<(), RuntimeError> {
        self.runtime.resources().consume_instruction_steps(steps)
    }
    pub fn poll(&self) -> Result<(), RuntimeError> {
        self.runtime.resources().ensure_execution_allowed()
    }
    pub fn signature(&self) -> &NativeSignature {
        self.signature
    }
    pub fn argument(&self, slot: usize) -> Option<Value> {
        (slot < self.arguments)
            .then(|| self.roots.get(slot))
            .flatten()
    }
    pub fn retained(&self, slot: usize) -> Option<Value> {
        self.arguments
            .checked_add(slot)
            .and_then(|index| self.roots.get(index))
    }
    pub fn retain(&mut self, slot: usize, value: Value) -> Result<(), RuntimeError> {
        let index = self
            .arguments
            .checked_add(slot)
            .ok_or_else(|| RuntimeError::module_validation("invalid native root slot"))?;
        self.roots
            .set(self.heap(), index, value)
            .ok_or_else(|| RuntimeError::module_validation("invalid native root slot or value"))
    }
    pub fn callback(
        &self,
        value: &Value,
        signature: &AbiType,
        arguments: Vec<Value>,
    ) -> Result<NativeCallback, RuntimeError> {
        callback(self.runtime, value, signature, arguments)
    }
    /// Select a method from a checked, rooted interface value. The request keeps
    /// its implementation generation and concrete output signature until return.
    pub fn interface_callback(
        &self,
        value: &Value,
        interface: &NominalAbiType,
        slot: usize,
        arguments: Vec<Value>,
    ) -> Result<NativeCallback, RuntimeError> {
        let method = self
            .runtime
            .resolve_interface_method_slot(value, interface, slot)?;
        let arguments = iter::once(method.receiver().clone())
            .chain(arguments)
            .collect::<Vec<_>>();
        self.runtime
            .validate_interface_method_arguments(&method, &arguments)?;
        let roots = self
            .runtime
            .gc()
            .root_execution_values(arguments.clone())
            .ok_or_else(|| RuntimeError::module_validation("interface callback roots"))?;
        Ok(NativeCallback {
            result: method.return_type().clone(),
            target: NativeCallbackTarget::Interface(Box::new(method)),
            arguments,
            _roots: roots,
        })
    }
    pub fn matches(&self, value: &Value, ty: &AbiType) -> bool {
        self.runtime
            .matches_interface_method_abi(value, ty, self.owner)
    }
    /// Invoke one compiler-selected dependency with its full checked argument list.
    /// Selection and generic specialization have already happened before linking.
    pub fn selected_callback(
        &self,
        slot: usize,
        arguments: Vec<Value>,
    ) -> Result<NativeCallback, RuntimeError> {
        let invalid = || RuntimeError::module_validation("selected native callable contract");
        let selected = self.callables.get(slot).ok_or_else(invalid)?;
        self.selected_application(self.owner, selected, arguments)
    }
    pub(crate) fn selected_application(
        &self,
        owner: &LoadedModule,
        selected: &NativeCallableApplication,
        arguments: Vec<Value>,
    ) -> Result<NativeCallback, RuntimeError> {
        let invalid = || RuntimeError::module_validation("selected native callable contract");
        let implementation = owner
            .members()
            .find(|owner| owner.bytecode.identity == selected.instance.declaration.module)
            .ok_or_else(invalid)?;
        self.runtime.validate_loaded_module(&implementation)?;
        if arguments.len() != selected.signature.params.len()
            || arguments
                .iter()
                .zip(&selected.signature.params)
                .any(|(value, ty)| {
                    !self
                        .runtime
                        .matches_interface_method_abi(value, ty, &implementation)
                })
        {
            return Err(invalid());
        }
        let target = match &selected.implementation {
            CallableImplementation::Script => implementation
                .bytecode
                .functions
                .iter()
                .find(|function| function.identity.as_ref() == Some(&selected.instance))
                .map(|function| CallableTarget::Script(function.id)),
            CallableImplementation::Native(binding) => implementation
                .bytecode
                .native_imports
                .iter()
                .position(|import| {
                    import.instance == selected.instance
                        && &import.binding == binding
                        && import.signature == selected.signature
                })
                .map(|index| CallableTarget::Native(NativeImportId::new(index))),
            CallableImplementation::Required | CallableImplementation::NativeDefault(_) => None,
        }
        .ok_or_else(invalid)?;
        let roots = self
            .heap()
            .root_execution_values(arguments.clone())
            .ok_or_else(invalid)?;
        Ok(NativeCallback {
            target: NativeCallbackTarget::Selected {
                implementation,
                target,
            },
            arguments,
            result: selected.signature.result.clone(),
            _roots: roots,
        })
    }
}

pub(crate) struct NativeInvocation {
    pub(crate) destination: Option<Register>,
    implementation: LoadedModule,
    import: NativeImportId,
    roots: RootSet,
    state: Box<dyn NativeInvocationState>,
    waiting: Option<AbiType>,
    entry: Option<NativeAction>,
    _registration: Rc<NativeRegistration>,
}
impl NativeInvocation {
    pub(crate) fn start(
        runtime: &Runtime,
        implementation: LoadedModule,
        import: NativeImportId,
        arguments: &[Value],
        destination: Option<Register>,
    ) -> Result<Self, RuntimeError> {
        let registration = implementation
            .native_binding(import)
            .ok_or_else(|| RuntimeError::module_validation("native import was not linked"))?;
        let signature = &implementation.bytecode.native_imports[import.index()].signature;
        if arguments.len() != signature.params.len()
            || arguments.iter().zip(&signature.params).any(|(value, ty)| {
                !runtime.matches_interface_method_abi(value, ty, &implementation)
            })
        {
            return Err(RuntimeError::module_validation(
                "native arguments differ from the checked application",
            ));
        }
        let mut values = arguments.to_vec();
        values.resize(arguments.len() + registration.scratch_slots, Value::Unit);
        let roots = runtime
            .gc()
            .root_execution_values(values)
            .ok_or_else(|| RuntimeError::module_validation("native invocation roots"))?;
        let mut context = NativeContext {
            runtime,
            owner: &implementation,
            signature,
            callables: &implementation.bytecode.native_imports[import.index()].callables,
            roots: &roots,
            arguments: arguments.len(),
        };
        let mut state = (registration.entry)(&mut context)?;
        let entry = state.advance(&mut context)?;
        let mut invocation = Self {
            destination,
            implementation,
            import,
            roots,
            state,
            waiting: None,
            entry: None,
            _registration: registration,
        };
        invocation.entry = Some(invocation.validate_action(runtime, entry)?);
        Ok(invocation)
    }
    pub(crate) fn take_entry(&mut self) -> NativeAction {
        self.entry.take().expect("native entry action")
    }
    fn validate_action(
        &mut self,
        runtime: &Runtime,
        action: NativeAction,
    ) -> Result<NativeAction, RuntimeError> {
        match &action {
            NativeAction::Callback(request) => {
                if self.waiting.replace(request.result.clone()).is_some() {
                    return Err(RuntimeError::module_validation(
                        "native entry requested overlapping callbacks",
                    ));
                }
            }
            NativeAction::Complete(value) => {
                let result = &self.implementation.bytecode.native_imports[self.import.index()]
                    .signature
                    .result;
                if !runtime.matches_interface_method_abi(value, result, &self.implementation) {
                    return Err(RuntimeError::module_validation(
                        "native result differs from its checked application",
                    ));
                }
            }
            NativeAction::Continue => {}
        }
        Ok(action)
    }
    pub(crate) fn advance(&mut self, runtime: &Runtime) -> Result<NativeAction, RuntimeError> {
        if self.waiting.is_some() {
            return Err(RuntimeError::module_validation(
                "native callback has not returned",
            ));
        }
        let signature = &self.implementation.bytecode.native_imports[self.import.index()].signature;
        let mut context = NativeContext {
            runtime,
            owner: &self.implementation,
            signature,
            callables: &self.implementation.bytecode.native_imports[self.import.index()].callables,
            roots: &self.roots,
            arguments: signature.params.len(),
        };
        let action = self.state.advance(&mut context)?;
        self.validate_action(runtime, action)
    }
    pub(crate) fn receive(
        &mut self,
        runtime: &Runtime,
        value: Value,
    ) -> Result<NativeAction, RuntimeError> {
        // The returning callable frame has been popped. Keep its result reachable
        // while the native entry receives it, including allocations before retain().
        let _result_root = runtime
            .gc()
            .root_execution_values(vec![value.clone()])
            .ok_or_else(|| RuntimeError::module_validation("native callback return root"))?;
        let expected = self
            .waiting
            .take()
            .ok_or_else(|| RuntimeError::module_validation("unexpected native callback return"))?;
        if !runtime.matches_interface_method_abi(&value, &expected, &self.implementation) {
            return Err(RuntimeError::module_validation(
                "native callback result contract",
            ));
        }
        let signature = &self.implementation.bytecode.native_imports[self.import.index()].signature;
        let mut context = NativeContext {
            runtime,
            owner: &self.implementation,
            signature,
            callables: &self.implementation.bytecode.native_imports[self.import.index()].callables,
            roots: &self.roots,
            arguments: signature.params.len(),
        };
        let action = self.state.receive(&mut context, value)?;
        self.validate_action(runtime, action)
    }
}
fn callback(
    runtime: &Runtime,
    value: &Value,
    signature: &AbiType,
    arguments: Vec<Value>,
) -> Result<NativeCallback, RuntimeError> {
    let AbiType::Function { params, result } = signature else {
        return Err(RuntimeError::module_validation("native callback signature"));
    };
    let closure = runtime.resolve_closure(value)?;
    let function = &closure.implementation.bytecode.functions[closure.function.index()];
    let prefix = closure.captures.len();
    if function.metadata.params.len() != prefix + params.len()
        || function.metadata.semantic.result.as_ref() != Some(result.as_ref())
        || params.iter().enumerate().any(|(slot, expected)| {
            function.metadata.semantic.params.get(&(prefix + slot)) != Some(expected)
        })
        || arguments.len() != params.len()
        || arguments.iter().zip(params).any(|(value, ty)| {
            !runtime.matches_interface_method_abi(value, ty, &closure.implementation)
        })
    {
        return Err(RuntimeError::module_validation(
            "native callback contract mismatch",
        ));
    }
    Ok(NativeCallback {
        target: NativeCallbackTarget::Closure(closure),
        _roots: runtime
            .gc()
            .root_execution_values(
                iter::once(value.clone())
                    .chain(arguments.iter().cloned())
                    .collect(),
            )
            .ok_or_else(|| RuntimeError::module_validation("native callback roots"))?,
        result: result.as_ref().clone(),
        arguments,
    })
}
